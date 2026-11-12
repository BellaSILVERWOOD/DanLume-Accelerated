<script setup lang="ts">
// 对应旧 widgets.Pill：一个小圆角标签（「11 门课 · 43 个作业」这种）
withDefaults(
  defineProps<{
    text?: string;
    color?: string;
    background?: string;
    fontSize?: number;
  }>(),
  { text: "", color: "var(--muted)", background: "var(--card-alt)", fontSize: 12 },
);
</script>

<template>
  <span
    class="pill"
    :style="{
      color,
      background,
      fontSize: fontSize + 'px',
    }"
    >{{ text }}<slot /></span
  >
</template>

<style scoped>
.pill {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 9px;
  border-radius: var(--radius-pill);
  font-weight: 550;
  line-height: 1.35;
  white-space: nowrap;
}
/* macOS 风格层：标签更像系统胶囊（更小、更淡的描边）；不要内高光（见 SoftButton 的说明） */
.pill {
  border: 1px solid var(--hairline, transparent);
  box-shadow: none;
}</style>
