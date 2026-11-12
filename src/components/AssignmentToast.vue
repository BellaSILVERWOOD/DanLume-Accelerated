<script setup lang="ts">
// 作业到期提醒：后台（Rust）到点发 `elearn:due` 事件，这里弹卡片。
//
// 为什么提醒逻辑在后端：系统通知也是后端发的 —— 窗口最小化到托盘时前端可能没在跑，
// 但「今天哪几份作业要交」这个判断只在后端做一次，前端只负责把结果摆出来。
import { onMounted, onUnmounted, ref } from "vue";
import SoftButton from "../components/SoftButton.vue";
import IconBadge from "../components/IconBadge.vue";
import { elearnDueReminders, onElearnDue, openUrl, type DueAssignment } from "../api";

import type { DueAssignment as DueItem } from "../api";

const items = ref<DueItem[]>([]);
let unlisten: (() => void) | null = null;

function push(rows: DueItem[]) {
  for (const row of rows) {
    if (items.value.some((one) => one.key === row.key)) continue;
    items.value.push(row);
  }
  // 最多同时摆 3 条，多了只留最后三条
  if (items.value.length > 3) items.value = items.value.slice(-3);
}

async function open(item: DueItem) {
  if (item.url) void openUrl(item.url);
  dismiss(item);
}

function dismiss(item: DueItem) {
  items.value = items.value.filter((one) => one.key !== item.key);
}

onMounted(async () => {
  // 先问一次「今天有没有要提的」——后台线程 25 秒后才第一次跑，
  // 这中间用户可能已经打开了程序
  try {
    const result = await elearnDueReminders(false);
    if (result.items?.length) push(result.items);
  } catch {
    // 读不到就算了，等事件
  }
  try {
    unlisten = await onElearnDue((payload) => {
      if (payload?.items?.length) push(payload.items);
    });
  } catch {
    /* 浏览器预览里没有事件系统 */
  }
});

onUnmounted(() => {
  unlisten?.();
  unlisten = null;
});
</script>

<template>
  <div class="due-toasts">
    <div v-for="item in items" :key="item.key" class="due-toast">
      <IconBadge glyph="CLOCK" tone="rose" :size="26" />
      <div class="due-main">
        <b>{{ item.left_text || "快到截止时间了" }}</b>
        <span class="due-text">{{ item.text }}</span>
      </div>
      <div class="due-actions">
        <SoftButton v-if="item.url" :height="26" :font-size="11.5" @click="open(item)">打开</SoftButton>
        <button class="due-x" title="知道了" @click="dismiss(item)">知道了</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 放在右上角（标题栏下面）：右下角是「作业截止」卡片和 ReminderToast 的地盘，
   挡在那儿会把「同步 eLearning」按钮压住（2026-11-12 实测点了没反应） */
.due-toasts {
  position: fixed;
  right: 18px;
  top: 56px;
  z-index: 60;
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 320px;
  max-width: 340px;
}
.due-toast {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 10px 11px;
  border-radius: 12px;
  background: var(--card);
  border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--border-soft));
  box-shadow: var(--shadow-pop, 0 8px 24px rgba(0, 0, 0, 0.18));
}
.due-main {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.due-main b {
  font-size: 12.5px;
  color: var(--text);
}
.due-text {
  font-size: 11.5px;
  color: var(--muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.due-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 6px;
}
.due-x {
  border: none;
  background: transparent;
  color: var(--muted);
  font-size: 11.5px;
  cursor: pointer;
  padding: 2px 4px;
  border-radius: 6px;
}
.due-x:hover {
  background: var(--card-hover);
  color: var(--text-soft);
}
</style>
