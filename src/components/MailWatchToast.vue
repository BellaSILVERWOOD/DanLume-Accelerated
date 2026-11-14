<script setup lang="ts">
// 盯邮箱（J2-6）：后台盯到「指定发件人的新邮件里有要做的事」时，弹这张卡让用户确认。
//
// 与旧版一致：确认后才写日程，写成功才把这封邮件记成「已处理」；点「忽略」也记已处理。
import { onMounted, onUnmounted, ref } from "vue";
import SoftButton from "../components/SoftButton.vue";
import IconBadge from "../components/IconBadge.vue";
import {
  mailAddEvents,
  mailWatchCheck,
  mailWatchConsume,
  onMailWatch,
  type MailWatchItem,
} from "../api";

const items = ref<MailWatchItem[]>([]);
const busy = ref(false);
const note = ref("");
let unlisten: (() => void) | null = null;

function push(rows: MailWatchItem[]) {
  for (const row of rows) {
    const key = `${row.uid || ""}|${row.title}|${row.date}`;
    if (items.value.some((one) => `${one.uid || ""}|${one.title}|${one.date}` === key)) continue;
    items.value.push(row);
  }
  if (items.value.length > 6) items.value = items.value.slice(-6);
}

/** 加入日程（只加这张卡里列出来的） */
async function addAll() {
  if (busy.value || !items.value.length) return;
  busy.value = true;
  note.value = "正在加进日程…";
  try {
    const result = await mailAddEvents(items.value);
    note.value = result.ok
      ? `已加进日程 ${result.added} 条`
      : `加失败了：${result.error || "未知原因"}`;
    if (result.ok) {
      const uids = items.value
        .map((one) => one.uid || "")
        .filter((one) => one.trim() !== "");
      if (uids.length) await mailWatchConsume(uids);
      items.value = [];
    }
  } catch (err) {
    note.value = `加失败了：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

/** 忽略：记成已处理，别再提示 */
async function ignore() {
  const uids = items.value.map((one) => one.uid || "").filter((one) => one.trim() !== "");
  items.value = [];
  note.value = "";
  try {
    if (uids.length) await mailWatchConsume(uids);
  } catch {
    /* 忽略失败无所谓，下次还会提示 */
  }
}

async function checkNow() {
  if (busy.value) return;
  busy.value = true;
  note.value = "正在看邮箱…";
  try {
    const result = await mailWatchCheck();
    push(result.items || []);
    if (!result.items?.length) {
      const errors = (result.errors || []).join("；");
      note.value = errors || "邮箱里没有需要加日程的事";
    } else {
      note.value = `从 ${result.mails?.length || 0} 封邮件里抽到 ${result.items.length} 条`;
    }
  } catch (err) {
    note.value = `看邮箱失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  try {
    unlisten = await onMailWatch((payload) => {
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

defineExpose({ checkNow });
</script>

<template>
  <div v-if="items.length || note" class="watch-card">
    <div class="watch-head">
      <IconBadge glyph="MAIL" tone="sky" :size="24" />
      <b>邮箱里可能要加日程</b>
      <span class="spacer" />
      <button class="watch-x" title="收起" @click="ignore">收起</button>
    </div>
    <div v-if="note" class="watch-note">{{ note }}</div>
    <div v-for="(item, index) in items" :key="index" class="watch-row">
      <span class="watch-date">{{ item.date }} {{ item.start_time || "" }}</span>
      <span class="watch-title">{{ item.title }}</span>
      <span v-if="item.from" class="watch-from" :title="item.from">{{ item.from }}</span>
    </div>
    <div class="watch-foot">
      <SoftButton :height="26" :font-size="11.5" :disabled="busy" @click="ignore">都不是</SoftButton>
      <SoftButton variant="primary" :height="26" :font-size="11.5" :disabled="busy || !items.length" @click="addAll">
        {{ busy ? "处理中…" : "加进日程" }}
      </SoftButton>
    </div>
  </div>
</template>

<style scoped>
/* 右上角，和作业提醒同一列，但更靠下一点（两个一起出现时不重叠） */
.watch-card {
  position: fixed;
  right: 18px;
  top: 56px;
  width: 340px;
  z-index: 61;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--card);
  border: 1px solid color-mix(in srgb, var(--accent) 26%, var(--border-soft));
  box-shadow: var(--shadow-pop, 0 8px 24px rgba(0, 0, 0, 0.18));
}
.watch-head {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 12.5px;
}
.watch-x {
  border: none;
  background: transparent;
  color: var(--muted);
  font-size: 11px;
  cursor: pointer;
}
.watch-note {
  font-size: 11px;
  color: var(--muted);
}
.watch-row {
  display: flex;
  align-items: baseline;
  gap: 6px;
  font-size: 11.5px;
  min-width: 0;
}
.watch-date {
  flex: 0 0 auto;
  color: var(--accent);
  font-weight: 600;
}
.watch-title {
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.watch-from {
  flex: 0 0 auto;
  max-width: 90px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--faint);
  font-size: 10.5px;
}
.watch-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 2px;
}
</style>
