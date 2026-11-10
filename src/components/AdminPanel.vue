<script setup lang="ts">
// 账户审核（管理员）：收申请 → 批准（验证码 + 回执）→ 注册表
// 对应旧版 admin_dialog.py；密钥与信封算法在 Rust 侧（adminsign.rs / registration.rs）
import { computed, ref, watch } from "vue";
import SoftButton from "../components/SoftButton.vue";
import {
  adminGenerateKey,
  adminKeyState,
  pickFiles,
  registrationAction,
  registrationApprove,
  registrationImportEml,
  registrationRegistry,
  registrationRequests,
  type AdminKeyState,
  type RegApproval,
  type RegRequest,
  type RegUser,
} from "../api";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "update:open", value: boolean): void }>();

const key = ref<AdminKeyState | null>(null);
const requests = ref<RegRequest[]>([]);
const errors = ref<string[]>([]);
const registry = ref<RegUser[]>([]);
const busy = ref(false);
const message = ref("");
const days = ref(180);
const chosen = ref<number | null>(null);
const approval = ref<RegApproval | null>(null);

const selected = computed(() =>
  chosen.value === null ? null : requests.value[chosen.value] || null,
);

async function refreshKey() {
  const state = await adminKeyState();
  key.value = state;
  days.value = state.valid_days || 180;
}

async function check() {
  if (busy.value) return;
  busy.value = true;
  message.value = "正在读收件箱…";
  try {
    const result = await registrationRequests(60);
    requests.value = result.requests || [];
    errors.value = result.errors || [];
    chosen.value = requests.value.length ? 0 : null;
    approval.value = null;
    message.value = requests.value.length
      ? `收到 ${requests.value.length} 条申请${errors.value.length ? `，${errors.value.length} 条有问题` : ""}`
      : "没有新的申请";
  } catch (err) {
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

async function loadRegistry() {
  try {
    registry.value = await registrationRegistry();
  } catch (err) {
    message.value = String(err);
  }
}

async function doApprove() {
  const request = selected.value;
  if (!request) return;
  busy.value = true;
  message.value = "正在批准…";
  try {
    const result = await registrationApprove(request, days.value);
    approval.value = result;
    message.value = result.ok === false ? String(result.error) : "已批准，下面是回信内容";
    await loadRegistry();
  } catch (err) {
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

async function importEml() {
  const picked = await pickFiles([{ name: "邮件文件", extensions: ["eml"] }]);
  if (!picked.length) return;
  try {
    const result = await registrationImportEml(picked[0]);
    if (!result.ok) {
      message.value = String(result.error);
      return;
    }
    requests.value = [result.request, ...requests.value];
    chosen.value = 0;
    approval.value = null;
    message.value = `已从文件读入：${result.request.username}`;
  } catch (err) {
    message.value = String(err);
  }
}

async function act(action: string, username: string) {
  busy.value = true;
  try {
    const result = await registrationAction(action, username);
    message.value = result.ok === false ? String(result.error) : `${username}：${action} 完成`;
    await loadRegistry();
  } catch (err) {
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

async function makeKey() {
  if (!confirm("生成新的管理员密钥会让旧公钥加密的申请再也解不开，确定？")) return;
  const result = await adminGenerateKey();
  message.value = result.ok ? `新密钥指纹：${result.fingerprint}` : String(result.error);
  await refreshKey();
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.value = "已复制";
  } catch {
    message.value = "复制失败，可以手动选中复制";
  }
}

watch(
  () => props.open,
  async (value) => {
    if (!value) return;
    await refreshKey();
    await loadRegistry();
  },
);
</script>

<template>
  <div v-if="open" class="mask" @click.self="emit('update:open', false)">
    <div class="panel">
      <header>
        <div class="titles">
          <div class="h2">账户审核（管理员）</div>
          <div class="tiny">
            注册申请走邮件：申请人用你的公钥加密发过来，你批准后回验证码 + 签名回执
          </div>
        </div>
        <span class="spacer" />
        <SoftButton :height="28" :font-size="12" @click="emit('update:open', false)">关闭</SoftButton>
      </header>

      <div class="keyline">
        <span class="tiny">
          指纹 {{ key?.fingerprint || "（还没有公钥）" }} ·
          默认有效期 {{ key?.valid_days }} 天 ·
          设备上限 {{ key?.device_limit }} 台
        </span>
        <span class="spacer" />
        <SoftButton v-if="!key?.has_private_key" :height="26" :font-size="11.5" @click="makeKey">
          生成管理员密钥
        </SoftButton>
        <SoftButton
          v-else
          :height="26"
          :font-size="11.5"
          @click="key?.public_pem && copy(key.public_pem)"
        >
          复制公钥
        </SoftButton>
      </div>

      <div class="row">
        <SoftButton
          variant="primary"
          :height="28"
          :font-size="12"
          :disabled="busy"
          @click="check"
        >
          检查新申请
        </SoftButton>
        <SoftButton :height="28" :font-size="12" @click="importEml">从 .eml 导入</SoftButton>
        <input v-model.number="days" class="days" type="number" min="1" max="3650" />
        <span class="tiny">天有效期</span>
        <span class="spacer" />
        <span class="tiny">{{ message }}</span>
      </div>

      <div class="cols">
        <div class="col">
          <div class="sec">注册申请</div>
          <div v-if="!requests.length" class="empty">点「检查新申请」读收件箱</div>
          <button
            v-for="(request, index) in requests"
            :key="`${request.username}-${index}`"
            class="item"
            :class="{ on: chosen === index }"
            @click="chosen = index"
          >
            <span class="who">{{ request.username }}</span>
            <span class="mail">{{ request.email }}</span>
            <span class="tag">{{ request.via }}</span>
            <span v-if="request.already" class="tag warn">已批准过</span>
          </button>
          <div v-if="errors.length" class="errors">
            <div v-for="(one, index) in errors.slice(0, 5)" :key="index">· {{ one }}</div>
          </div>
          <SoftButton
            :height="28"
            :font-size="12"
            :disabled="!selected || busy"
            @click="doApprove"
          >
            批准选中
          </SoftButton>
        </div>

        <div class="col">
          <div class="sec">注册表（批准过的账户）</div>
          <div v-if="!registry.length" class="empty">还没有批准过账户</div>
          <div v-for="user in registry" :key="user.id" class="item static">
            <span class="who">{{ user.username }}</span>
            <span class="mail">{{ user.email }}</span>
            <span class="tag" :class="{ warn: user.status !== 'active' }">
              {{ user.status === "active" ? "有效" : "停用" }}
            </span>
            <span class="tiny">{{ user.expires_at }}</span>
            <span class="acts">
              <button class="mini" @click="act('renew', user.username)">续期</button>
              <button class="mini" @click="act(user.status === 'active' ? 'disable' : 'enable', user.username)">
                {{ user.status === "active" ? "停用" : "启用" }}
              </button>
              <button class="mini danger" @click="act('delete', user.username)">注销</button>
            </span>
          </div>
        </div>
      </div>

      <div v-if="approval" class="reply">
        <div class="sec">回信全文（验证码 {{ approval.code }}，有效期到 {{ approval.expires_at }}）</div>
        <pre class="reply-body">{{ approval.body }}</pre>
        <div class="row">
          <SoftButton :height="26" :font-size="11.5" @click="copy(approval.body || '')">
            复制回信全文
          </SoftButton>
          <SoftButton :height="26" :font-size="11.5" @click="copy(String(approval.receipt || ''))">
            只复制回执
          </SoftButton>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.32);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 60;
}
.panel {
  width: min(920px, 92vw);
  max-height: 86vh;
  overflow: auto;
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: 14px;
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
header {
  display: flex;
  align-items: flex-start;
  gap: 8px;
}
.h2 {
  font-size: 15px;
  font-weight: 600;
  color: var(--text);
}
.spacer {
  flex: 1;
}
.keyline,
.row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.days {
  width: 74px;
  height: 26px;
}
.cols {
  display: flex;
  gap: 14px;
  min-height: 0;
}
.col {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.sec {
  font-size: 12px;
  color: var(--muted);
  letter-spacing: 0.3px;
}
.item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 8px;
  border-radius: 8px;
  border: 1px solid transparent;
  background: var(--card-alt);
  cursor: pointer;
  text-align: left;
  font-size: 12px;
  color: var(--text-soft);
}
.item.static {
  cursor: default;
}
.item.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, var(--card-alt));
}
.who {
  color: var(--text);
  font-weight: 600;
}
.mail {
  color: var(--muted);
  font-size: 11px;
}
.tag {
  font-size: 10.5px;
  padding: 0 6px;
  border-radius: 999px;
  background: var(--accent-soft);
  color: var(--accent);
}
.tag.warn {
  background: transparent;
  border: 1px solid var(--warning);
  color: var(--warning);
}
.acts {
  margin-left: auto;
  display: flex;
  gap: 4px;
}
.mini {
  font-size: 11px;
  padding: 1px 7px;
  border-radius: 6px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  color: var(--text-soft);
  cursor: pointer;
  text-align: center;
}
.mini.danger {
  color: var(--danger);
}
.empty,
.errors {
  font-size: 11.5px;
  color: var(--muted);
  padding: 4px 2px;
}
.reply {
  border-top: 1px dashed var(--border-soft);
  padding-top: 8px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.reply-body {
  max-height: 160px;
  overflow: auto;
  margin: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  white-space: pre-wrap;
  color: var(--text);
  background: var(--card-alt);
  border-radius: 8px;
  padding: 8px;
}
</style>
