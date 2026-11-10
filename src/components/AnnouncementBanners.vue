<script setup lang="ts">
// 内容区顶部的公告横幅：只显示重要级别（warning / critical）的服务器公告，点 × 可以关掉。
// 关掉的只记在内存里（重启再提醒一次，避免「重要的那条永远被关掉看不见」）。
import { computed, onMounted, onUnmounted, ref } from "vue";
import IconGlyph from "./IconGlyph.vue";
import {
  accountAnnouncements,
  onAnnouncement,
  type ServerAnnouncement,
} from "../api";

const items = ref<ServerAnnouncement[]>([]);
const dismissed = ref<number[]>([]);
let timer: number | null = null;
let off: (() => void) | null = null;

/** 只有 warning / critical 才配横幅；info 走铃铛里的列表 */
const banners = computed(() =>
  items.value
    .filter(
      (one) =>
        (one.level === "warning" || one.level === "critical") &&
        !dismissed.value.includes(one.id),
    )
    .slice(0, 3),
);

async function load() {
  try {
    const result = await accountAnnouncements(40);
    items.value = result.items || [];
  } catch {
    // 服务器/本地库都读不到就不显示横幅，别挡内容
  }
}

function incoming(item: ServerAnnouncement) {
  if (!item || !item.id) return;
  if (items.value.some((one) => one.id === item.id)) return;
  items.value = [item, ...items.value];
}

function close(id: number) {
  dismissed.value = [...dismissed.value, id];
}

onMounted(async () => {
  await load();
  off = await onAnnouncement(incoming);
  timer = window.setInterval(load, 60000);
});

onUnmounted(() => {
  if (timer !== null) window.clearInterval(timer);
  if (off) off();
});
</script>

<template>
  <div v-if="banners.length" class="banners">
    <div v-for="item in banners" :key="item.id" class="banner" :class="item.level">
      <IconGlyph :name="item.level === 'critical' ? 'WARNING' : 'INFO'" :size="14" />
      <div class="text">
        <span class="title">{{ item.title || "服务器公告" }}</span>
        <span v-if="item.body" class="body">{{ item.body }}</span>
      </div>
      <button class="close" title="关闭这条横幅" @click="close(item.id)">×</button>
    </div>
  </div>
</template>

<style scoped>
.banners {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 14px 0;
}
.banner {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px solid color-mix(in srgb, var(--warning) 40%, var(--border-soft));
  background: color-mix(in srgb, var(--warning) 10%, var(--card));
  color: var(--text);
  font-size: 12.5px;
}
.banner.critical {
  border-color: color-mix(in srgb, var(--danger) 45%, var(--border-soft));
  background: color-mix(in srgb, var(--danger) 10%, var(--card));
}
.banner.critical :deep(.glyph) {
  color: var(--danger);
}
.banner.warning :deep(.glyph) {
  color: var(--warning);
}
.text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.title {
  font-weight: 600;
  overflow-wrap: anywhere;
}
.body {
  color: var(--text-soft);
  line-height: 1.6;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.close {
  flex: none;
  border: none;
  background: transparent;
  color: var(--muted);
  font-size: 15px;
  line-height: 1;
  padding: 0 2px;
  cursor: pointer;
}
.close:hover {
  color: var(--text);
}
</style>
