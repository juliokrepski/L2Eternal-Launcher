<script setup>
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ─── Estado ──────────────────────────────────────────────────────────────────
const login = ref("");
const senha = ref("");
const logado = ref(false);
const jogoRodando = ref(false);
const atualizando = ref(false);
const loginando = ref(false);

const statusMsg = ref("Verificando arquivos...");
const statusTipo = ref("info"); // info | sucesso | erro

const tokenAtual = ref("");
const hwidAtual = ref("");
const loginAtual = ref("");

const progresso = ref(0); // 0–100
const mostrarBarra = ref(false);

// ─── Helpers ─────────────────────────────────────────────────────────────────
function setStatus(msg, tipo = "info") {
  statusMsg.value = msg;
  statusTipo.value = tipo;
}

// ─── Inicialização ────────────────────────────────────────────────────────────
onMounted(async () => {
  // Escuta evento de jogo fechado (emitido pelo Rust)
  await listen("game-closed", () => {
    jogoRodando.value = false;
    setStatus("✅ Jogo encerrado. Pronto para jogar novamente.", "sucesso");
  });

  await listen("kill-game", () => {
    jogoRodando.value = false;
    setStatus("🛑 Jogo encerrado pelo sistema de segurança.", "erro");
  });

  // Monitor periódico do processo
  setInterval(async () => {
    if (jogoRodando.value) {
      const rodando = await invoke("get_game_status");
      if (!rodando) {
        jogoRodando.value = false;
        setStatus("✅ Jogo encerrado. Pronto para jogar novamente.", "sucesso");
      }
    }
  }, 5000);
});

// ─── Login ────────────────────────────────────────────────────────────────────
async function fazerLogin() {
  if (!login.value.trim() || !senha.value.trim()) {
    setStatus("❌ Preencha login e senha.", "erro");
    return;
  }

  loginando.value = true;
  setStatus("🔄 Verificando anti-hack...", "info");

  try {
    // 1. Anti-hack
    const limpo = await invoke("scan_anti_hack");
    if (!limpo) {
      setStatus("❌ Software proibido detectado. Feche-o e tente novamente.", "erro");
      loginando.value = false;
      return;
    }

    // 2. HWID
    setStatus("🔄 Identificando máquina...", "info");
    const hwid = await invoke("get_hwid");
    hwidAtual.value = hwid;

    // 3. API login
    setStatus("🔄 Autenticando...", "info");
    const response = await fetch("https://l2eternal.org/api/auth/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        login: login.value.trim(),
        password: senha.value.trim(),
        hwid: hwid,
      }),
    });

    const data = await response.json();

    if (!data.success) {
      setStatus(`❌ ${data.message || "Erro ao autenticar."}`, "erro");
      loginando.value = false;
      return;
    }

    tokenAtual.value = data.token;
    loginAtual.value = data.login;
    logado.value = true;

    setStatus(`✅ Bem-vindo, ${data.login}!`, "sucesso");

    // 4. Após login, verifica atualizações automaticamente
    await verificarAtualizacoes();
  } catch (e) {
    setStatus(`❌ Erro de conexão: ${e}`, "erro");
  } finally {
    loginando.value = false;
  }
}

// ─── Atualizar arquivos ───────────────────────────────────────────────────────
async function verificarAtualizacoes() {
  atualizando.value = true;
  mostrarBarra.value = true;
  progresso.value = 0;
  setStatus("🔄 Verificando atualizações...", "info");

  // Simula progresso enquanto o Rust trabalha
  const intervalo = setInterval(() => {
    if (progresso.value < 90) progresso.value += 5;
  }, 300);

  try {
    const resultado = await invoke("atualizar_arquivos");
    progresso.value = 100;
    setStatus(`✅ ${resultado}`, "sucesso");
  } catch (e) {
    setStatus(`❌ Erro ao atualizar: ${e}`, "erro");
  } finally {
    clearInterval(intervalo);
    setTimeout(() => {
      atualizando.value = false;
      mostrarBarra.value = false;
      progresso.value = 0;
    }, 1500);
  }
}

// ─── Iniciar jogo ─────────────────────────────────────────────────────────────
async function iniciarJogo() {
  if (!logado.value || !tokenAtual.value) {
    setStatus("❌ Faça login antes de iniciar o jogo.", "erro");
    return;
  }
  if (atualizando.value) {
    setStatus("⏳ Aguarde o download terminar.", "info");
    return;
  }

  setStatus("🔄 Iniciando o jogo...", "info");

  try {
    await invoke("abrir_l2", {
      token: tokenAtual.value,
      hwid: hwidAtual.value,
      login: loginAtual.value,
    });
    jogoRodando.value = true;
    setStatus("🎮 Jogo em execução! Boa sorte!", "sucesso");
  } catch (e) {
    setStatus(`❌ Erro ao abrir o jogo: ${e}`, "erro");
    jogoRodando.value = false;
  }
}

// ─── Fechar jogo ──────────────────────────────────────────────────────────────
async function fecharJogo() {
  try {
    await invoke("kill_game");
    jogoRodando.value = false;
    setStatus("🛑 Jogo encerrado.", "info");
  } catch (e) {
    setStatus(`❌ Erro ao fechar: ${e}`, "erro");
  }
}

// ─── Logout ───────────────────────────────────────────────────────────────────
function fazerLogout() {
  logado.value = false;
  tokenAtual.value = "";
  hwidAtual.value = "";
  loginAtual.value = "";
  login.value = "";
  senha.value = "";
  setStatus("Aguardando login...", "info");
}
</script>

<template>
  <main class="launcher">
    <!-- ── Logo ─────────────────────────────────────────────────────────── -->
    <div class="logo-area">
      <h1 class="titulo">
        L2
        <span class="destaque">Eternal</span>
      </h1>
      <p class="subtitulo">Servidor Privado · Interlude</p>
    </div>

    <!-- ── Formulário de Login ───────────────────────────────────────────── -->
    <transition name="fade">
      <div v-if="!logado" class="card">
        <input v-model="login" type="text" placeholder="Usuário" maxlength="16" autocomplete="off" :disabled="loginando" @keyup.enter="fazerLogin" class="input-field" />
        <input v-model="senha" type="password" placeholder="Senha" maxlength="16" :disabled="loginando" @keyup.enter="fazerLogin" class="input-field" />
        <button @click="fazerLogin" :disabled="loginando" class="btn btn-primary">
          <span v-if="loginando" class="spinner"></span>
          {{ loginando ? "Aguarde..." : "ENTRAR" }}
        </button>
      </div>
    </transition>

    <!-- ── Painel pós-login ──────────────────────────────────────────────── -->
    <transition name="fade">
      <div v-if="logado" class="card">
        <!-- Barra de progresso -->
        <div v-if="mostrarBarra" class="progress-wrap">
          <div class="progress-bar" :style="{ width: progresso + '%' }"></div>
        </div>

        <!-- Botão atualizar (disponível enquanto não está jogando) -->
        <button v-if="!jogoRodando" @click="verificarAtualizacoes" :disabled="atualizando" class="btn btn-secondary">
          {{ atualizando ? "⏳ Atualizando..." : "🔄 Verificar Atualizações" }}
        </button>

        <!-- Botão JOGAR (desativado enquanto atualiza) -->
        <button v-if="!jogoRodando" @click="iniciarJogo" :disabled="atualizando" class="btn btn-play">▶ INICIAR JOGO</button>

        <!-- Botão FECHAR JOGO -->
        <button v-if="jogoRodando" @click="fecharJogo" class="btn btn-danger">■ FECHAR JOGO</button>

        <!-- Logout -->
        <button v-if="!jogoRodando" @click="fazerLogout" class="btn btn-ghost">Trocar conta</button>
      </div>
    </transition>

    <!-- ── Barra de status ───────────────────────────────────────────────── -->
    <p
      class="status-bar"
      :class="{
        'status-sucesso': statusTipo === 'sucesso',
        'status-erro': statusTipo === 'erro',
        'status-info': statusTipo === 'info',
      }"
    >
      {{ statusMsg }}
    </p>
  </main>
</template>

<style scoped>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

.launcher {
  width: 100vw;
  height: 100vh;
  background: linear-gradient(160deg, #0b0b14 0%, #130d28 60%, #0b0b14 100%);
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 28px;
  font-family: "Segoe UI", sans-serif;
  color: #e0e0e0;
  user-select: none;
  overflow: hidden;
}

/* ── Logo ── */
.logo-area {
  text-align: center;
}

.titulo {
  font-size: 3rem;
  font-weight: 900;
  letter-spacing: 6px;
  color: #fff;
  text-shadow: 0 0 30px rgba(139, 92, 246, 0.7);
}

.destaque {
  color: #a78bfa;
}

.subtitulo {
  font-size: 0.75rem;
  color: #6b7280;
  letter-spacing: 3px;
  text-transform: uppercase;
  margin-top: 6px;
}

/* ── Card ── */
.card {
  display: flex;
  flex-direction: column;
  gap: 14px;
  width: 320px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(139, 92, 246, 0.15);
  border-radius: 16px;
  padding: 28px 24px;
  backdrop-filter: blur(8px);
}

/* ── Inputs ── */
.input-field {
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(139, 92, 246, 0.25);
  border-radius: 8px;
  padding: 13px 16px;
  color: #e5e7eb;
  font-size: 0.95rem;
  outline: none;
  transition:
    border-color 0.2s,
    box-shadow 0.2s;
}

.input-field:focus {
  border-color: #a78bfa;
  box-shadow: 0 0 0 3px rgba(167, 139, 250, 0.15);
}

.input-field::placeholder {
  color: #4b5563;
}
.input-field:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* ── Botões ── */
.btn {
  border: none;
  border-radius: 8px;
  padding: 13px;
  font-size: 0.95rem;
  font-weight: 700;
  letter-spacing: 1px;
  cursor: pointer;
  transition:
    transform 0.15s,
    box-shadow 0.15s,
    opacity 0.15s;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}

.btn:hover:not(:disabled) {
  transform: translateY(-2px);
}
.btn:active:not(:disabled) {
  transform: translateY(0);
}
.btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.btn-primary {
  background: linear-gradient(135deg, #6d28d9, #a78bfa);
  color: #fff;
}
.btn-primary:hover:not(:disabled) {
  box-shadow: 0 6px 20px rgba(109, 40, 217, 0.45);
}

.btn-secondary {
  background: rgba(139, 92, 246, 0.1);
  border: 1px solid rgba(139, 92, 246, 0.3);
  color: #a78bfa;
  font-size: 0.85rem;
}
.btn-secondary:hover:not(:disabled) {
  background: rgba(139, 92, 246, 0.2);
}

.btn-play {
  background: linear-gradient(135deg, #065f46, #10b981);
  color: #fff;
  font-size: 1rem;
  letter-spacing: 3px;
  padding: 16px;
}
.btn-play:hover:not(:disabled) {
  box-shadow: 0 6px 20px rgba(16, 185, 129, 0.4);
}

.btn-danger {
  background: linear-gradient(135deg, #991b1b, #ef4444);
  color: #fff;
}
.btn-danger:hover {
  box-shadow: 0 6px 20px rgba(239, 68, 68, 0.4);
}

.btn-ghost {
  background: transparent;
  color: #6b7280;
  font-size: 0.78rem;
  font-weight: 400;
  letter-spacing: 0;
  padding: 6px;
}
.btn-ghost:hover:not(:disabled) {
  color: #9ca3af;
  transform: none;
}

/* ── Barra de progresso ── */
.progress-wrap {
  width: 100%;
  height: 4px;
  background: rgba(255, 255, 255, 0.08);
  border-radius: 99px;
  overflow: hidden;
}

.progress-bar {
  height: 100%;
  background: linear-gradient(90deg, #6d28d9, #a78bfa);
  border-radius: 99px;
  transition: width 0.3s ease;
}

/* ── Status ── */
.status-bar {
  font-size: 0.82rem;
  text-align: center;
  max-width: 360px;
  min-height: 18px;
  transition: color 0.3s;
}

.status-info {
  color: #6b7280;
}
.status-sucesso {
  color: #34d399;
}
.status-erro {
  color: #f87171;
}

/* ── Spinner ── */
.spinner {
  width: 14px;
  height: 14px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: #fff;
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

/* ── Transição fade ── */
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
