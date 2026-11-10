<script setup lang="ts">
// 对应旧 widgets.SoftButton：primary / ghost / subtle 三种，可带图标、可设高度
import IconGlyph from "./IconGlyph.vue";
import type { GlyphName } from "../icons";

withDefaults(
  defineProps<{
    variant?: "primary" | "ghost" | "subtle" | "danger";
    glyph?: GlyphName | "";
    height?: number;
    minWidth?: number;
    fontSize?: number;
    disabled?: boolean;
    active?: boolean;
  }>(),
  {
    variant: "ghost",
    glyph: "",
    height: 34,
    minWidth: 0,
    fontSize: 13,
    disabled: false,
    active: false,
  },
);

defineEmits<{ (e: "click"): void }>();
</script>

<template>
  <button
    class="soft"
    :class="[variant, { active }]"
    :style="{
      height: height + 'px',
      minWidth: minWidth ? minWidth + 'px' : undefined,
      fontSize: fontSize + 'px',
    }"
    :disabled="disabled"
    type="button"
    @click="$emit('click')"
  >
    <IconGlyph v-if="glyph" :name="glyph" :size="fontSize + 2" />
    <span class="label"><slot /></span>
  </button>
</template>

<style scoped>
.soft {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 0 14px;
  border-radius: var(--radius-btn);
  border: 1px solid transparent;
  font-family: inherit;
  font-weight: 550;
  cursor: pointer;
  white-space: nowrap;
  transition:
    background 0.12s ease,
    border-color 0.12s ease,
    color 0.12s ease;
  user-select: none;
}
.label {
  line-height: 1;
}
/* 主按钮：强调色实底 */
.soft.primary {
  background: var(--accent);
  color: #fff;
}
.soft.primary:hover {
  background: var(--accent-deep);
}
/* 幽灵按钮：卡片底 + 细边 */
.soft.ghost {
  background: var(--card);
  color: var(--text);
  border-color: var(--border);
}
.soft.ghost:hover {
  background: var(--card-hover);
  border-color: var(--input-hover-border);
}
/* 轻按钮：没有边，只有底色 */
.soft.subtle {
  background: var(--card-alt);
  color: var(--text-soft);
}
.soft.subtle:hover {
  background: var(--card-hover);
  color: var(--text);
}
.soft.danger {
  background: transparent;
  color: var(--danger);
  border-color: var(--border);
}
.soft.danger:hover {
  background: color-mix(in srgb, var(--danger) 10%, transparent);
}
.soft.active {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, var(--card));
}
.soft:disabled {
  opacity: 0.45;
  cursor: default;
}
.soft:active:not(:disabled) {
  transform: translateY(0.5px);
}
/* macOS 风格层：轻微内高光 + hover 只提亮不位移（Ventura 的按钮不跳） */
.soft {
  box-shadow: var(--inner-highlight, none), 0 1px 1px rgba(0, 0, 0, 0.04);
  transition: background 140ms ease, border-color 140ms ease, box-shadow 140ms ease;
}
.soft:hover:not(:disabled) {
  box-shadow: var(--shadow-card, none);
}
.soft.primary {
  border: 1px solid color-mix(in srgb, var(--accent) 70%, #000 12%);
}</style>
