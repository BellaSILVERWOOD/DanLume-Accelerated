<script setup lang="ts">
// 对应旧 dashboard_widgets.MetricLine：彩点 + 主文字 + 右边小字（可高亮成「进行中」）
import IconGlyph from "./IconGlyph.vue";
import { state as themeState } from "../theme";
import type { GlyphName } from "../icons";

withDefaults(
  defineProps<{
    title: string;
    meta?: string;
    tone?: string;
    metaActive?: boolean;
    link?: boolean;
    glyph?: GlyphName | "";
  }>(),
  { meta: "", tone: "", metaActive: false, link: false, glyph: "" },
);

const TINTS: Record<string, [string, string]> = {
  violet: ["#F3EDFE", "#6C3FA6"],
  teal: ["#E2F5F7", "#00707F"],
  amber: ["#FDF1DF", "#8A5300"],
  rose: ["#FEE9EE", "#B1123C"],
  sky: ["#E4F0FF", "#0058B8"],
};
const TINTS_DARK: Record<string, [string, string]> = {
  violet: ["#352B47", "#D7B8FF"],
  teal: ["#103235", "#7FE0DD"],
  amber: ["#3A2B15", "#FFC563"],
  rose: ["#3C1F2B", "#FF8FAE"],
  sky: ["#10283F", "#7FBEFF"],
};

function dotColor(tone: string): string {
  if (!tone) return "transparent";
  const dark = themeState.dark;
  const table = dark ? TINTS_DARK : TINTS;
  return (table[tone] || table.violet)[1];
}
</script>

<template>
  <div class="metric">
    <span v-if="tone" class="dot" :style="{ color: dotColor(tone) }">●</span>
    <IconGlyph v-else-if="glyph" :name="glyph" :size="12" color="var(--muted)" />
    <span class="text" :class="{ link }">{{ title }}</span>
    <span v-if="meta || $slots.meta" class="meta" :class="{ active: metaActive }">
      <slot name="meta">{{ meta }}</slot>
    </span>
  </div>
</template>

<style scoped>
.metric {
  display: flex;
  align-items: flex-start;
  gap: 7px;
  padding: 2px 0;
  min-width: 0;
}
.dot {
  font-size: 7px;
  line-height: 1.6;
  flex: none;
}
.text {
  flex: 1;
  min-width: 0;
  font-size: 15px;
  color: var(--text);
  overflow-wrap: anywhere;
}
.text.link {
  color: var(--accent);
  text-decoration: underline;
  text-underline-offset: 2px;
  cursor: pointer;
}
.meta {
  flex: none;
  font-size: 12.5px;
  color: var(--muted);
  text-align: right;
  white-space: nowrap;
  padding-top: 2px;
}
.meta.active {
  color: var(--accent);
  font-weight: 650;
}
</style>
