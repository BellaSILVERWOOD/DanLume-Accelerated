<script setup lang="ts">
// 标题栏：左上 Logo + 「DanLume 日程与工作汇报」，右上 中栏/右栏 开关、深浅切换、
// 邮件状态、最小化/最大化/关闭（旧版是无边框窗口 + 自绘按钮）
import { ref } from "vue";
import IconGlyph from "../components/IconGlyph.vue";
import Pill from "../components/Pill.vue";
import AnnouncementBell from "../components/AnnouncementBell.vue";
import { state, toggleMode } from "../theme";
import {
  windowClose,
  windowMinimize,
  windowToggleMaximize,
} from "../api";

const props = defineProps<{
  middleVisible: boolean;
  dockVisible: boolean;
  mailStatus: string;
}>();
const emit = defineEmits<{
  (e: "toggle-middle"): void;
  (e: "toggle-dock"): void;
}>();

const maximized = ref(false);

async function onMaximize() {
  await windowToggleMaximize();
  maximized.value = !maximized.value;
}
</script>

<template>
  <div class="titlebar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region>
      <span class="logo">
        <IconGlyph name="CALENDAR" :size="14" color="#fff" />
      </span>
      <span class="name">DanLume</span>
      <span class="tagline">日程与工作汇报</span>
    </div>
    <div class="spacer" data-tauri-drag-region />
    <div class="actions">
      <button
        class="chip"
        :class="{ on: middleVisible }"
        @click="emit('toggle-middle')"
        title="显示 / 隐藏中间栏"
      >
        中栏
      </button>
      <button
        class="chip"
        :class="{ on: dockVisible }"
        @click="emit('toggle-dock')"
        title="显示 / 隐藏右栏"
      >
        右栏
      </button>
      <AnnouncementBell />
      <button class="icon-btn" @click="toggleMode" :title="state.dark ? '切到浅色' : '切到深色'">
        <IconGlyph :name="state.dark ? 'SUN' : 'MOON'" :size="14" />
      </button>
      <Pill :text="mailStatus" :font-size="11.5" />
      <span class="divider" />
      <button class="win-btn" @click="windowMinimize" title="最小化">
        <IconGlyph name="MINIMIZE" :size="11" />
      </button>
      <button class="win-btn" @click="onMaximize" :title="maximized ? '还原' : '最大化'">
        <IconGlyph :name="maximized ? 'RESTORE' : 'MAXIMIZE'" :size="11" />
      </button>
      <button class="win-btn close" @click="windowClose" title="关闭">
        <IconGlyph name="CLOSE" :size="11" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.titlebar {
  height: var(--titlebar-h);
  flex: none;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 12px 0 14px;
  border-bottom: 1px solid var(--border-soft);
  background: transparent;
}
.brand {
  display: flex;
  align-items: center;
  gap: 8px;
}
.logo {
  width: 22px;
  height: 22px;
  border-radius: 7px;
  background: linear-gradient(135deg, var(--accent), var(--accent-2));
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.name {
  font-size: 13.5px;
  font-weight: 680;
}
.tagline {
  font-size: 12px;
  color: var(--muted);
}
.spacer {
  flex: 1;
}
.actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.chip {
  height: 24px;
  padding: 0 10px;
  border-radius: var(--radius-pill);
  border: 1px solid var(--border);
  background: transparent;
  color: var(--muted);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.chip.on {
  color: var(--accent);
  border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
  background: color-mix(in srgb, var(--accent) 8%, transparent);
}
.icon-btn,
.win-btn {
  width: 26px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-soft);
  border-radius: 7px;
  cursor: pointer;
}
.icon-btn:hover,
.win-btn:hover {
  background: var(--card-alt);
}
.win-btn.close:hover {
  background: #e81123;
  color: #fff;
}
.divider {
  width: 1px;
  height: 16px;
  background: var(--border-soft);
  margin: 0 2px;
}
/* macOS 风格层（C1）：标题栏更薄、按钮圆角、分隔线用发丝线 */
.titlebar {
  border-bottom: 1px solid var(--hairline, var(--border-soft));
  background: color-mix(in srgb, var(--material-sidebar, transparent) 70%, transparent);
  -webkit-backdrop-filter: var(--material-blur, none);
  backdrop-filter: var(--material-blur, none);
}
.chip {
  border-radius: var(--radius-control, 7px);
  border: 1px solid var(--hairline, transparent);
  transition: background 140ms ease, color 140ms ease;
}
.icon-btn,
.win-btn {
  border-radius: var(--radius-control, 7px);
  transition: background 140ms ease, color 140ms ease;
}
.divider {
  background: var(--hairline, var(--border-soft));
}</style>
