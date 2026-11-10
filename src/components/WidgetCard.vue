<script setup lang="ts">
// 对应旧 dashboard_widgets.WidgetCard：小组件卡（圆角 18 + 图标徽标 + 右上小字 + 内容区）
import IconBadge from "./IconBadge.vue";
import type { GlyphName } from "../icons";

withDefaults(
  defineProps<{
    title: string;
    glyph?: GlyphName | "";
    tone?: string;
    meta?: string;
    minHeight?: number;
  }>(),
  { glyph: "", tone: "teal", meta: "", minHeight: 0 },
);
</script>

<template>
  <div
    class="widget"
    :style="{ minHeight: minHeight ? minHeight + 'px' : undefined }"
  >
    <header>
      <IconBadge :glyph="glyph" :tone="tone" :size="26" />
      <span class="title">{{ title }}</span>
      <span class="spacer" />
      <span class="meta">{{ meta }}</span>
    </header>
    <div class="content"><slot /></div>
  </div>
</template>

<style scoped>
.widget {
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-widget);
  padding: 14px 15px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.045);
}
header {
  display: flex;
  align-items: center;
  gap: 8px;
}
.title {
  font-size: 13.5px;
  font-weight: 600;
  color: var(--text-soft);
  white-space: nowrap;
}
.spacer {
  flex: 1;
}
.meta {
  font-size: 12px;
  color: var(--muted);
  white-space: nowrap;
}
.content {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-height: 0;
}
</style>
