<script setup lang="ts">
// 对应旧 dashboard_widgets.HeroWidget：「今天 / 天气」那种大号数值 + 右侧两行说明
import IconBadge from "./IconBadge.vue";
import type { GlyphName } from "../icons";

withDefaults(
  defineProps<{
    title: string;
    glyph?: GlyphName | "";
    tone?: string;
    meta?: string;
    value: string;
    sub?: string;
    hint?: string;
  }>(),
  { glyph: "", tone: "teal", meta: "", sub: "", hint: "" },
);
</script>

<template>
  <div class="hero">
    <header>
      <IconBadge :glyph="glyph" :tone="tone" :size="26" />
      <span class="title">{{ title }}</span>
      <span class="spacer" />
      <span class="meta">{{ meta }}</span>
    </header>
    <div class="body">
      <span class="value">{{ value }}</span>
      <div class="column">
        <span v-if="sub" class="sub">{{ sub }}</span>
        <span v-if="hint" class="hint">{{ hint }}</span>
        <slot name="hint" />
      </div>
    </div>
    <slot />
  </div>
</template>

<style scoped>
.hero {
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-widget);
  padding: 14px 15px;
  display: flex;
  flex-direction: column;
  gap: 8px;
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
}
.spacer {
  flex: 1;
}
.meta {
  font-size: 12px;
  color: var(--muted);
}
.body {
  display: flex;
  align-items: flex-end;
  gap: 12px;
  min-width: 0;
}
.value {
  font-size: 40px;
  font-weight: 700;
  line-height: 1;
  letter-spacing: -0.5px;
}
.column {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  padding-bottom: 3px;
}
.sub {
  font-size: 15px;
  color: var(--text);
}
.hint {
  font-size: 12px;
  color: var(--muted);
}
</style>
