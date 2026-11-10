<script setup lang="ts">
// 对应旧 dashboard_widgets.IconBadge：小圆角方块 + 中间一个图标（跟主题色晕染）
import { computed } from "vue";
import IconGlyph from "./IconGlyph.vue";
import type { GlyphName } from "../icons";
import { state as themeState } from "../theme";

const props = withDefaults(
  defineProps<{
    glyph?: GlyphName | "";
    tone?: string;
    size?: number;
  }>(),
  { glyph: "", tone: "teal", size: 26 },
);

const SOFT: Record<string, string> = {
  violet: "#F3EDFE",
  teal: "#E2F5F7",
  amber: "#FDF1DF",
  rose: "#FEE9EE",
  sky: "#E4F0FF",
};
const SOFT_DARK: Record<string, string> = {
  violet: "#352B47",
  teal: "#103235",
  amber: "#3A2B15",
  rose: "#3C1F2B",
  sky: "#10283F",
};
const INK: Record<string, string> = {
  violet: "#6C3FA6",
  teal: "#00707F",
  amber: "#8A5300",
  rose: "#B1123C",
  sky: "#0058B8",
};
const INK_DARK: Record<string, string> = {
  violet: "#D7B8FF",
  teal: "#7FE0DD",
  amber: "#FFC563",
  rose: "#FF8FAE",
  sky: "#7FBEFF",
};

const dark = () => themeState.dark;
const background = computed(
  () => (dark() ? SOFT_DARK : SOFT)[props.tone] || "#E2F5F7",
);
const ink = computed(() => (dark() ? INK_DARK : INK)[props.tone] || "#00707F");
const radius = computed(() => Math.max(6, Math.round(props.size * 0.32)));
const glyphSize = computed(() => Math.max(9, Math.round(props.size * 0.5)));
</script>

<template>
  <span
    class="badge"
    :style="{
      width: size + 'px',
      height: size + 'px',
      borderRadius: radius + 'px',
      background,
      color: ink,
    }"
  >
    <IconGlyph v-if="glyph" :name="glyph" :size="glyphSize" />
  </span>
</template>

<style scoped>
.badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;
}
</style>
