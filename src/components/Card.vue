<script setup lang="ts">
// 对应旧 widgets.Card：圆角 14、白底、1px 边、内边距可调
withDefaults(
  defineProps<{
    radius?: number;
    padding?: number;
    spacing?: number;
    flat?: boolean;
  }>(),
  { radius: 14, padding: 14, spacing: 6, flat: false },
);
</script>

<template>
  <div
    class="card"
    :class="{ flat }"
    :style="{
      borderRadius: radius + 'px',
      padding: padding + 'px',
      gap: spacing + 'px',
    }"
  >
    <slot />
  </div>
</template>

<style scoped>
.card {
  background: var(--card);
  border: 1px solid var(--border-soft);
  display: flex;
  flex-direction: column;
  min-width: 0;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
}
.card.flat {
  box-shadow: none;
}
</style>
