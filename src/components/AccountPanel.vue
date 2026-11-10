<script setup lang="ts">
// 设置 → 账户（DanLume 服务器）：服务器地址 / 测试连接 / 登录 / 注册 / 设备管理 / 退出登录。
//
// 老的「邮件申请 + 管理员签发」那条路（AdminPanel）一个字都没删：
// 服务器不可用时这里给一个「改为邮件申请」按钮，直接跳到它。
import { computed, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import SoftButton from "./SoftButton.vue";
import Pill from "./Pill.vue";
import {
  accountDevices,
  accountHealth,
  accountKickDevice,
  accountLogin,
  accountLogout,
  accountNotifyStatus,
  accountRegister,
  accountSaveServer,
  accountState,
  asAccountError,
  requestEmailCode,
  type AccountState,
  type NotifyStatus,
  type ServerDevice,
} from "../api";

const emit = defineEmits<{ (e: "mail-apply"): void }>();

const state = ref<AccountState | null>(null);
const notify = ref<NotifyStatus | null>(null);
const urlDraft = ref("");
const testing = ref(false);
const busy = ref(false);
const message = ref("");
const health = ref<{ ok: boolean; version: string; latency_ms: number; at: string } | null>(
  null,
);
const offline = ref(false);

// 登录 / 注册弹层
const dialog = ref<"" | "login" | "register">("");
const form = reactive({
  who: "",
  password: "",
  username: "",
  email: "",
  invite: "",
  code: "",
});
const dialogError = ref("");
const limitDevices = ref<ServerDevice[]>([]);

// 「获取验证码」：发送中转圈，成功后按钮按服务器的 cooldown 倒计时
const codeSending = ref(false);
const codeLeft = ref(0);
const codeNote = ref("");
let codeTimer: ReturnType<typeof setInterval> | null = null;

// 设备管理
const devices = ref<ServerDevice[]>([]);
const devicesUsed = ref(0);
const devicesLimit = ref(0);
const devicesOpen = ref(false);
const deviceNote = ref("");

const loggedIn = computed(() => !!state.value?.logged_in);
const unread = computed(() => state.value?.announcements_unread ?? 0);
/** 设备数：优先用刚拉到的 /devices（它带 devices_used/device_limit），其次用登录响应里的 */
const deviceCount = computed(() => {
  const used =
    devicesUsed.value || devices.value.length || state.value?.devices_used || 0;
  const limit = devicesLimit.value || state.value?.device_limit || 0;
  if (!used && !limit) return "—";
  return limit > 0 ? `${used} / ${limit} 台` : `${used} 台`;
});
const notifyText = computed(() => {
  const one = notify.value;
  if (!one) return "";
  if (one.unsupported) return "公告通道：服务器还没有 /announcements 接口（会自动重试）";
  if (one.last_error) return `公告通道：${one.last_error}`;
  if (one.last_ok_at) return `公告通道：每 ${one.interval_secs} 秒轮询 · 上次成功 ${one.last_ok_at}`;
  return `公告通道：每 ${one.interval_secs} 秒轮询（登录后开始）`;
});

async function refresh() {
  try {
    state.value = await accountState();
    urlDraft.value = state.value.server_url;
  } catch (err) {
    message.value = String(err);
  }
  try {
    notify.value = await accountNotifyStatus();
  } catch {
    notify.value = null;
  }
}

/** 测试连接：在线/离线、版本、延迟 */
async function testConnection() {
  if (testing.value) return;
  testing.value = true;
  message.value = "正在连服务器…";
  try {
    const result = await accountHealth();
    health.value = {
      ok: result.ok && result.online,
      version: result.version,
      latency_ms: result.latency_ms,
      at: new Date().toLocaleTimeString(),
    };
    offline.value = false;
    message.value = `服务器在线 · 版本 ${result.version || "?"} · ${result.latency_ms} ms`;
  } catch (err) {
    const problem = asAccountError(err);
    health.value = null;
    offline.value = problem.offline;
    message.value = problem.offline
      ? "服务器暂不可用：连不上"
      : `测试失败：${problem.error}`;
  } finally {
    testing.value = false;
    await refresh();
  }
}

async function saveUrl() {
  busy.value = true;
  try {
    const saved = await accountSaveServer(urlDraft.value);
    message.value = saved.server_url
      ? `服务器地址已保存：${saved.server_url}`
      : "已设为不用服务器（留空）";
    await refresh();
  } catch (err) {
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

function openDialog(kind: "login" | "register") {
  dialogError.value = "";
  limitDevices.value = [];
  if (kind === "register") codeNote.value = "";
  dialog.value = kind;
}

function closeDialog() {
  dialog.value = "";
  dialogError.value = "";
  limitDevices.value = [];
}

/* ------------------------------------------------------------ 邮箱验证码 */

function stopCodeCountdown() {
  if (codeTimer !== null) {
    clearInterval(codeTimer);
    codeTimer = null;
  }
}

/** 按钮变成「重新获取(59s)」：按服务器给的 cooldown 走，没有就 60 秒兜底 */
function startCodeCountdown(seconds: number) {
  stopCodeCountdown();
  codeLeft.value = Math.max(0, Math.floor(seconds));
  if (codeLeft.value <= 0) return;
  codeTimer = setInterval(() => {
    codeLeft.value -= 1;
    if (codeLeft.value <= 0) {
      codeLeft.value = 0;
      stopCodeCountdown();
    }
  }, 1000);
}

onBeforeUnmount(stopCodeCountdown);

/** 点「获取验证码」：成功就倒计时；429 按 retry_after 倒计时；503 提示服务器发不出信 */
async function sendCode() {
  if (codeSending.value || codeLeft.value > 0) return;
  const email = form.email.trim();
  dialogError.value = "";
  if (!email) {
    dialogError.value = "请先填邮箱";
    return;
  }
  codeSending.value = true;
  codeNote.value = "";
  try {
    const result = await requestEmailCode(email);
    const cooldown = result.cooldown > 0 ? result.cooldown : 60;
    startCodeCountdown(cooldown);
    codeNote.value = `验证码已发到 ${email}，${Math.round((result.expires_in || 600) / 60)} 分钟内有效`;
  } catch (err) {
    const problem = asAccountError(err);
    if (problem.kind === "cooldown") {
      const wait = problem.retry_after > 0 ? problem.retry_after : 60;
      startCodeCountdown(wait);
      dialogError.value = `请 ${wait} 秒后再试`;
    } else if (problem.offline) {
      dialogError.value = "服务器暂不可用，可以改用邮件申请";
    } else if (problem.kind === "email") {
      dialogError.value =
        "服务器暂时发不出验证码邮件（SMTP 没配好或发信失败），可以先走下面的邮件申请";
    } else {
      dialogError.value = problem.error;
    }
  } finally {
    codeSending.value = false;
  }
}

/** 6 位数字验证码（本地先挡一道，别为格式问题白跑一趟服务器） */
function isValidCode(code: string) {
  return /^\d{6}$/.test(code.trim());
}

/** 409 设备超限：把服务器给的设备列表摆出来，让人踢一台再重试 */
function handleLimit(err: ReturnType<typeof asAccountError>) {
  if (err.kind === "device_limit") {
    limitDevices.value = err.devices || [];
    dialogError.value =
      err.error ||
      `设备数已达上限（${err.devices_used}/${err.device_limit}），请先踢掉一台`;
    return true;
  }
  return false;
}

async function doLogin() {
  if (busy.value) return;
  busy.value = true;
  dialogError.value = "";
  try {
    await accountLogin(form.who.trim(), form.password);
    message.value = `已登录：${form.who.trim()}`;
    form.password = "";
    closeDialog();
    await refresh();
  } catch (err) {
    const problem = asAccountError(err);
    if (!handleLimit(problem)) {
      dialogError.value = problem.offline
        ? "服务器暂不可用，可以改用邮件申请"
        : problem.error;
    }
  } finally {
    busy.value = false;
  }
}

async function doRegister() {
  if (busy.value) return;
  const email = form.email.trim();
  const code = form.code.trim();
  // 提交前本地校验：邮箱非空 + 6 位验证码
  if (!email) {
    dialogError.value = "请先填邮箱";
    return;
  }
  if (!isValidCode(code)) {
    dialogError.value = "邮箱验证码是 6 位数字";
    return;
  }
  busy.value = true;
  dialogError.value = "";
  try {
    await accountRegister(
      form.username.trim(),
      email,
      form.password,
      form.invite.trim(),
      code,
    );
    message.value = `注册成功：${form.username.trim()}`;
    form.password = "";
    form.code = "";
    codeNote.value = "";
    stopCodeCountdown();
    codeLeft.value = 0;
    closeDialog();
    await refresh();
  } catch (err) {
    const problem = asAccountError(err);
    if (problem.kind === "device_limit") {
      handleLimit(problem);
    } else if (problem.kind === "invalid_code") {
      // 服务器说验证码不对 / 过期，顺便把还能试几次写进文案
      const left = problem.attempts_left;
      if (problem.code === "code_expired") {
        dialogError.value = problem.error; // 服务器原话已经说了「请重新获取」
      } else if (left > 0) {
        dialogError.value = `${problem.error}，还可以再试 ${left} 次`;
      } else {
        dialogError.value = `${problem.error}，请点「获取验证码」重新发一封`;
      }
    } else if (problem.kind === "cooldown") {
      const wait = problem.retry_after > 0 ? problem.retry_after : 60;
      dialogError.value = `请 ${wait} 秒后再试`;
    } else if (problem.http_status === 403) {
      dialogError.value = `${problem.error}（这台服务器当前没开放注册，可以走邮件申请）`;
    } else if (problem.offline) {
      dialogError.value = "服务器暂不可用，可以改用邮件申请";
    } else {
      dialogError.value = problem.error;
    }
  } finally {
    busy.value = false;
  }
}

async function loadDevices() {
  busy.value = true;
  try {
    const result = await accountDevices();
    devices.value = result.devices || [];
    if (typeof result.devices_used === "number") devicesUsed.value = result.devices_used;
    if (typeof result.device_limit === "number") devicesLimit.value = result.device_limit;
    deviceNote.value = devices.value.length
      ? `共 ${devices.value.length} 台设备`
      : "服务器没有返回设备";
  } catch (err) {
    deviceNote.value = asAccountError(err).error;
  } finally {
    busy.value = false;
  }
}

async function toggleDevices() {
  devicesOpen.value = !devicesOpen.value;
  if (devicesOpen.value) await loadDevices();
}

async function kick(id: number) {
  busy.value = true;
  try {
    await accountKickDevice(id);
    deviceNote.value = "已踢出这台设备";
    await loadDevices();
    await refresh();
  } catch (err) {
    deviceNote.value = asAccountError(err).error;
  } finally {
    busy.value = false;
  }
}

/** 在 409 弹层里踢设备（用来腾出名额） */
async function kickFromLimit(id: number) {
  try {
    await accountKickDevice(id);
    limitDevices.value = limitDevices.value.filter((one) => one.id !== id);
    dialogError.value = "已踢出，可以再试一次登录";
  } catch (err) {
    dialogError.value = asAccountError(err).error;
  }
}

async function doLogout() {
  if (!confirm("退出登录后本地会清掉服务器 token，确定？")) return;
  busy.value = true;
  try {
    await accountLogout();
    message.value = "已退出登录";
    devicesOpen.value = false;
    devices.value = [];
    await refresh();
  } catch (err) {
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  await refresh();
  // 进设置页就自动探一次，省得用户不知道服务器在不在线
  if (state.value?.enabled) await testConnection();
});
</script>

<template>
  <div class="account">
    <div class="head">
      <span class="sec">DanLume 账号</span>
      <span class="spacer" />
      <Pill
        :text="state?.enabled ? (loggedIn ? '已登录' : '未登录') : '未启用'"
        :color="loggedIn ? 'var(--success)' : 'var(--muted)'"
        :font-size="11"
      />
      <Pill
        v-if="unread > 0"
        :text="`${unread} 条公告未读`"
        color="var(--accent)"
        :font-size="11"
      />
    </div>

    <!-- 服务器地址 + 测试连接 -->
    <div class="field">
      <div class="label">服务器地址</div>
      <div class="row">
        <input
          v-model="urlDraft"
          class="url"
          type="text"
          placeholder="https://43.108.99.38.sslip.io（留空 = 不用服务器）"
          spellcheck="false"
        />
        <SoftButton :height="30" :font-size="12.5" :disabled="busy" @click="saveUrl">
          保存地址
        </SoftButton>
        <SoftButton
          variant="primary"
          :height="30"
          :font-size="12.5"
          glyph="REFRESH"
          :disabled="testing"
          @click="testConnection"
        >
          {{ testing ? "连接中…" : "测试连接" }}
        </SoftButton>
      </div>
      <div class="hint">
        <template v-if="health">
          在线 · 版本 {{ health.version || "?" }} · {{ health.latency_ms }} ms ·
          {{ health.at }}
        </template>
        <template v-else-if="message">{{ message }}</template>
        <template v-else>点「测试连接」看看服务器在不在线</template>
      </div>
      <div v-if="notifyText" class="hint">{{ notifyText }}</div>
    </div>

    <!-- 未登录：登录 / 注册 -->
    <div v-if="!loggedIn" class="field">
      <div class="label">登录状态</div>
      <div class="row">
        <SoftButton
          variant="primary"
          :height="32"
          :font-size="13"
          glyph="LOCK"
          :disabled="busy"
          @click="openDialog('login')"
        >
          登录
        </SoftButton>
        <SoftButton
          :height="32"
          :font-size="13"
          glyph="ADD"
          :disabled="busy"
          @click="openDialog('register')"
        >
          注册
        </SoftButton>
      </div>
      <div class="hint">
        账号注册 / 登录走自家服务器；登录后会自动同步服务器公告，并在设备管理里能看到本机。
      </div>
    </div>

    <!-- 已登录：账号信息 + 设备 + 退出 -->
    <template v-else>
      <div class="field">
        <div class="label">当前账户</div>
        <div class="info-grid">
          <span class="k">用户名</span>
          <span class="v">{{ state?.username || "（服务器没返回）" }}</span>
          <span class="k">账号 ID</span>
          <span class="v">{{ state?.account_id || "—" }}</span>
          <span class="k">设备</span>
          <span class="v">{{ deviceCount }}</span>
          <span class="k">本机指纹</span>
          <span class="v mono">{{ (state?.device_fingerprint || "").slice(0, 18) }}…</span>
        </div>
      </div>

      <div class="field">
        <div class="label">设备管理</div>
        <div class="row">
          <SoftButton :height="30" :font-size="12.5" :disabled="busy" @click="toggleDevices">
            {{ devicesOpen ? "收起设备列表" : "设备管理" }}
          </SoftButton>
          <SoftButton
            variant="danger"
            :height="30"
            :font-size="12.5"
            :disabled="busy"
            @click="doLogout"
          >
            退出登录
          </SoftButton>
        </div>
        <div v-if="devicesOpen" class="devices">
          <div v-if="!devices.length" class="hint">{{ deviceNote || "没有设备" }}</div>
          <div v-for="one in devices" :key="one.id" class="device">
            <span class="dname">{{ one.name || `设备 ${one.id}` }}</span>
            <span class="dmeta">{{ one.platform || "?" }} · {{ one.last_seen_at || "" }}</span>
            <button class="mini" @click="kick(one.id)">踢出</button>
          </div>
          <div v-if="deviceNote && devices.length" class="hint">{{ deviceNote }}</div>
        </div>
      </div>
    </template>

    <!-- 服务器不可用：一键退回老的邮件申请 -->
    <div class="fallback">
      <span class="hint">
        {{ offline ? "服务器暂不可用。" : "连不上服务器或注册没开放？" }}
        老路子还在：用管理员公钥加密后发邮件，管理员批准回验证码 + 回执。
      </span>
      <SoftButton :height="28" :font-size="12" glyph="MAIL" @click="emit('mail-apply')">
        改为邮件申请
      </SoftButton>
    </div>

    <!-- 登录弹层 -->
    <div v-if="dialog === 'login'" class="mask" @click.self="closeDialog">
      <div class="panel">
        <div class="panel-head">
          <div class="h2">登录 DanLume 账号</div>
          <span class="spacer" />
          <button class="link" @click="closeDialog">关闭</button>
        </div>
        <label class="fl">
          <span>用户名或邮箱</span>
          <input v-model="form.who" type="text" autofocus @keyup.enter="doLogin" />
        </label>
        <label class="fl">
          <span>口令</span>
          <input
            v-model="form.password"
            type="password"
            placeholder="••••••••"
            @keyup.enter="doLogin"
          />
        </label>
        <div v-if="limitDevices.length" class="limit">
          <div class="hint">这台服务器说设备超限了，先踢一台再登录：</div>
          <div v-for="one in limitDevices" :key="one.id" class="device">
            <span class="dname">{{ one.name || `设备 ${one.id}` }}</span>
            <span class="dmeta">{{ one.last_seen_at || "" }}</span>
            <button class="mini" @click="kickFromLimit(one.id)">踢出</button>
          </div>
        </div>
        <div v-if="dialogError" class="err">{{ dialogError }}</div>
        <div class="panel-foot">
          <SoftButton :height="30" :font-size="12.5" @click="closeDialog">取消</SoftButton>
          <SoftButton
            variant="primary"
            :height="30"
            :font-size="12.5"
            :disabled="busy"
            @click="doLogin"
          >
            {{ busy ? "登录中…" : "登录" }}
          </SoftButton>
        </div>
      </div>
    </div>

    <!-- 注册弹层 -->
    <div v-if="dialog === 'register'" class="mask" @click.self="closeDialog">
      <div class="panel">
        <div class="panel-head">
          <div class="h2">注册 DanLume 账号</div>
          <span class="spacer" />
          <button class="link" @click="closeDialog">关闭</button>
        </div>
        <label class="fl">
          <span>用户名</span>
          <input v-model="form.username" type="text" placeholder="登录用的名字" />
        </label>
        <div class="fl">
          <span>邮箱</span>
          <div class="code-row">
            <input v-model="form.email" type="text" placeholder="name@example.com" />
            <SoftButton
              :height="30"
              :font-size="12.5"
              :disabled="codeSending || codeLeft > 0 || !form.email.trim()"
              @click="sendCode"
            >
              {{
                codeLeft > 0
                  ? `重新获取(${codeLeft}s)`
                  : codeSending
                    ? "发送中…"
                    : "获取验证码"
              }}
            </SoftButton>
          </div>
        </div>
        <label class="fl">
          <span>邮箱验证码</span>
          <input
            v-model="form.code"
            type="text"
            inputmode="numeric"
            maxlength="6"
            placeholder="6 位数字"
            @keyup.enter="doRegister"
          />
        </label>
        <div v-if="codeNote" class="hint">{{ codeNote }}</div>
        <label class="fl">
          <span>口令</span>
          <input v-model="form.password" type="password" placeholder="至少 6 位" />
        </label>
        <label class="fl">
          <span>邀请码（可空）</span>
          <input v-model="form.invite" type="text" placeholder="服务器关了注册时要填" />
        </label>
        <div v-if="dialogError" class="err">{{ dialogError }}</div>
        <div class="panel-foot">
          <SoftButton :height="30" :font-size="12.5" @click="closeDialog">取消</SoftButton>
          <SoftButton
            variant="primary"
            :height="30"
            :font-size="12.5"
            :disabled="busy"
            @click="doRegister"
          >
            {{ busy ? "注册中…" : "注册并登录" }}
          </SoftButton>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.account {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.sec {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text);
}
.spacer {
  flex: 1;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.label {
  font-size: 12.5px;
  color: var(--muted);
}
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.url {
  flex: 1;
  min-width: 240px;
}
.hint {
  font-size: 11.5px;
  color: var(--muted);
  line-height: 1.6;
}
.info-grid {
  display: grid;
  grid-template-columns: 78px 1fr;
  gap: 3px 10px;
  font-size: 12.5px;
}
.k {
  color: var(--muted);
}
.v {
  color: var(--text);
  overflow-wrap: anywhere;
}
.mono {
  font-family: var(--font-mono);
  font-size: 11.5px;
}
.devices {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 6px 0 0;
}
.device {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 8px;
  border-radius: 8px;
  background: var(--card-alt);
  font-size: 12px;
}
.dname {
  color: var(--text);
  font-weight: 550;
}
.dmeta {
  flex: 1;
  color: var(--muted);
  font-size: 11px;
  overflow-wrap: anywhere;
}
.mini {
  font-size: 11px;
  padding: 1px 7px;
  border-radius: 6px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  color: var(--danger);
  cursor: pointer;
  font-family: inherit;
}
.fallback {
  display: flex;
  align-items: center;
  gap: 10px;
  padding-top: 6px;
  border-top: 1px dashed var(--border-soft);
}
.fallback .hint {
  flex: 1;
}
.mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.32);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 70;
}
.panel {
  width: min(420px, 92vw);
  max-height: 86vh;
  overflow: auto;
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: 14px;
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.panel-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.h2 {
  font-size: 14.5px;
  font-weight: 600;
  color: var(--text);
}
.link {
  border: none;
  background: transparent;
  color: var(--accent);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.fl {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--muted);
}
.fl input {
  height: 30px;
}
/* 邮箱 + 「获取验证码」并排 */
.code-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.code-row input {
  flex: 1;
  min-width: 0;
}
.limit {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.err {
  font-size: 12px;
  color: var(--danger);
  line-height: 1.6;
}
.panel-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
