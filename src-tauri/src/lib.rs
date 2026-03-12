use sha2::{Sha256, Digest};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use sysinfo::{System, ProcessesToUpdate};
use std::collections::HashMap;
use std::env;
use std::thread;
use std::sync::Mutex;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, AppHandle, Emitter, WindowEvent,
};
use notify::{Watcher, RecursiveMode};

// ─── Estado global ────────────────────────────────────────────────────────────
struct LauncherState {
    game_pid:   Mutex<Option<u32>>,
    auth_token: Mutex<Option<String>>,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn get_launcher_dir() -> PathBuf {
    let exe = env::current_exe().expect("Não foi possível localizar o executável");
    exe.parent().expect("Não foi possível achar a pasta raiz").to_path_buf()
}

fn get_local_hash(path: &Path) -> Option<String> {
    let mut file   = File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf    = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    hasher.update(&buf);
    Some(format!("{:x}", hasher.finalize()))
}

fn pid_is_running(pid: u32) -> bool {
    let mut s = System::new();
    s.refresh_processes(ProcessesToUpdate::All, true);
    s.process(sysinfo::Pid::from_u32(pid)).is_some()
}

fn gravar_patch_settings(hwid: &str, token: &str) -> Result<(), String> {
    let ini_path = get_launcher_dir().join("system").join("PatchSettings.ini");
    let conteudo = format!("[L2Guard]\nHWID={}\nTOKEN={}\n", hwid, token);
    fs::write(&ini_path, conteudo)
        .map_err(|e| format!("Erro ao gravar PatchSettings.ini: {}", e))
}

fn deletar_patch_settings() {
    let ini_path = get_launcher_dir().join("system").join("PatchSettings.ini");
    let _ = fs::remove_file(ini_path);
}

fn processo_l2_rodando() -> bool {
    match Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq l2.exe", "/NH", "/FO", "CSV"])
        .output()
    {
        Ok(out) => String::from_utf8_lossy(&out.stdout).to_lowercase().contains("l2.exe"),
        Err(_)  => false,
    }
}

// ─── Comandos Tauri ───────────────────────────────────────────────────────────

#[tauri::command]
fn get_hwid() -> String {
    machine_uid::get().unwrap_or_else(|_| "unknown_hwid".to_string())
}

#[tauri::command]
fn scan_anti_hack() -> bool {
    let mut s = System::new_all();
    s.refresh_all();
    let blacklist = [
        "cheatengine", "adrenaline", "l2tower", "l2net",
        "wireshark", "fiddler", "processhacker", "x96dbg",
        "ollydbg", "ida", "ghidra",
    ];
    for process in s.processes().values() {
        let name = process.name().to_string_lossy().to_lowercase();
        if blacklist.iter().any(|&hack| name.contains(hack)) {
            return false;
        }
    }
    true
}

#[tauri::command]
fn abrir_l2(
    token: String,
    hwid: String,
    _login: String,
    state: tauri::State<LauncherState>,
    app: AppHandle,
) -> Result<u32, String> {
    let root_dir     = get_launcher_dir();
    let caminho_l2   = root_dir.join("system").join("l2.exe");
    let pasta_system = root_dir.join("system");

    if !caminho_l2.exists() {
        return Err(format!("l2.exe não encontrado em: {}", caminho_l2.display()));
    }

    if let Some(pid) = *state.game_pid.lock().unwrap() {
        if pid_is_running(pid) {
            return Err("O jogo já está rodando!".into());
        }
    }

    gravar_patch_settings(&hwid, &token)?;
    *state.auth_token.lock().unwrap() = Some(token.clone());

    let child = Command::new(&caminho_l2)
        .current_dir(&pasta_system)
        .arg("-nointro")
        .spawn()
        .map_err(|e| format!("Falha ao abrir l2.exe: {}", e))?;

    let pid = child.id();
    *state.game_pid.lock().unwrap() = Some(pid);

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    iniciar_watcher(pasta_system.clone(), app.clone());

    let app_monitor = app.clone();
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_secs(5));
        loop {
            thread::sleep(std::time::Duration::from_secs(3));
            if !processo_l2_rodando() {
                deletar_patch_settings();
                if let Some(window) = app_monitor.get_webview_window("main") {
                    let _ = window.emit("game-closed", ());
                    let _ = window.show();
                }
                break;
            }
        }
    });

    Ok(pid)
}

#[tauri::command]
fn is_process_running(pid: u32) -> bool {
    pid_is_running(pid)
}

#[tauri::command]
fn kill_game(state: tauri::State<LauncherState>) -> Result<(), String> {
    let pid_opt = *state.game_pid.lock().unwrap();
    if let Some(pid) = pid_opt {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .output()
            .map_err(|e| format!("Falha ao matar processo: {}", e))?;
        deletar_patch_settings();
        *state.game_pid.lock().unwrap()   = None;
        *state.auth_token.lock().unwrap() = None;
    }
    Ok(())
}

#[tauri::command]
fn get_game_status(state: tauri::State<LauncherState>) -> bool {
    if let Some(pid) = *state.game_pid.lock().unwrap() {
        pid_is_running(pid)
    } else {
        false
    }
}

// ─── Watcher ─────────────────────────────────────────────────────────────────

fn iniciar_watcher(path: PathBuf, app: AppHandle) {
    thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w)  => w,
            Err(e) => { eprintln!("Falha ao criar watcher: {}", e); return; }
        };
        let _ = watcher.watch(&path, RecursiveMode::Recursive);
        let protegidos = ["Interface.u", "L2Guard.dll"];
        for res in rx {
            if let Ok(event) = res {
                let alterou = event.paths.iter().any(|p| {
                    p.file_name()
                        .map(|n| {
                            let n = n.to_string_lossy();
                            protegidos.iter().any(|&prot| n == prot)
                        })
                        .unwrap_or(false)
                });
                if alterou {
                    deletar_patch_settings();
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.emit("kill-game", ());
                    }
                }
            }
        }
    });
}

// ─── Atualização ─────────────────────────────────────────────────────────────

#[tauri::command]
async fn atualizar_arquivos() -> Result<String, String> {
    let url_patchlist = "https://l2eternal.org/patch/patchlist.json";
    let url_base      = "https://l2eternal.org/patch/";
    let root_dir      = get_launcher_dir();
    let client        = reqwest::Client::new();

    let patchlist: HashMap<String, String> = client
        .get(url_patchlist)
        .send()
        .await
        .map_err(|e| format!("Erro de rede: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Erro no JSON: {}", e))?;

    let mut atualizados = 0usize;

    for (arquivo_relativo, hash_remoto) in &patchlist {
        let caminho    = root_dir.join(arquivo_relativo.replace('/', std::path::MAIN_SEPARATOR_STR));
        let hash_local = get_local_hash(&caminho).unwrap_or_default();

        if &hash_local != hash_remoto {
            let bytes = client
                .get(format!("{}{}", url_base, arquivo_relativo))
                .send()
                .await
                .map_err(|e| format!("Erro ao baixar {}: {}", arquivo_relativo, e))?
                .bytes()
                .await
                .map_err(|e| e.to_string())?;

            if let Some(parent) = caminho.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            File::create(&caminho)
                .map_err(|e| e.to_string())?
                .write_all(&bytes)
                .map_err(|e| e.to_string())?;
            atualizados += 1;
        }
    }

    if atualizados == 0 {
        Ok("Todos os arquivos estão atualizados!".to_string())
    } else {
        Ok(format!("{} arquivo(s) atualizado(s)!", atualizados))
    }
}

// ─── Run ─────────────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {

    // ── Auto-elevação: relança como admin se necessário ──
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let is_admin = Command::new("net")
            .args(["session"])
            .creation_flags(0x08000000)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if !is_admin {
            let exe = env::current_exe().unwrap();
            Command::new("powershell")
                .args([
                    "-WindowStyle", "Hidden",
                    "-Command",
                    &format!("Start-Process '{}' -Verb RunAs", exe.display()),
                ])
                .spawn()
                .unwrap();
            std::process::exit(0);
        }
    }

    let state = LauncherState {
        game_pid:   Mutex::new(None),
        auth_token: Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
                let _ = w.emit("force-relogin", ());
            }
        }))
        .manage(state)
        .setup(|app| {
            let show_i = MenuItem::with_id(app, "show", "Mostrar Launcher", true, None::<&str>)?;
            let kill_i = MenuItem::with_id(app, "kill", "Fechar Jogo",      true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Sair do Launcher", true, None::<&str>)?;
            let menu   = Menu::with_items(app, &[&show_i, &kill_i, &quit_i])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("L2 Eternal Launcher")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app: &AppHandle, event| match event.id.as_ref() {
                    "quit" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.emit("kill-game", ());
                        }
                        app.exit(0);
                    }
                    "kill" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.emit("kill-game", ());
                        }
                    }
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            if let Ok(true) = w.is_visible() {
                                let _ = w.hide();
                            } else {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_hwid,
            scan_anti_hack,
            abrir_l2,
            atualizar_arquivos,
            is_process_running,
            kill_game,
            get_game_status,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o launcher");
}
