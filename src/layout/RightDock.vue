<script setup lang="ts">
// 右栏：日程 / AI / 文献 三个标签（旧版 right_dock.py）
// AI 标签是真对话：Markdown + KaTeX 渲染，走 Rust 侧的 OpenAI 兼容接口。
import { computed, nextTick, onMounted, ref, watch } from "vue";
import IconGlyph from "../components/IconGlyph.vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import { renderMarkdown } from "../markdown";
import {
  aiChatTools,
  aiSaveSession,
  aiSessions,
  aiState,
  type AiState,
  type ChatMessage,
  type ChatSession,
  type ToolLogEntry,
  type UpcomingItem,
} from "../api";

const props = withDefaults(
  defineProps<{ items: UpcomingItem[]; initialTab?: "日程" | "AI" | "文献" }>(),
  { initialTab: "日程" },
);
const emit = defineEmits<{
  (e: "open", item: UpcomingItem): void;
  /** 切换标签时通知外面（用来记住 agenda_mode / 左导航高亮） */
  (e: "tab", value: "日程" | "AI" | "文献"): void;
}>();

const tabs = ["日程", "AI", "文献"] as const;
const active = ref<(typeof tabs)[number]>(props.initialTab);

watch(
  () => props.initialTab,
  (value) => {
    if (value) active.value = value;
  },
);

watch(active, (value) => emit("tab", value));

const messages = ref<{
  role: "user" | "assistant";
  text: string;
  tools?: ToolLogEntry[];
}[]>([]);
const draft = ref("");
const busy = ref(false);
const error = ref("");
const state = ref<AiState | null>(null);
const history = ref<ChatSession[]>([]);
// AI 全屏：把右栏展开成覆盖整个内容区（用户要求「AI 要能全屏」）
const wide = ref(localStorage.getItem("danlume.aiWide") === "1");
function toggleWide() {
  wide.value = !wide.value;
  localStorage.setItem("danlume.aiWide", wide.value ? "1" : "0");
  void scrollDown();
}
const showHistory = ref(false);
const scroller = ref<HTMLElement | null>(null);

const modelLabel = computed(() => {
  const value = state.value;
  if (!value) return "AI";
  return value.has_key ? value.model || value.name || "AI" : "未配置";
});

async function refreshState() {
  try {
    state.value = await aiState();
    error.value = state.value.ok ? "" : state.value.error;
  } catch (err) {
    error.value = String(err);
  }
}

async function loadHistory() {
  history.value = await aiSessions(30);
}

async function scrollDown() {
  await nextTick();
  if (scroller.value) scroller.value.scrollTop = scroller.value.scrollHeight;
}

async function send() {
  const text = draft.value.trim();
  if (!text || busy.value) return;
  messages.value.push({ role: "user", text });
  draft.value = "";
  busy.value = true;
  error.value = "";
  await scrollDown();
  try {
    const payload: ChatMessage[] = messages.value.map((one) => ({
      role: one.role,
      content: one.text,
    }));
    const result = await aiChatTools(payload);
    messages.value.push({
      role: "assistant",
      text: result.text,
      tools: result.log || [],
    });
    try {
      await aiSaveSession(
        text.slice(0, 30),
        state.value?.model || "",
        JSON.stringify(messages.value),
      );
    } catch {
      // 存历史失败不影响对话
    }
  } catch (err) {
    error.value = String(err);
  } finally {
    busy.value = false;
    await scrollDown();
  }
}

function newChat() {
  messages.value = [];
  error.value = "";
  draft.value = "";
}

function restore(session: ChatSession) {
  try {
    const rows = JSON.parse(session.payload || "[]");
    if (Array.isArray(rows)) {
      messages.value = rows
        .filter((one) => one && (one.role === "user" || one.role === "assistant"))
        .map((one) => ({
          role: one.role as "user" | "assistant",
          text: String(one.text ?? one.content ?? ""),
        }));
    }
    showHistory.value = false;
    scrollDown();
  } catch {
    error.value = "这条历史读不出来";
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    send();
  }
}

onMounted(async () => {
  await refreshState();
  await loadHistory();
});

// 从左侧「AI」切过来时，顺手把配置状态和历史刷新一遍（可能刚在设置里改过 Key）
watch(active, async (value) => {
  if (value !== "AI") return;
  await refreshState();
  await loadHistory();
  await scrollDown();
});
</script>

<template>
  <aside class="dock">
    <header>
      <div class="tabs">
        <button
          v-for="tab in tabs"
          :key="tab"
          class="tab"
          :class="{ on: active === tab }"
          @click="active = tab"
        >
          {{ tab }}
        </button>
      </div>
      <button class="wide-btn" :title="wide ? '还原宽度' : '全屏展开'" @click="toggleWide">{{ wide ? '⤡' : '⤢' }}</button>
      <span class="spacer" />
      <button class="icon" title="新对话" @click="newChat">
        <IconGlyph name="ADD" :size="13" />
      </button>
      <button class="icon" title="历史会话" @click="(showHistory = !showHistory), loadHistory()">
        <IconGlyph name="CLOCK" :size="13" />
      </button>
      <button class="icon" title="刷新 AI 配置" @click="refreshState">
        <IconGlyph name="REFRESH" :size="13" />
      </button>
    </header>

    <div class="body">
      <template v-if="active === '日程'">
        <div class="section-title">最近日程</div>
        <div v-if="!items.length" class="empty">今天还没有日程</div>
        <div
          v-for="item in items"
          :key="item.id"
          class="agenda-row"
          @click="emit('open', item)"
        >
          <span class="time">{{ item.begin }}–{{ item.finish }}</span>
          <span class="title" :class="{ done: item.done }">{{ item.title }}</span>
        </div>
      </template>

      <template v-else-if="active === 'AI'">
        <div class="ai-head">
          <Pill :text="modelLabel" :font-size="11" />
          <span class="spacer" />
          <span v-if="messages.length" class="tiny">{{ messages.length }} 条</span>
        </div>

        <div v-if="showHistory" class="history">
          <div class="section-title">历史会话</div>
          <button
            v-for="session in history"
            :key="session.id"
            class="history-row"
            @click="restore(session)"
          >
            <span class="name">{{ session.title || "（未命名）" }}</span>
            <span class="meta">{{ session.model }} · {{ session.updated_at.slice(5, 16) }}</span>
          </button>
          <div v-if="!history.length" class="empty">还没有历史会话</div>
        </div>

        <div ref="scroller" class="chat">
          <div v-if="!messages.length" class="empty">
            问点什么吧。支持 Markdown 和 LaTeX 公式；Enter 发送，Shift+Enter 换行。
          </div>
          <div v-for="(message, index) in messages" :key="index" class="bubble" :class="message.role">
            <div
              v-if="message.role === 'assistant'"
              class="md"
              v-html="renderMarkdown(message.text)"
            />
            <div v-else class="plain">{{ message.text }}</div>
            <div v-if="message.tools && message.tools.length" class="tools-used">
              <span class="tool-head">用了 {{ message.tools.length }} 次工具</span>
              <span
                v-for="(tool, index2) in message.tools"
                :key="index2"
                class="tool-chip"
                :class="{ bad: !tool.ok }"
                :title="tool.summary"
              >
                {{ tool.name }}
              </span>
            </div>
          </div>
          <div v-if="busy" class="bubble assistant">
            <div class="typing">正在思考…</div>
          </div>
          <div v-if="error" class="error">{{ error }}</div>
        </div>

        <div class="composer">
          <textarea
            v-model="draft"
            class="input"
            rows="3"
            placeholder="问点什么…（Enter 发送 / Shift+Enter 换行）"
            @keydown="onKeydown"
          />
          <div class="composer-row">
            <span class="tiny">{{ state?.has_key ? state.base_url : "还没配好 AI" }}</span>
            <span class="spacer" />
            <SoftButton variant="primary" :height="28" :font-size="12" :disabled="busy" @click="send">
              发送
            </SoftButton>
          </div>
        </div>
      </template>

      <template v-else>
        <div class="section-title">文献</div>
        <div class="empty">文献库在左边「文献库」页里管理；这里以后放当前文献的摘要与流程图。</div>
      </template>
    </div>
  </aside>
</template>

<style scoped>
.dock {
  width: var(--dock-w);
  flex: none;
  display: flex;
  flex-direction: column;
  min-height: 0;
  background: transparent;
  border-left: 1px solid var(--border-soft);
}
header {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px 6px;
  flex: none;
}
.tabs {
  display: flex;
  gap: 4px;
}
.tab {
  height: 22px;
  padding: 0 9px;
  border-radius: var(--radius-pill);
  border: none;
  background: transparent;
  color: var(--muted);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.tab.on {
  background: var(--card);
  color: var(--text);
  font-weight: 600;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
}
.spacer {
  flex: 1;
}
.icon {
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  color: var(--muted);
  border-radius: 6px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.icon:hover {
  background: var(--card-alt);
}
.body {
  flex: 1;
  overflow-y: auto;
  padding: 4px 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-height: 0;
}
.section-title {
  font-size: 13px;
  font-weight: 640;
  color: var(--text-soft);
  margin: 6px 2px 4px;
}
.empty {
  font-size: 12px;
  color: var(--muted);
  line-height: 1.6;
  padding: 2px;
}
.agenda-row {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 6px 8px;
  border-radius: 9px;
  cursor: pointer;
  border-left: 3px solid var(--accent);
  background: color-mix(in srgb, var(--accent) 6%, transparent);
}
.agenda-row:hover {
  background: color-mix(in srgb, var(--accent) 11%, transparent);
}
.time {
  font-size: 11.5px;
  color: var(--muted);
}
.title {
  font-size: 13px;
  color: var(--text);
  overflow-wrap: anywhere;
}
.title.done {
  text-decoration: line-through;
  color: var(--muted);
}
.ai-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 2px 4px;
}
.history {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border-soft);
}
.history-row {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 5px 8px;
  border-radius: 8px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  font-family: inherit;
  text-align: left;
  cursor: pointer;
}
.history-row .name {
  font-size: 12px;
  color: var(--text);
}
.history-row .meta {
  font-size: 10.5px;
  color: var(--muted);
}
.chat {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 2px;
  min-height: 0;
}
.bubble {
  border-radius: 12px;
  padding: 7px 10px;
  font-size: 13px;
  line-height: 1.7;
  max-width: 100%;
  overflow-wrap: anywhere;
}
.bubble.user {
  background: color-mix(in srgb, var(--accent) 12%, var(--card));
  color: var(--text);
  align-self: flex-end;
}
.bubble.assistant {
  background: var(--card);
  border: 1px solid var(--border-soft);
  color: var(--text);
}
.typing {
  color: var(--muted);
  font-size: 12px;
}
.error {
  font-size: 11.5px;
  color: var(--danger);
  background: color-mix(in srgb, var(--danger) 9%, transparent);
  border-radius: 8px;
  padding: 6px 8px;
  overflow-wrap: anywhere;
}
.md :deep(p) {
  margin: 3px 0;
}
.md :deep(h3),
.md :deep(h4),
.md :deep(h5) {
  margin: 6px 0 3px;
  font-size: 13.5px;
}
.md :deep(ul),
.md :deep(ol) {
  margin: 3px 0;
  padding-left: 18px;
}
.md :deep(code.md-code) {
  font-family: var(--font-mono);
  font-size: 11.5px;
  background: var(--card-alt);
  padding: 1px 4px;
  border-radius: 4px;
}
.md :deep(.md-pre) {
  background: var(--card-alt);
  border-radius: 8px;
  margin: 5px 0;
  overflow: hidden;
}
.md :deep(.md-pre-head) {
  font-size: 10.5px;
  color: var(--muted);
  padding: 3px 8px;
  border-bottom: 1px solid var(--border-soft);
}
.md :deep(.md-pre pre) {
  margin: 0;
  padding: 7px 9px;
  overflow-x: auto;
  font-family: var(--font-mono);
  font-size: 11.5px;
  line-height: 1.6;
}
.md :deep(.md-display) {
  text-align: center;
  margin: 6px 0;
  overflow-x: auto;
}
.composer {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding-top: 6px;
  border-top: 1px solid var(--border-soft);
}
.input {
  width: 100%;
  resize: none;
  font-size: 13px;
  line-height: 1.6;
  min-height: 62px;
}
.composer-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.tools-used {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px;
  margin-top: 6px;
  padding-top: 6px;
  border-top: 1px dashed var(--border-soft);
}
.tool-head {
  font-size: 10.5px;
  color: var(--muted);
}
.tool-chip {
  font-size: 10.5px;
  padding: 1px 6px;
  border-radius: 999px;
  background: var(--accent-soft);
  color: var(--accent);
  font-family: var(--font-mono);
}
.tool-chip.bad {
  color: var(--danger);
  border: 1px solid var(--danger);
  background: transparent;
}

/* AI 全屏：占满中间内容区（左导航右侧到底），可再点一次还原 */
.dock.wide {
  position: fixed;
  top: var(--titlebar-h, 46px);
  right: 0;
  bottom: 0;
  left: var(--sidebar-w, 208px);
  width: auto;
  z-index: 40;
  box-shadow: -8px 0 28px -18px rgba(0, 0, 0, 0.35);
}
.wide-btn {
  width: 22px;
  height: 22px;
  border-radius: 6px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text-soft);
  font-size: 12px;
  line-height: 1;
  padding: 0;
  cursor: pointer;
  text-align: center;
}
.wide-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}
/* AI 全屏时，聊天区与输入框也要跟着变宽（否则只有背景变宽、内容还在窄条里） */
.dock.wide .chat,
.dock.wide .composer,
.dock.wide .composer .input,
.dock.wide .composer-row,
.dock.wide .history {
  max-width: none;
  width: 100%;
}
.dock.wide .bubble {
  max-width: min(920px, 92%);
}</style>
