<script setup lang="ts">
// 学生工作：定时通知（给某些同学、通过某渠道、发某内容）
//
// 渠道：邮箱（走已有 SMTP）/ 自定义 webhook / 微信（没有可靠接口 → 整理好内容让你复制去发）
import { computed, onMounted, ref } from "vue";
import Card from "../components/Card.vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import ContactsPanel from "../components/ContactsPanel.vue";
import {
  contactsList,
  studentNoticeDelete,
  studentNotices,
  studentNoticeSave,
  studentNoticeSend,
  type Contact,
  type NoticeTarget,
  type StudentNotice,
} from "../api";

const tab = ref<"notices" | "contacts">("notices");
const rows = ref<StudentNotice[]>([]);
const contacts = ref<Contact[]>([]);
const busy = ref(false);
const message = ref("");

const editing = ref<StudentNotice | null>(null);
const form = ref({
  id: 0,
  title: "",
  body: "",
  channel: "email",
  send_at: "",
  repeat: "",
  targets: [] as NoticeTarget[],
  rawAddress: "",
});

const confirmOpen = ref(false);
const pending = ref<StudentNotice | null>(null);

const CHANNELS = [
  { value: "email", label: "邮箱（走本机发信配置）" },
  { value: "webhook", label: "自定义 webhook（POST JSON）" },
  { value: "wechat", label: "微信（整理好内容，你自己粘贴发送）" },
];

const repeatLabels: Record<string, string> = { "": "一次", daily: "每天", weekly: "每周" };

const targetText = computed(() =>
  form.value.targets
    .map((one) =>
      one.type === "contact"
        ? contacts.value.find((item) => item.id === one.id)?.name || `联系人 #${one.id}`
        : one.label || one.address || "",
    )
    .filter(Boolean)
    .join("、"),
);

function defaultSendAt(): string {
  const now = new Date(Date.now() + 10 * 60 * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(
    now.getHours(),
  )}:${pad(now.getMinutes())}`;
}

async function load() {
  busy.value = true;
  try {
    rows.value = await studentNotices();
    contacts.value = await contactsList("");
  } catch (err) {
    message.value = `读定时通知失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

function startAdd() {
  editing.value = { id: 0 } as StudentNotice;
  form.value = {
    id: 0,
    title: "",
    body: "",
    channel: "email",
    send_at: defaultSendAt(),
    repeat: "",
    targets: [],
    rawAddress: "",
  };
}

function startEdit(row: StudentNotice) {
  editing.value = row;
  form.value = {
    id: row.id,
    title: row.title,
    body: row.body,
    channel: row.channel || "email",
    send_at: row.send_at,
    repeat: row.repeat || "",
    targets: [...(row.targets || [])],
    rawAddress: "",
  };
}

function toggleContact(item: Contact) {
  const index = form.value.targets.findIndex(
    (one) => one.type === "contact" && one.id === item.id,
  );
  if (index >= 0) form.value.targets.splice(index, 1);
  else form.value.targets.push({ type: "contact", id: item.id });
}

function isPicked(item: Contact): boolean {
  return form.value.targets.some((one) => one.type === "contact" && one.id === item.id);
}

function addRawAddress() {
  const address = form.value.rawAddress.trim();
  if (!address) return;
  form.value.targets.push({ type: "raw", address, label: address });
  form.value.rawAddress = "";
}

function dropTarget(index: number) {
  form.value.targets.splice(index, 1);
}

async function submit() {
  if (!form.value.title.trim()) {
    message.value = "标题不能为空";
    return;
  }
  if (!form.value.send_at.trim()) {
    message.value = "要填发送时间";
    return;
  }
  if (!form.value.targets.length) {
    message.value = "至少要选一个收件人";
    return;
  }
  busy.value = true;
  try {
    await studentNoticeSave({
      id: form.value.id,
      title: form.value.title.trim(),
      body: form.value.body,
      channel: form.value.channel,
      send_at: form.value.send_at.trim(),
      repeat: form.value.repeat,
      targets: form.value.targets,
    });
    editing.value = null;
    message.value = "已保存（到点会自动发）";
    await load();
  } catch (err) {
    message.value = `保存失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

async function sendNow(row: StudentNotice) {
  message.value = `正在发送「${row.title}」…`;
  try {
    const result = await studentNoticeSend(row.id);
    message.value = result.ok
      ? `已发出去 ${result.sent} 条`
      : `没发成功：${result.error || "未知原因"}`;
    await load();
  } catch (err) {
    message.value = `发送失败：${String(err)}`;
  }
}

function askRemove(row: StudentNotice) {
  pending.value = row;
  confirmOpen.value = true;
}

async function doRemove() {
  const row = pending.value;
  pending.value = null;
  if (!row) return;
  try {
    await studentNoticeDelete(row.id);
    message.value = `已删掉「${row.title}」`;
    await load();
  } catch (err) {
    message.value = `删除失败：${String(err)}`;
  }
}

/** 微信渠道：把整理好的内容复制到剪贴板，自己去粘贴 */
async function copyContent(row: StudentNotice) {
  const text = `【${row.title}】\n${row.body}`;
  try {
    await navigator.clipboard.writeText(text);
    message.value = "内容已复制，去微信里粘贴发送即可";
  } catch {
    message.value = "复制失败（浏览器不允许）";
  }
}

function statusLabel(row: StudentNotice): string {
  if (row.status === "sent") return "已发送";
  if (row.status === "failed") return "发送失败";
  return "等待发送";
}

function statusColor(row: StudentNotice): string {
  if (row.status === "sent") return "var(--success)";
  if (row.status === "failed") return "var(--danger)";
  return "var(--muted)";
}

onMounted(load);
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">学生工作</h1>
        <div class="muted">
          定时给同学发通知（邮箱 / webhook / 微信）；通讯录在下面和「设置 → 通讯录」都能改
        </div>
      </div>
      <div class="spacer" />
      <SoftButton :height="32" :font-size="12.5" :class="{ on: tab === 'notices' }" @click="tab = 'notices'">
        定时通知 {{ rows.length }}
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" :class="{ on: tab === 'contacts' }" @click="tab = 'contacts'">
        通讯录 {{ contacts.length }}
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" glyph="REFRESH" :disabled="busy" @click="load">刷新</SoftButton>
    </div>

    <div v-if="message" class="hint">{{ message }}</div>

    <Card v-if="tab === 'notices'" :padding="12" class="board">
      <div class="board-head">
        <span class="label">定时通知</span>
        <span class="spacer" />
        <SoftButton variant="primary" :height="28" :font-size="12" @click="startAdd">＋ 新建通知</SoftButton>
      </div>

      <div v-if="editing" class="editor">
        <div class="editor-head">
          <b>{{ form.id ? "改通知" : "新通知" }}</b>
          <span class="spacer" />
          <SoftButton :height="26" :font-size="11.5" @click="editing = null">取消</SoftButton>
          <SoftButton variant="primary" :height="26" :font-size="11.5" :disabled="busy" @click="submit">
            保存
          </SoftButton>
        </div>

        <label class="field">
          <span>标题</span>
          <input v-model="form.title" type="text" placeholder="例如：周五 17:00 前交材料" />
        </label>
        <label class="field">
          <span>内容</span>
          <textarea v-model="form.body" rows="4" placeholder="要发给同学的原话" />
        </label>

        <div class="row2">
          <label>
            <span>渠道</span>
            <select v-model="form.channel">
              <option v-for="one in CHANNELS" :key="one.value" :value="one.value">{{ one.label }}</option>
            </select>
          </label>
          <label>
            <span>发送时间</span>
            <input v-model="form.send_at" type="text" placeholder="2026-11-14 09:00" spellcheck="false" />
          </label>
          <label>
            <span>重复</span>
            <select v-model="form.repeat">
              <option value="">只发一次</option>
              <option value="daily">每天</option>
              <option value="weekly">每周</option>
            </select>
          </label>
        </div>

        <div class="targets">
          <span class="tiny">收件人（通讯录里选，或直接填地址）</span>
          <div class="target-tags">
            <span v-for="(one, index) in form.targets" :key="index" class="tag">
              {{ one.type === "contact"
                ? contacts.find((item) => item.id === one.id)?.name || `#${one.id}`
                : one.address }}
              <button class="x" @click="dropTarget(index)">×</button>
            </span>
            <span v-if="!form.targets.length" class="tiny">还没选</span>
          </div>
          <div class="contact-picker">
            <button
              v-for="item in contacts"
              :key="item.id"
              class="pick"
              :class="{ on: isPicked(item) }"
              :title="item.email || item.wechat || item.extra || ''"
              @click="toggleContact(item)"
            >
              {{ item.name }}
            </button>
            <span v-if="!contacts.length" class="tiny">
              通讯录还是空的 —— 先到「通讯录」里加几个人
            </span>
          </div>
          <div class="raw-row">
            <input
              v-model="form.rawAddress"
              type="text"
              :placeholder="form.channel === 'webhook' ? 'https://…（webhook 地址）' : '也可以直接填邮箱地址'"
              spellcheck="false"
              @keyup.enter="addRawAddress"
            />
            <SoftButton :height="28" :font-size="12" @click="addRawAddress">加上</SoftButton>
          </div>
        </div>
      </div>

      <div class="list">
        <div v-if="!rows.length" class="empty">
          还没有定时通知。点「＋ 新建通知」，选好人和时间，到点会自动发。
        </div>
        <div v-for="row in rows" :key="row.id" class="row-item">
          <div class="line1">
            <b>{{ row.title || "（无标题）" }}</b>
            <Pill :text="statusLabel(row)" :color="statusColor(row)" :font-size="10.5" />
            <span class="when">{{ row.send_at }}</span>
            <span class="when">{{ repeatLabels[row.repeat] || "一次" }}</span>
            <span class="spacer" />
            <SoftButton :height="24" :font-size="11" @click="startEdit(row)">改</SoftButton>
            <SoftButton :height="24" :font-size="11" @click="sendNow(row)">立刻发</SoftButton>
            <SoftButton v-if="row.channel === 'wechat'" :height="24" :font-size="11" @click="copyContent(row)">
              复制内容
            </SoftButton>
            <SoftButton :height="24" :font-size="11" @click="askRemove(row)">删</SoftButton>
          </div>
          <div class="line2">
            <span class="chan">{{ CHANNELS.find((one) => one.value === row.channel)?.label || row.channel }}</span>
            <span v-if="row.last_sent_at">上次发送：{{ row.last_sent_at }}</span>
            <span v-if="row.last_error" class="err">{{ row.last_error }}</span>
          </div>
        </div>
      </div>
    </Card>

    <Card v-else :padding="12" class="board">
      <ContactsPanel />
    </Card>

    <ConfirmDialog
      v-model:open="confirmOpen"
      title="确定删掉这条定时通知吗？"
      :message="`「${pending?.title || ''}」删除后不会再发。`"
      confirm-text="确认删除"
      @confirm="doRemove"
    />
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
  min-height: 0;
}
.board {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow: auto;
}
.board-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border-soft);
}
.label {
  font-size: 13px;
  font-weight: 640;
  color: var(--text-soft);
}
.editor {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
}
.editor-head {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.field > span,
.row2 label > span {
  font-size: 11px;
  color: var(--muted);
}
.row2 {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}
.row2 label {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 150px;
}
.targets {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.target-tags {
  display: flex;
  gap: 5px;
  flex-wrap: wrap;
}
.tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11.5px;
  padding: 2px 7px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 12%, var(--card));
  border: 1px solid color-mix(in srgb, var(--accent) 32%, var(--border-soft));
}
.tag .x {
  border: none;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
  font-size: 12px;
  line-height: 1;
}
.contact-picker {
  display: flex;
  gap: 5px;
  flex-wrap: wrap;
}
.pick {
  border: 1px solid var(--border-soft);
  background: var(--card);
  color: var(--text-soft);
  border-radius: 999px;
  padding: 2px 9px;
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.pick.on {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, var(--card));
}
.raw-row {
  display: flex;
  gap: 6px;
  align-items: center;
}
.raw-row input {
  flex: 1;
  min-width: 120px;
  height: 28px;
  font-size: 12px;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.row-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 7px 10px;
  border-radius: 9px;
  border: 1px solid var(--border-soft);
  background: var(--card);
}
.line1 {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  font-size: 12.5px;
}
.line1 b {
  font-size: 13px;
}
.when {
  font-size: 11px;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}
.line2 {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
  font-size: 11px;
  color: var(--muted);
}
.line2 .err {
  color: var(--danger);
}
.empty {
  font-size: 12px;
  color: var(--muted);
  padding: 10px 2px;
}
.hint {
  font-size: 11.5px;
  color: var(--muted);
}
.tiny {
  font-size: 11px;
  color: var(--muted);
}
.spacer {
  flex: 1;
}
</style>
