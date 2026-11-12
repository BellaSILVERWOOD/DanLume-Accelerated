<script setup lang="ts">
// 右栏：日程 / AI / 文献 三个标签（旧版 right_dock.py）
// AI 标签整块交给 `components/AiPanel.vue`（照旧版 ai_panel.py / ai_api_panel.py 重做：
// 场景提示词、⋯ 菜单、附件、用量、历史会话、WebUI 内嵌、系统提示词）。
import { ref, watch } from "vue";
import AiPanel from "../components/AiPanel.vue";
import type { UpcomingItem } from "../api";

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

// AI 全屏：把右栏展开成覆盖整个内容区（用户要求「AI 要能全屏」）
const wide = ref(localStorage.getItem("danlume.aiWide") === "1");
function toggleWide() {
  wide.value = !wide.value;
  localStorage.setItem("danlume.aiWide", wide.value ? "1" : "0");
}
</script>

<template>
  <aside class="dock" :class="{ wide }">
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
        <!-- AI 面板：照旧版 ai_panel.py / ai_api_panel.py 重做（API 服务 / WebUI / 提示词） -->
        <AiPanel />
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
/* ---- AI 里的「对话 / 网页版」子标签 ---- */
.sub-tabs {
  flex: none;
  display: flex;
  gap: 3px;
  padding: 2px;
  border-radius: var(--radius-pill);
  background: var(--card-alt);
  align-self: flex-start;
}
.sub-tab {
  height: 22px;
  padding: 0 10px;
  border: none;
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--muted);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.sub-tab.on {
  background: var(--card);
  color: var(--text);
  font-weight: 600;
}
.web-panel {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.web-tip {
  font-size: 11px;
  color: var(--muted);
  line-height: 1.55;
}
.web-bar {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
}
.web-url {
  flex: 1;
  min-width: 0;
  font-size: 10.5px;
  color: var(--faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.web-btn {
  flex: none;
  height: 22px;
  padding: 0 8px;
  border-radius: 6px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  color: var(--text-soft);
  font-family: inherit;
  font-size: 11px;
  cursor: pointer;
}
.web-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}
.web-btn.primary {
  border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, var(--card));
}
.web-warn {
  font-size: 11px;
  line-height: 1.6;
  color: var(--warning);
  background: color-mix(in srgb, var(--warning) 10%, transparent);
  border-radius: 8px;
  padding: 6px 8px;
}
.web-frame {
  flex: 1;
  min-height: 220px;
  border-radius: 10px;
  overflow: hidden;
  border: 1px solid var(--border-soft);
  background: #fff;
}
.web-iframe {
  width: 100%;
  height: 100%;
  border: 0;
  display: block;
  background: #fff;
}
.web-foot {
  font-size: 10.5px;
  color: var(--faint);
  line-height: 1.6;
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
/* 「本次注入了 N 条记忆」：回答底下的一行小字（memory.rs 注入的回执） */
.mem-note {
  margin-top: 5px;
  padding-top: 4px;
  border-top: 1px dashed var(--border-soft);
  font-size: 10.5px;
  color: var(--muted);
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
