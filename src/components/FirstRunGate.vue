<script setup lang="ts">
// 首次使用必须先有一个 DanLume 服务器账号（用户 2026-11-11 的要求：所有人一律强制）。
//
// 为什么强制：公告/更新推送走服务器；没有账号就收不到补丁通知，管理员也没法按账号管人。
// 这里是一条**门**：没账号就进不了主界面；只有服务器连不上时才给「先离线使用」的退路
// （否则服务器一宕机，所有人的软件就都开不了了）。
import { computed, onMounted, ref } from "vue";
import SoftButton from "./SoftButton.vue";
import {
  accountHealth,
  accountLogin,
  accountRegister,
  accountSaveServer,
  accountState,
  asAccountError,
  requestEmailCode,
  requestResetCode,
  resetPassword,
} from "../api";

const emit = defineEmits<{ (e: "done"): void }>();

const mode = ref<"register" | "login">("register");
/** 忘记密码：在登录页里就地展开，不再弹第二个窗口 */
const resetOpen = ref(false);
const resetBusy = ref(false);
const resetNote = ref("");
const busy = ref(false);
const error = ref("");
const note = ref("");
const serverUrl = ref("");

const form = ref({
  username: "",
  email: "",
  code: "",
  password: "",
  confirm: "",
  who: "",
  invite: "",
  loginPassword: "",
});

// 验证码倒计时
const codeSending = ref(false);
const codeLeft = ref(0);
let timer: ReturnType<typeof setInterval> | null = null;

const canRegister = computed(
  () =>
    form.value.username.trim().length >= 2 &&
    form.value.email.includes("@") &&
    /^\d{6}$/.test(form.value.code.trim()) &&
    form.value.password.length >= 8,
);

const canLogin = computed(
  () => form.value.who.trim().length > 0 && form.value.loginPassword.length > 0,
);

function startCountdown(seconds: number) {
  if (timer) window.clearInterval(timer);
  codeLeft.value = Math.max(1, seconds);
  timer = window.setInterval(() => {
    codeLeft.value -= 1;
    if (codeLeft.value <= 0 && timer) {
      window.clearInterval(timer);
      timer = null;
    }
  }, 1000);
}

async function sendCode() {
  if (codeSending.value || codeLeft.value > 0) return;
  const email = form.value.email.trim();
  if (!email.includes("@")) {
    error.value = "先填一个复旦邮箱（@fudan.edu.cn 或 @m.fudan.edu.cn）";
    return;
  }
  codeSending.value = true;
  error.value = "";
  note.value = "";
  try {
    const result = await requestEmailCode(email);
    startCountdown(result.cooldown > 0 ? result.cooldown : 60);
    note.value = `验证码已发到 ${email}，${Math.round((result.expires_in || 600) / 60)} 分钟内有效`;
  } catch (err) {
    const problem = asAccountError(err);
    if (problem.kind === "cooldown") {
      startCountdown(problem.retry_after > 0 ? problem.retry_after : 60);
      error.value = `刚发过，请 ${problem.retry_after || 60} 秒后再点`;
    } else if (problem.offline) {
      error.value = "服务器连不上，检查一下网络（或公司/学校 VPN）";
    } else {
      error.value = problem.error;
    }
  } finally {
    codeSending.value = false;
  }
}

async function doRegister() {
  if (busy.value) return;
  if (form.value.password !== form.value.confirm) {
    error.value = "两次填的口令不一样";
    return;
  }
  if (!canRegister.value) {
    error.value = "用户名至少 2 个字、邮箱要填对、验证码 6 位、口令至少 8 位";
    return;
  }
  busy.value = true;
  error.value = "";
  try {
    await accountRegister(
      form.value.username.trim(),
      form.value.email.trim(),
      form.value.password,
      form.value.invite.trim(),
      form.value.code.trim(),
    );
    emit("done");
  } catch (err) {
    failedAttempts.value += 1;
    const problem = asAccountError(err);
    if (problem.kind === "invite") {
      error.value = `${problem.error}（把管理员给的邀请码填到下面）`;
    } else if (problem.kind === "invalid_code") {
      error.value = problem.error;
    } else if (problem.offline) {
      error.value = "服务器连不上，检查一下网络";
    } else {
      error.value = problem.error;
    }
  } finally {
    busy.value = false;
  }
}

async function doLogin() {
  if (busy.value || !canLogin.value) return;
  busy.value = true;
  error.value = "";
  try {
    await accountLogin(form.value.who.trim(), form.value.loginPassword);
    emit("done");
  } catch (err) {
    const problem = asAccountError(err);
    error.value = problem.offline ? "服务器连不上，检查一下网络" : problem.error;
  } finally {
    busy.value = false;
  }
}

/** 服务器连不上时才允许先离线用（不然服务器一挂，所有人的软件都开不了） */
const serverDown = ref(false);
/** 注册连续失败几次（邮箱域名被拒之类）之后也放开离线口子，免得彻底卡死 */
const failedAttempts = ref(0);
const allowOffline = computed(() => serverDown.value || failedAttempts.value >= 3);

// ---- 忘记密码（邮箱验证码重置）
const resetForm = ref({ email: "", code: "", password: "", confirm: "" });
const resetSending = ref(false);
const resetLeft = ref(0);
let resetTimer: ReturnType<typeof setInterval> | null = null;

function startResetCountdown(seconds: number) {
  if (resetTimer) window.clearInterval(resetTimer);
  resetLeft.value = Math.max(1, seconds);
  resetTimer = window.setInterval(() => {
    resetLeft.value -= 1;
    if (resetLeft.value <= 0 && resetTimer) {
      window.clearInterval(resetTimer);
      resetTimer = null;
    }
  }, 1000);
}

function openReset() {
  resetOpen.value = true;
  resetNote.value = "";
  error.value = "";
  // 登录框里填过的账号如果是邮箱，直接带过去
  const who = form.value.who.trim();
  if (who.includes("@")) resetForm.value.email = who;
}

async function sendResetCode() {
  if (resetSending.value || resetLeft.value > 0) return;
  const email = resetForm.value.email.trim();
  if (!email.includes("@")) {
    error.value = "先填注册时用的邮箱";
    return;
  }
  resetSending.value = true;
  error.value = "";
  resetNote.value = "";
  try {
    const result = await requestResetCode(email);
    startResetCountdown(result.cooldown > 0 ? result.cooldown : 60);
    resetNote.value = `如果 ${email} 注册过，重置验证码已经发出（10 分钟内有效）`;
  } catch (err) {
    const problem = asAccountError(err);
    if (problem.kind === "cooldown") {
      startResetCountdown(problem.retry_after > 0 ? problem.retry_after : 60);
      error.value = `刚发过，请 ${problem.retry_after || 60} 秒后再点`;
    } else {
      error.value = problem.offline ? "服务器连不上，检查一下网络" : problem.error;
    }
  } finally {
    resetSending.value = false;
  }
}

async function doReset() {
  if (resetBusy.value) return;
  const { email, code, password, confirm } = resetForm.value;
  if (!email.includes("@") || !/^\d{6}$/.test(code.trim())) {
    error.value = "邮箱和 6 位验证码都要填对";
    return;
  }
  if (password.length < 8) {
    error.value = "新口令至少 8 位";
    return;
  }
  if (password !== confirm) {
    error.value = "两次填的新口令不一样";
    return;
  }
  resetBusy.value = true;
  error.value = "";
  try {
    const result = await resetPassword(email.trim(), code.trim(), password);
    resetOpen.value = false;
    mode.value = "login";
    form.value.who = email.trim();
    note.value = result.message || "密码已重置，用新口令登录即可";
    resetForm.value = { email: "", code: "", password: "", confirm: "" };
  } catch (err) {
    const problem = asAccountError(err);
    error.value = problem.offline ? "服务器连不上，检查一下网络" : problem.error;
  } finally {
    resetBusy.value = false;
  }
}

async function checkServer() {
  try {
    const health = await accountHealth();
    serverDown.value = !health.ok;
    note.value = health.ok
      ? `服务器正常（v${health.version}，${health.latency_ms} ms）`
      : "服务器返回异常";
    return health.ok;
  } catch {
    serverDown.value = true;
    note.value = "";
    error.value = "连不上服务器，检查网络后再试一次";
    return false;
  }
}

function useOffline() {
  emit("done");
}

async function saveServer() {
  const url = serverUrl.value.trim();
  if (!url.startsWith("http")) {
    error.value = "服务器地址要以 http(s):// 开头";
    return;
  }
  await accountSaveServer(url);
  await checkServer();
}

onMounted(async () => {
  try {
    const state = await accountState();
    serverUrl.value = state.server_url || "";
    if (state.logged_in) emit("done"); // 已经登录过：直接放行
  } catch {
    /* 读不到就照常显示表单 */
  }
  void checkServer();
});
</script>

<template>
  <div class="gate">
    <div class="gate-card">
      <div class="gate-head">
        <div class="gate-logo">旦</div>
        <div class="gate-titles">
          <h1>欢迎使用 DanLume</h1>
          <p>
            开始之前请先注册一个 DanLume 账号。我们用它做账号管理，也用它把更新和公告第一时间推给你。
            账号只保存用户名、邮箱和口令的哈希，别的一概不需要。
          </p>
        </div>
      </div>

      <div class="gate-tabs">
        <button class="tab" :class="{ on: mode === 'register' }" @click="mode = 'register'">
          注册新账号
        </button>
        <button class="tab" :class="{ on: mode === 'login' }" @click="mode = 'login'">
          已有账号，直接登录
        </button>
      </div>

      <div v-if="mode === 'register'" class="gate-form">
        <label>
          <span>用户名</span>
          <input v-model="form.username" type="text" placeholder="显示用，2–20 个字" spellcheck="false" />
        </label>
        <label>
          <span>复旦邮箱</span>
          <input
            v-model="form.email"
            type="text"
            placeholder="学号@fudan.edu.cn 或 @m.fudan.edu.cn"
            spellcheck="false"
          />
        </label>
        <label>
          <span>邮箱验证码</span>
          <div class="row">
            <input v-model="form.code" type="text" placeholder="6 位数字" maxlength="6" spellcheck="false" />
            <SoftButton
              :height="30"
              :font-size="12"
              :disabled="codeSending || codeLeft > 0"
              @click="sendCode"
            >
              {{ codeLeft > 0 ? `${codeLeft} 秒后重发` : codeSending ? "发送中…" : "获取验证码" }}
            </SoftButton>
          </div>
        </label>
        <label>
          <span>口令</span>
          <input v-model="form.password" type="password" placeholder="至少 8 位" spellcheck="false" />
        </label>
        <label>
          <span>再填一次</span>
          <input v-model="form.confirm" type="password" placeholder="再输一遍口令" spellcheck="false" />
        </label>
        <label v-if="error.includes('邀请码')">
          <span>邀请码</span>
          <input v-model="form.invite" type="text" placeholder="管理员给你的邀请码" spellcheck="false" />
        </label>
      </div>

      <div v-else class="gate-form">
        <label>
          <span>用户名或邮箱</span>
          <input v-model="form.who" type="text" placeholder="注册时用的那个" spellcheck="false" />
        </label>
        <label>
          <span>口令</span>
          <input v-model="form.loginPassword" type="password" spellcheck="false" />
        </label>
        <div class="gate-forgot">
          <button class="link" @click="openReset">忘记密码？</button>
          <span class="tiny">（用注册邮箱收验证码重设）</span>
        </div>
      </div>

      <!-- 忘记密码：就地展开，邮箱验证码 + 新口令 -->
      <div v-if="resetOpen" class="gate-reset">
        <div class="gate-reset-head">
          <b>重设密码</b>
          <span class="spacer" />
          <button class="link" @click="resetOpen = false">收起</button>
        </div>
        <label>
          <span>注册时用的邮箱</span>
          <input v-model="resetForm.email" type="text" placeholder="26303050173@fudan.edu.cn" spellcheck="false" />
        </label>
        <label>
          <span>邮箱验证码</span>
          <div class="row">
            <input v-model="resetForm.code" type="text" maxlength="6" placeholder="6 位数字" spellcheck="false" />
            <SoftButton
              :height="30"
              :font-size="12"
              :disabled="resetSending || resetLeft > 0"
              @click="sendResetCode"
            >
              {{ resetLeft > 0 ? `${resetLeft} 秒后重发` : resetSending ? "发送中…" : "发验证码" }}
            </SoftButton>
          </div>
        </label>
        <label>
          <span>新口令</span>
          <input v-model="resetForm.password" type="password" placeholder="至少 8 位" spellcheck="false" />
        </label>
        <label>
          <span>再填一次</span>
          <input v-model="resetForm.confirm" type="password" spellcheck="false" />
        </label>
        <div v-if="resetNote" class="gate-note">{{ resetNote }}</div>
        <div class="gate-actions">
          <SoftButton variant="primary" :height="32" :font-size="12.5" :disabled="resetBusy" @click="doReset">
            {{ resetBusy ? "正在重置…" : "重设密码" }}
          </SoftButton>
        </div>
      </div>

      <div v-if="error" class="gate-error">{{ error }}</div>
      <div v-if="note" class="gate-note">{{ note }}</div>

      <div class="gate-foot">
        <div class="gate-server">
          <span class="tiny">服务器</span>
          <input v-model="serverUrl" type="text" spellcheck="false" @change="saveServer" />
          <SoftButton :height="26" :font-size="11.5" @click="checkServer">测试</SoftButton>
        </div>
        <div class="gate-actions">
          <SoftButton v-if="allowOffline" :height="30" :font-size="12" @click="useOffline">
            先离线使用
          </SoftButton>
          <SoftButton
            v-if="mode === 'register'"
            variant="primary"
            :height="34"
            :font-size="13"
            :disabled="busy || !canRegister"
            @click="doRegister"
          >
            {{ busy ? "正在注册…" : "注册并开始使用" }}
          </SoftButton>
          <SoftButton
            v-else
            variant="primary"
            :height="34"
            :font-size="13"
            :disabled="busy || !canLogin"
            @click="doLogin"
          >
            {{ busy ? "正在登录…" : "登录" }}
          </SoftButton>
        </div>
      </div>
      <div v-if="allowOffline" class="gate-hint">
        服务器暂时连不上、或者注册一直不成功，可以先离线使用；
        但公告和更新推送要等账号注册好才收得到。
      </div>
    </div>
  </div>
</template>

<style scoped>
.gate {
  position: fixed;
  inset: 0;
  z-index: 900;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: color-mix(in srgb, var(--bg) 82%, transparent);
  backdrop-filter: blur(6px);
}
.gate-card {
  width: min(520px, 92vw);
  max-height: 88vh;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 20px 22px;
  border-radius: 16px;
  border: 1px solid var(--border);
  background: var(--card);
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.18);
}
.gate-head {
  display: flex;
  gap: 12px;
  align-items: flex-start;
}
.gate-logo {
  flex: none;
  width: 40px;
  height: 40px;
  border-radius: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  font-weight: 700;
  color: #fff;
  background: var(--accent);
}
.gate-titles h1 {
  margin: 0 0 4px;
  font-size: 18px;
}
.gate-titles p {
  margin: 0;
  font-size: 12px;
  line-height: 1.7;
  color: var(--muted);
}
.gate-tabs {
  display: flex;
  gap: 6px;
}
.gate-tabs .tab {
  flex: 1;
  height: 30px;
  border-radius: 9px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text-soft);
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
}
.gate-tabs .tab.on {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 10%, var(--card));
}
.gate-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.gate-form label {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.gate-form label > span {
  font-size: 11.5px;
  color: var(--muted);
}
.gate-form .row {
  display: flex;
  gap: 6px;
  align-items: center;
}
.gate-form .row input {
  flex: 1;
  min-width: 0;
}
.gate-error {
  font-size: 12px;
  line-height: 1.6;
  color: var(--danger);
}
.gate-note {
  font-size: 11.5px;
  line-height: 1.6;
  color: var(--muted);
}
.gate-foot {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-top: 4px;
  border-top: 1px solid var(--border-soft);
}
.gate-server {
  display: flex;
  align-items: center;
  gap: 6px;
}
.gate-server input {
  flex: 1;
  min-width: 0;
  height: 26px;
  font-size: 11.5px;
}
.gate-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.gate-hint {
  font-size: 11.5px;
  line-height: 1.6;
  color: var(--warning, var(--muted));
}
.gate-forgot {
  display: flex;
  align-items: center;
  gap: 6px;
}
.link {
  border: none;
  background: transparent;
  color: var(--accent);
  font-family: inherit;
  font-size: 11.5px;
  padding: 0;
  cursor: pointer;
}
.gate-reset {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 11px;
  border: 1px dashed var(--border);
  background: var(--card-alt);
}
.gate-reset-head {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
}
.gate-reset label {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.gate-reset label > span {
  font-size: 11.5px;
  color: var(--muted);
}
.gate-reset .row {
  display: flex;
  gap: 6px;
  align-items: center;
}
.gate-reset .row input {
  flex: 1;
  min-width: 0;
}
</style>
