<script setup lang="ts">
// 保持既有 IconGlyph 接口不变：只替换底层 Segoe 字体为跨平台 SVG。
import { computed } from "vue";
import { CircleHelp } from "lucide-vue-next";
import { GLYPH, type GlyphName } from "../icons";

const props = withDefaults(
  defineProps<{
    name: GlyphName | string;
    size?: number | string;
    color?: string;
  }>(),
  { size: 14, color: "" },
);
const component = computed(() => GLYPH[props.name as GlyphName] ?? CircleHelp);
const px = computed(() =>
  typeof props.size === "number" ? `${props.size}px` : props.size,
);
</script>

<template>
  <component
    :is="component"
    class="glyph"
    :size="Number(props.size) || 14"
    :color="color || 'currentColor'"
    :stroke-width="1.9"
    aria-hidden="true"
  />
</template>

<style scoped>
.glyph {
  display: inline-block;
  flex: none;
  vertical-align: middle;
  user-select: none;
  pointer-events: none;
}
</style>
