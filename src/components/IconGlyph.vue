<script setup lang="ts">
// 一行图标：就是 Segoe Fluent Icons 里的一个字形
import { computed } from "vue";
import { GLYPH, type GlyphName } from "../icons";

const props = withDefaults(
  defineProps<{
    name: GlyphName | string;
    size?: number | string;
    color?: string;
  }>(),
  { size: 14, color: "" },
);

const char = computed(() => {
  const key = props.name as GlyphName;
  return (GLYPH as Record<string, string>)[key] || props.name;
});
const px = computed(() =>
  typeof props.size === "number" ? `${props.size}px` : props.size,
);
</script>

<template>
  <span
    class="glyph"
    :style="{ fontSize: px, color: color || undefined }"
    aria-hidden="true"
    >{{ char }}</span
  >
</template>

<style scoped>
.glyph {
  font-family: "Segoe Fluent Icons", "Segoe MDL2 Assets", sans-serif;
  font-weight: 400;
  line-height: 1;
  display: inline-block;
  flex: none;
  -webkit-font-smoothing: antialiased;
  user-select: none;
  pointer-events: none;
}
</style>
