<script setup lang="ts">
// 左导航：与旧版 main_window._nav_items 完全一致的 13 项 + 快捷操作 + 今天卡
import IconGlyph from "../components/IconGlyph.vue";
import SoftButton from "../components/SoftButton.vue";
import type { GlyphName } from "../icons";

export type PageKey =
  | "dashboard"
  | "calendar"
  | "mail"
  | "notices"
  | "memos"
  | "homework"
  | "ehall"
  | "classroom"
  | "docs"
  | "lit"
  | "latex"
  | "student"
  | "settings";

const props = defineProps<{
  current: PageKey;
  /** 右栏停在哪一页（AI / 文献 / 日程）——用来把左侧对应项也点亮 */
  dockTab?: "日程" | "AI" | "文献";
  dockVisible?: boolean;
  /** 中栏（页面内容）是否显示：关掉时左侧**不应该**再高亮任何页面 */
  middleVisible?: boolean;
  today: { day: string; week: string; month: string; done: number; total: number };
}>();
const emit = defineEmits<{
  (e: "navigate", key: PageKey): void;
  (e: "ai"): void;
  (e: "new-event"): void;
  (e: "new-mail"): void;
  (e: "focus"): void;
  (e: "mascot"): void;
}>();

const NAV: { key: PageKey | "ai"; label: string; glyph: GlyphName }[] = [
  { key: "dashboard", label: "仪表板", glyph: "INFO" },
  { key: "calendar", label: "日历", glyph: "CALENDAR" },
  { key: "mail", label: "邮箱", glyph: "MAIL" },
  { key: "notices", label: "通知", glyph: "BELL" },
  { key: "memos", label: "备忘", glyph: "DOCUMENT" },
  { key: "homework", label: "作业", glyph: "DOWNLOAD" },
  { key: "ehall", label: "eHall", glyph: "FOLDER" },
  { key: "classroom", label: "空教室", glyph: "CALENDAR" },
  { key: "docs", label: "文档", glyph: "DOCUMENT" },
  { key: "lit", label: "文献库", glyph: "FOLDER" },
  { key: "latex", label: "公式", glyph: "EDIT" },
  { key: "student", label: "学生工作", glyph: "CONTACT" },
  { key: "ai", label: "AI", glyph: "INFO" },
  { key: "settings", label: "设置", glyph: "SETTINGS" },
];

function go(item: { key: PageKey | "ai" }) {
  if (item.key === "ai") emit("ai");
  else emit("navigate", item.key);
}

/** AI 项不是页面：右栏打开且停在 AI 标签时算「选中」 */
function isActive(item: { key: PageKey | "ai" }): boolean {
  if (item.key === "ai") return Boolean(props.dockVisible) && props.dockTab === "AI";
  // 中栏关掉时页面根本没在显示，左侧就不该硬点着一项
  // （用户 2026-11-11：同时开过页面和 AI 卡之后，把中栏一关，那一项还亮着）
  if (props.middleVisible === false) return false;
  return props.current === item.key;
}
</script>

<template>
  <aside class="sidebar">
    <div class="section">导航</div>
    <nav>
      <button
        v-for="item in NAV"
        :key="item.key"
        class="nav-item"
        :class="{ active: isActive(item) }"
        @click="go(item)"
      >
        <IconGlyph :name="item.glyph" :size="14" />
        <span>{{ item.label }}</span>
      </button>
    </nav>

    <div class="section">快捷操作</div>
    <div class="quick">
      <SoftButton variant="primary" glyph="ADD" :height="38" @click="emit('new-event')">
        新建日程
      </SoftButton>
      <SoftButton glyph="MAIL" :height="38" @click="emit('new-mail')">新邮件</SoftButton>
      <SoftButton glyph="CLOCK" :height="38" @click="emit('focus')">专注模式</SoftButton>
      <!-- T2 看板娘悬浮球：桌面置顶小窗，把文件拖进去就直接发给 AI -->
      <SoftButton glyph="INFO" :height="38" @click="emit('mascot')">看板娘</SoftButton>
    </div>

    <div class="spacer" />
    <div class="today">
      <div class="line">
        <span class="day">{{ today.day }}</span>
        <span class="meta">
          <span class="month">{{ today.month }}</span>
          <span class="week">{{ today.week }}</span>
        </span>
      </div>
      <div class="foot">{{ today.total }} 个日程 · {{ today.done }} 已完成</div>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  width: var(--sidebar-w);
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 10px 12px;
  background: var(--sidebar-bg);
  border-right: 1px solid var(--border-soft);
  min-height: 0;
  overflow-y: auto;
}
.section {
  font-size: 11.5px;
  font-weight: 600;
  color: var(--faint);
  padding: 6px 8px 2px;
  letter-spacing: 0.3px;
}
nav {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 34px;
  padding: 0 10px;
  border: none;
  border-radius: 9px;
  background: transparent;
  color: var(--text-soft);
  font-family: inherit;
  font-size: 13.5px;
  text-align: left;
  cursor: pointer;
}
.nav-item:hover {
  background: var(--nav-hover-bg);
}
.nav-item.active {
  background: var(--nav-active-bg);
  color: var(--text);
  font-weight: 600;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.07);
}
.quick {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 0 2px;
}
.spacer {
  flex: 1;
  min-height: 12px;
}
.today {
  border-radius: var(--radius-card);
  background: var(--card);
  border: 1px solid var(--border-soft);
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.line {
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.day {
  font-size: 26px;
  font-weight: 700;
  line-height: 1;
}
.meta {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.month {
  font-size: 12px;
  color: var(--text-soft);
}
.week {
  font-size: 11.5px;
  color: var(--muted);
}
.foot {
  font-size: 11.5px;
  color: var(--muted);
}
/* macOS 风格层（C1）：侧栏用 Source List 的取法——小节标题变淡变小、选中态是一块圆角底 */
.sidebar {
  padding-top: 4px;
}
.section {
  text-transform: uppercase;
  letter-spacing: 0.6px;
  font-size: 10.5px;
  opacity: 0.7;
  padding-left: 12px;
}
.nav-item {
  border-radius: var(--radius-control, 7px);
  margin: 1px 8px;
  padding: 5px 10px;
}
.nav-item:hover {
  background: color-mix(in srgb, var(--muted) 12%, transparent);
}
.nav-item.active {
  background: color-mix(in srgb, var(--accent) 15%, transparent);
  color: var(--accent);
  font-weight: 600;
}
.today {
  border-radius: var(--radius-card, 12px);
  border: 1px solid var(--hairline, transparent);
  box-shadow: var(--shadow-card, none);
}</style>
