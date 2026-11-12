<script setup lang="ts">
// 邮箱：收件箱 / 发件箱 / 写邮件（对应旧版 page_mailbox / inbox_panel / page_outbox）
import { computed, onMounted, ref } from "vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import IconBadge from "../components/IconBadge.vue";
import {
  bounceSync,
  mailAccountState,
  mailAddEvents,
  mailBody,
  mailList,
  mailSend,
  mailSummarize,
  mailSync,
  mails,
  deleteMail,
  openPath,
  pickFiles,
  type MailBody,
  type MailEventSuggestion,
  type MailSummary,
  type MailRecord,
} from "../api";

const tab = ref<"inbox" | "outbox">("inbox");
const inbox = ref<MailSummary[]>([]);
const outbox = ref<MailRecord[]>([]);
const body = ref<MailBody | null>(null);
const selectedUid = ref("");
const busy = ref(false);
const message = ref("");
const account = ref({ imap: false, smtp: false, user: "", from: "", imap_error: "", smtp_error: "" });

/**
 * 每封邮件顶端的 AI 摘要状态。
 *
 * 后端把摘要缓存在 `inbox_messages.summary_zh`，同一封邮件只花一次 token；
 * 这里再留一份内存表，左右来回点也不会重复请求。
 */
const summaryText = ref("");
const summaryError = ref("");
const summaryBusy = ref(false);
const summaryModel = ref("");
const summaryCached = ref(false);
const summaryMemory = new Map<string, { text: string; model: string }>();

/**
 * 摘要里「可能要加进日程的事」：每项一个圆圈，勾完点右下角「添加日程」。
 *
 * 勾选状态放在 `picked` 里（按序号），加成功的会从列表里移走。
 */
const eventItems = ref<MailEventSuggestion[]>([]);
const pickedEvents = ref<number[]>([]);
const addingEvents = ref(false);
const eventsNote = ref("");

const composeOpen = ref(false);
const composeFiles = ref<string[]>([]);
const composeTo = ref("");
const composeSubject = ref("");
const composeBody = ref("");
const sending = ref(false);

const statusText = computed(() => {
  const parts = [`共 ${inbox.value.length || 0} 封`];
  parts.push(account.value.imap ? "账户 " + account.value.user : "还没配收件箱");
  return parts.join(" · ");
});

async function refreshAccount() {
  account.value = await mailAccountState();
}

/**
 * 收件箱：**先读本地缓存**（秒开、离线也能看），再增量同步新邮件。
 *
 * 对应旧版 `imap_inbox` 的 uid 缓存：邮件头存进 `inbox_messages`，
 * 每次只取服务器上「UID 比本地最大 UID 更大」的那些。
 */
async function refreshInbox() {
  busy.value = true;
  message.value = "";
  try {
    const cached = await mailList(30);
    if (cached.ok) {
      inbox.value = cached.items;
      if (!selectedUid.value && inbox.value.length) await open(inbox.value[0]);
    }
    message.value = "正在同步新邮件…";
    const sync = await mailSync(50);
    if (!sync.ok) {
      message.value = cached.ok && inbox.value.length
        ? `显示本地缓存（${inbox.value.length} 封）；同步失败：${sync.error || "未知错误"}`
        : sync.error || "同步失败";
      return;
    }
    const fresh = await mailList(30);
    if (fresh.ok) {
      inbox.value = fresh.items;
      if (!selectedUid.value && inbox.value.length) await open(inbox.value[0]);
    }
    const added = sync.added || 0;
    message.value = added
      ? `新增 ${added} 封（本地共缓存 ${sync.cached ?? fresh.cached ?? inbox.value.length} 封）`
      : "没有新邮件";
  } catch (err) {
    message.value = `收信失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

async function refreshOutbox() {
  outbox.value = await mails(100);
}

/**
 * 检查退信：扫收件箱里的 PostMaster / Delivery Status，把发件箱里对应的那封标成「已退信」。
 * 对应旧版 `bounce.py::sync`（本地判断，不发信）。
 */
const bounceBusy = ref(false);
async function checkBounces() {
  if (bounceBusy.value) return;
  bounceBusy.value = true;
  message.value = "正在检查退信…";
  try {
    const result = await bounceSync();
    if (!result.ok) {
      message.value = `检查退信失败：${result.error}`;
      return;
    }
    await refreshOutbox();
    const marked = result.marked || [];
    message.value = marked.length
      ? `发现 ${marked.length} 封退信：${marked
          .map((one) => `${one.to || "?"}（${[one.code, one.reason].filter(Boolean).join(" ")}）`)
          .join("、")}`
      : result.scanned
        ? `扫了 ${result.scanned} 封退信，发件箱里没找到对应邮件`
        : "收件箱里没有退信";
  } catch (err) {
    message.value = `检查退信失败：${String(err)}`;
  } finally {
    bounceBusy.value = false;
  }
}

async function open(item: MailSummary) {
  selectedUid.value = item.uid;
  body.value = null;
  message.value = "正在读正文…";
  // 换了一封，摘要先清空（同一封回看直接拿内存里的）
  summaryError.value = "";
  summaryModel.value = "";
  summaryCached.value = false;
  summaryText.value = summaryMemory.get(item.uid)?.text || "";
  summaryBusy.value = false;
  // 换了一封：日程候选也清掉（等这封的摘要回来再填）
  eventItems.value = [];
  pickedEvents.value = [];
  eventsNote.value = "";
  const result = await mailBody(item.uid);
  if (result.ok) {
    body.value = result.body;
    message.value = "";
    // 正文到手就顺手把摘要生成出来（没缓存才真发请求），失败只影响摘要那块
    // 摘要已缓存的也调一次：后端会顺手把「可能要加的日程」补抽出来
    void loadSummary(item.uid, false);
  } else {
    message.value = result.error;
  }
}

/**
 * 生成 / 重取一封邮件的 AI 摘要。走用户自己在「AI」页配的接口。
 *
 * `force` 是「重新生成」：绕过后端缓存，重新花一次 token。
 */
async function loadSummary(uid: string, force = false) {
  if (!uid || summaryBusy.value) return;
  summaryBusy.value = true;
  summaryError.value = "";
  try {
    const result = await mailSummarize(uid, force);
    // 生成期间用户可能已经点开别的邮件，这时别把旧结果盖上去
    if (selectedUid.value !== uid) return;
    if (result.ok && result.summary) {
      summaryText.value = result.summary;
      summaryModel.value = result.model || "";
      summaryCached.value = !!result.cached;
      summaryMemory.set(uid, { text: result.summary, model: result.model || "" });
      eventItems.value = result.events || [];
      pickedEvents.value = [];
    } else {
      summaryText.value = "";
      summaryError.value = result.error || "没拿到摘要";
      eventItems.value = [];
    }
  } catch (err) {
    if (selectedUid.value === uid) summaryError.value = `摘要失败：${String(err)}`;
  } finally {
    summaryBusy.value = false;
  }
}

/** 勾 / 取消一个「可能要加进日程的事」 */
function toggleEvent(index: number) {
  if (pickedEvents.value.includes(index)) {
    pickedEvents.value = pickedEvents.value.filter((one) => one !== index);
  } else {
    pickedEvents.value = [...pickedEvents.value, index];
  }
}

/** 摘要右上角的日期写法：2026-11-18 12:00 → 11-18 12:00 */
function shortWhen(item: MailEventSuggestion): string {
  const date = (item.date || "").slice(5);
  const start = (item.start_time || "").trim();
  return start ? `${date} ${start}` : date;
}

/** 把勾选的日程真的加进日历（后端走 AI 的 schedule_add 工具） */
async function addPickedEvents() {
  if (addingEvents.value || !pickedEvents.value.length) return;
  const items = pickedEvents.value
    .map((index) => eventItems.value[index])
    .filter((one): one is MailEventSuggestion => !!one);
  if (!items.length) return;
  addingEvents.value = true;
  eventsNote.value = "正在加进日历…";
  try {
    const result = await mailAddEvents(items);
    if (result.added > 0) {
      // 加成功的从列表里移走，免得重复点
      const done = new Set(items.map((one) => `${one.title}|${one.date}`));
      eventItems.value = eventItems.value.filter((one) => !done.has(`${one.title}|${one.date}`));
      pickedEvents.value = [];
      eventsNote.value = result.error
        ? `加了 ${result.added} 条，有 ${result.error}`
        : `已加进日历：${items.map((one) => one.title).join("、")}`;
    } else {
      eventsNote.value = result.error || "一条都没加进去";
    }
  } catch (err) {
    eventsNote.value = `添加失败：${String(err)}`;
  } finally {
    addingEvents.value = false;
  }
}

function sanitize(html: string): string {
  return html
    .replace(/<script[\s\S]*?<\/script>/gi, "")
    .replace(/<style[\s\S]*?<\/style>/gi, "")
    .replace(/<iframe[\s\S]*?<\/iframe>/gi, "")
    .replace(/\son[a-z]+\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)/gi, "")
    .replace(/javascript:/gi, "#");
}

function openCompose() {
  composeOpen.value = true;
  composeFiles.value = [];
  composeTo.value = "";
  composeSubject.value = "";
  composeBody.value = "";
}

async function chooseFiles() {
  const picked = await pickFiles(undefined, true);
  for (const file of picked) {
    if (!composeFiles.value.includes(file)) composeFiles.value.push(file);
  }
}

async function send() {
  if (!composeTo.value.trim()) {
    message.value = "收件人不能为空";
    return;
  }
  if (!/^\s*[^@\s]+@[^@\s]+\.[^@\s]+/.test(composeTo.value)) {
    message.value = "收件人地址看起来不对";
    return;
  }
  sending.value = true;
  try {
    const result = await mailSend(
      composeTo.value,
      composeSubject.value || "（无主题）",
      composeBody.value,
      composeFiles.value,
    );
    if (result.ok) {
      composeOpen.value = false;
      message.value = "已发送";
      await refreshOutbox();
    } else {
      message.value = result.error;
    }
  } finally {
    sending.value = false;
  }
}

async function remove(record: MailRecord) {
  await deleteMail(record.id);
  await refreshOutbox();
}

function fmtDate(value: string): string {
  if (!value) return "";
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value.slice(0, 16);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())} ${pad(
    parsed.getHours(),
  )}:${pad(parsed.getMinutes())}`;
}

async function switchTab(next: "inbox" | "outbox") {
  tab.value = next;
  if (next === "outbox") await refreshOutbox();
}

onMounted(async () => {
  await refreshAccount();
  if (account.value.imap) await refreshInbox();
  else message.value = account.value.imap_error;
  await refreshOutbox();
});
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">邮箱</h1>
        <div class="muted">{{ statusText }}</div>
      </div>
      <div class="spacer" />
      <SoftButton
        :variant="tab === 'inbox' ? 'primary' : 'ghost'"
        :height="32"
        :font-size="12.5"
        glyph="INBOX"
        @click="switchTab('inbox')"
      >
        收件箱 {{ inbox.length }}
      </SoftButton>
      <SoftButton
        :variant="tab === 'outbox' ? 'primary' : 'ghost'"
        :height="32"
        :font-size="12.5"
        glyph="SEND"
        @click="switchTab('outbox')"
      >
        发件箱
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" glyph="EDIT" @click="openCompose">写邮件</SoftButton>
      <SoftButton
        :height="32"
        :font-size="12.5"
        glyph="CHECK"
        :disabled="busy"
        @click="checkBounces"
      >
        {{ bounceBusy ? "检查退信…" : "检查退信" }}
      </SoftButton>
      <SoftButton
        :height="32"
        :font-size="12.5"
        glyph="REFRESH"
        :disabled="busy"
        @click="tab === 'inbox' ? refreshInbox() : refreshOutbox()"
      >
        刷新
      </SoftButton>
    </div>

    <div v-if="tab === 'inbox'" class="body">
      <div class="list">
        <div v-if="message" class="hint">{{ message }}</div>
        <button
          v-for="item in inbox"
          :key="item.uid"
          class="row"
          :class="{ on: selectedUid === item.uid, unread: item.unread }"
          @click="open(item)"
        >
          <span class="subject">{{ item.subject }}</span>
          <span class="meta">{{ item.from }}</span>
          <span class="date">{{ fmtDate(item.date) }}</span>
        </button>
      </div>

      <div class="preview">
        <template v-if="body">
          <div class="preview-head">
            <IconBadge glyph="MAIL" tone="sky" :size="26" />
            <div class="head-text">
              <div class="subject">{{ body.subject }}</div>
              <div class="meta">{{ body.from }} · {{ fmtDate(body.date) }}</div>
            </div>
          </div>
          <div class="ai-summary">
            <div class="ai-head">
              <span class="ai-chip">AI</span>
              <span class="ai-title">摘要</span>
              <span v-if="summaryBusy" class="ai-note">正在生成…</span>
              <span v-else-if="summaryModel" class="ai-note">
                {{ summaryModel }}<template v-if="summaryCached"> · 已缓存</template>
              </span>
              <span class="spacer" />
              <button
                class="ai-regen"
                :disabled="summaryBusy"
                @click="loadSummary(selectedUid, true)"
              >
                {{ summaryBusy ? "生成中…" : summaryText ? "重新生成" : "生成摘要" }}
              </button>
            </div>
            <div v-if="summaryText" class="ai-text">{{ summaryText }}</div>
            <div v-else-if="summaryError" class="ai-error">{{ summaryError }}</div>
            <div v-else class="ai-note">{{ summaryBusy ? "把正文压成两三行，稍等几秒…" : "还没有摘要" }}</div>
            <!-- 可能要加进日程的事：每项一个圆圈，勾完点右下角「添加日程」 -->
            <div v-if="eventItems.length" class="ai-events">
              <div class="ai-events-title">可能要加进日程的事（勾选后点右下角）</div>
              <button
                v-for="(item, index) in eventItems"
                :key="`${item.title}-${index}`"
                class="event-row"
                :class="{ on: pickedEvents.includes(index) }"
                :title="item.note || item.title"
                @click="toggleEvent(index)"
              >
                <span class="event-dot" :class="{ on: pickedEvents.includes(index) }" />
                <span class="event-when">{{ shortWhen(item) }}</span>
                <span class="event-title">{{ item.title }}</span>
              </button>
              <div class="ai-events-foot">
                <span v-if="eventsNote" class="ai-events-note">{{ eventsNote }}</span>
                <span class="spacer" />
                <button
                  class="event-add"
                  :disabled="addingEvents || !pickedEvents.length"
                  @click="addPickedEvents"
                >
                  {{ addingEvents ? "正在添加…" : "添加日程" }}
                </button>
              </div>
            </div>
            <!-- 加完之后列表可能空了，但结果要说一声 -->
            <div v-else-if="eventsNote" class="ai-note">{{ eventsNote }}</div>
          </div>
          <div class="preview-body">
            <iframe
              v-if="body.html"
              class="mail-frame"
              :srcdoc="sanitize(body.html)"
              sandbox="allow-same-origin"
            />
            <pre v-else class="text">{{ body.text || "（这封邮件没有正文）" }}</pre>
          </div>
          <div v-if="body.attachments.length" class="attachments">
            <button
              v-for="file in body.attachments"
              :key="file.path"
              class="attach"
              @click="openPath(file.path)"
            >
              <IconBadge glyph="ATTACH" tone="amber" :size="22" />
              <span class="name">{{ file.name }}</span>
              <span class="size">{{ Math.max(1, Math.round(file.size / 1024)) }} KB</span>
              <span class="hint-text">点一下用系统程序打开</span>
            </button>
          </div>
        </template>
        <div v-else class="empty">{{ message || "选左边一封邮件就能读正文" }}</div>
      </div>
    </div>

    <div v-else class="outbox">
      <div v-if="!outbox.length" class="empty">发件箱还是空的</div>
      <div v-for="record in outbox" :key="record.id" class="outbox-row">
        <Pill
          :text="
            record.status === 'sent'
              ? '已发送'
              : record.status === 'bounced'
                ? '已退信'
                : record.status === 'failed'
                  ? '发送失败'
                  : '草稿'
          "
          :color="
            record.status === 'bounced'
              ? 'var(--danger)'
              : record.status === 'failed'
                ? 'var(--danger)'
                : 'var(--muted)'
          "
          :font-size="11.5"
        />
        <div class="outbox-text">
          <div class="subject">{{ record.subject || "（无主题）" }}</div>
          <div class="meta">
            收件人 {{ record.to }} · {{ fmtDate(record.sent_at || record.created_at) }}
            <span v-if="record.error"> · {{ record.error }}</span>
          </div>
        </div>
        <SoftButton :height="26" :font-size="11.5" @click="remove(record)">删除</SoftButton>
      </div>
    </div>

    <div v-if="composeOpen" class="modal">
      <div class="modal-card">
        <div class="modal-head">
          <span class="h2">写邮件</span>
          <span class="spacer" />
          <SoftButton :height="28" :font-size="12" @click="composeOpen = false">关闭</SoftButton>
        </div>
        <label class="field">
          <span>收件人</span>
          <input v-model="composeTo" type="text" placeholder="someone@example.com" />
        </label>
        <label class="field">
          <span>主题</span>
          <input v-model="composeSubject" type="text" placeholder="（可留空）" />
        </label>
        <label class="field grow">
          <span>正文</span>
          <textarea v-model="composeBody" placeholder="写点什么…" />
        </label>
        <div class="attach-row">
          <SoftButton :height="28" :font-size="12" glyph="ATTACH" @click="chooseFiles">
            选择附件
          </SoftButton>
          <span v-for="file in composeFiles" :key="file" class="attach-name">
            {{ file.split("\\").pop() }}
            <button class="x" @click="composeFiles = composeFiles.filter((one) => one !== file)">×</button>
          </span>
        </div>
        <div class="modal-foot">
          <span class="tiny">{{ account.smtp ? `发件人 ${account.from}` : account.smtp_error }}</span>
          <span class="spacer" />
          <SoftButton variant="primary" :height="32" :font-size="12.5" :disabled="sending" @click="send">
            {{ sending ? "发送中…" : "发送" }}
          </SoftButton>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
}
.body {
  flex: 1;
  display: flex;
  gap: 12px;
  min-height: 0;
}
.list {
  width: 330px;
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 3px;
  overflow-y: auto;
}
.row {
  display: flex;
  flex-direction: column;
  /* 全局 .row 是 align-items:center，邮件列表竖排会把标题居中，这里覆盖成左对齐 */
  align-items: stretch;
  gap: 3px;
  padding: 9px 11px;
  border-radius: var(--radius-card);
  border: 1px solid var(--border-soft);
  background: var(--card);
  text-align: left;
  font-family: inherit;
  cursor: pointer;
}
.row.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 7%, var(--card));
}
.row .subject {
  font-size: 13.5px;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row.unread .subject {
  font-weight: 650;
}
.row .meta,
.row .date {
  font-size: 11.5px;
  color: var(--muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.preview {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-card);
  padding: 12px 14px;
  min-height: 0;
}
.preview-head {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-soft);
}
.head-text {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}
.preview-head .subject {
  font-size: 15px;
  font-weight: 650;
}
.preview-head .meta {
  font-size: 12px;
  color: var(--muted);
}
.preview-body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}
/* 邮件顶端的 AI 摘要：淡淡一块，不抢正文的位置，也不做光泽 */
.ai-summary {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 9px 11px;
  border-radius: 11px;
  border: 1px solid color-mix(in srgb, var(--accent) 26%, var(--border-soft));
  background: color-mix(in srgb, var(--accent) 5%, var(--card));
  flex: none;
}
.ai-head {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.ai-chip {
  flex: none;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.4px;
  color: var(--accent);
  border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
  border-radius: 5px;
  padding: 0 4px;
  line-height: 15px;
}
.ai-title {
  font-size: 12px;
  font-weight: 650;
  color: var(--text);
  flex: none;
}
.ai-note {
  font-size: 11px;
  color: var(--muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ai-regen {
  flex: none;
  border: none;
  background: transparent;
  color: var(--accent);
  font-family: inherit;
  font-size: 11.5px;
  padding: 2px 4px;
  border-radius: 6px;
  cursor: pointer;
}
.ai-regen:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}
.ai-regen:disabled {
  color: var(--muted);
  cursor: default;
}
.ai-text {
  font-size: 12.5px;
  line-height: 1.7;
  color: var(--text-soft);
  /* 模型给的是「· 」开头的几行，靠 pre-line 保住换行 */
  white-space: pre-line;
  user-select: text;
}
.ai-error {
  font-size: 12px;
  line-height: 1.6;
  color: var(--muted);
}
/* 「可能要加进日程的事」：一行一件，前面一个圆圈 */
.ai-events {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding-top: 6px;
  border-top: 1px dashed color-mix(in srgb, var(--accent) 30%, var(--border-soft));
}
.ai-events-title {
  font-size: 11px;
  color: var(--muted);
}
.event-row {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 4px 6px;
  border: none;
  border-radius: 7px;
  background: transparent;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
}
.event-row:hover {
  background: color-mix(in srgb, var(--accent) 8%, transparent);
}
.event-dot {
  flex: none;
  width: 13px;
  height: 13px;
  border-radius: 999px;
  border: 1.5px solid color-mix(in srgb, var(--accent) 55%, var(--border));
  background: transparent;
}
.event-dot.on {
  border-color: var(--accent);
  background: var(--accent);
  box-shadow: inset 0 0 0 2px var(--card);
}
.event-when {
  flex: none;
  font-size: 11px;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
}
.event-title {
  font-size: 12.5px;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ai-events-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 2px;
}
.ai-events-note {
  font-size: 11px;
  color: var(--muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 右下角「添加日程」 */
.event-add {
  flex: none;
  height: 26px;
  padding: 0 12px;
  border-radius: 999px;
  border: 1px solid var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, var(--card));
  color: var(--accent);
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
}
.event-add:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 22%, var(--card));
}
.event-add:disabled {
  border-color: var(--border-soft);
  background: transparent;
  color: var(--faint);
  cursor: default;
}
.mail-frame {
  width: 100%;
  height: 100%;
  border: none;
  background: transparent;
}
.text {
  margin: 0;
  /* 纯文本邮件用界面字体读着更舒服（等宽字体会让中文和标点间距很怪） */
  font-family: var(--font);
  font-size: 13px;
  line-height: 1.75;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
}
.attachments {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding-top: 8px;
  border-top: 1px solid var(--border-soft);
}
.attach-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding-top: 2px;
}
.attach-name {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11.5px;
  color: var(--text-soft);
  background: var(--card-alt);
  border-radius: 8px;
  padding: 3px 8px;
}
.attach-name .x {
  border: none;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
  font-size: 12px;
  line-height: 1;
}
.attach {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 5px 10px 5px 6px;
  border-radius: 10px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  font-family: inherit;
  cursor: pointer;
}
.attach .name {
  font-size: 12.5px;
  color: var(--text);
  max-width: 190px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.attach .size,
.attach .hint-text {
  font-size: 11px;
  color: var(--muted);
}
.outbox {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.outbox-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 12px;
  border-radius: var(--radius-card);
  border: 1px solid var(--border-soft);
  background: var(--card);
}
.outbox-text {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.outbox-text .subject {
  font-size: 13.5px;
}
.outbox-text .meta {
  font-size: 11.5px;
  color: var(--muted);
}
.hint {
  font-size: 12px;
  color: var(--muted);
  padding: 4px 2px;
}
.empty {
  font-size: 12.5px;
  color: var(--muted);
  padding: 10px 2px;
}
.modal {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.28);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 40;
}
.modal-card {
  width: 620px;
  max-height: 82vh;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 16px;
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  box-shadow: 0 18px 48px rgba(0, 0, 0, 0.28);
}
.modal-head,
.modal-foot {
  display: flex;
  align-items: center;
  gap: 8px;
}
.modal-head .h2 {
  font-size: 16px;
  font-weight: 650;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.field > span {
  font-size: 12.5px;
  color: var(--text-soft);
}
.field.grow {
  flex: 1;
  min-height: 0;
}
.field textarea {
  flex: 1;
  min-height: 180px;
  resize: vertical;
  font-size: 14px;
  line-height: 1.7;
}
.spacer {
  flex: 1;
}
/* 列表行一律左对齐（button 的默认 text-align 是 center，会把标题也居中） */
.list .row,
.list .row > span {
  text-align: left;
  align-items: flex-start;
}
</style>
