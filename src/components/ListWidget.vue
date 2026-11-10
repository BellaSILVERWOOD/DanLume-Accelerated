<script setup lang="ts">
// 对应旧 dashboard_widgets.ListWidget：带标题的清单卡（即将到来 / 教务信息 / 最近作业）
import WidgetCard from "./WidgetCard.vue";
import MetricLine from "./MetricLine.vue";
import type { GlyphName } from "../icons";

withDefaults(
  defineProps<{
    title: string;
    glyph?: GlyphName | "";
    tone?: string;
    meta?: string;
    hint?: string;
  }>(),
  { glyph: "", tone: "teal", meta: "", hint: "" },
);

export type Row = {
  title: string;
  meta?: string;
  tone?: string;
  metaActive?: boolean;
  link?: boolean;
  onOpen?: () => void;
};
</script>

<template>
  <WidgetCard :title="title" :glyph="glyph" :tone="tone" :meta="meta">
    <slot />
    <p v-if="hint" class="hint">{{ hint }}</p>
  </WidgetCard>
</template>

<style scoped>
.hint {
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--muted);
  line-height: 1.5;
}
</style>
