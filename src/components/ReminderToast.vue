<script setup lang="ts">
// 桌面提醒：每 30 秒问一次 Rust「有没有到点的日程」，右下角弹卡片（旧版是同样的行为）
import { onMounted, onUnmounted, ref } from "vue";
import SoftButton from "../components/SoftButton.vue";
import IconBadge from "../components/IconBadge.vue";
import {
  dueReminders,
  eventSetDone,
  markNotified,
  settingsAll,
  snoozeEvent,
  type Reminder,
} from "../api";

const shown = ref<Reminder[]>([]);
let timer: number | undefined;
// 旧版 reminder_sound：默认开；关掉就不响
let soundOn = true;

/** 用 WebAudio 叮一声（不依赖音频文件，避免打包体积） */
function beep() {
  if (!soundOn) return;
  try {
    const Ctx =
      window.AudioContext ||
      (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Ctx) return;
    const ctx = new Ctx();
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = "sine";
    osc.frequency.value = 880;
    gain.gain.setValueAtTime(0.0001, ctx.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.16, ctx.currentTime + 0.02);
    gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.35);
    osc.connect(gain).connect(ctx.destination);
    osc.start();
    osc.stop(ctx.currentTime + 0.36);
    osc.onended = () => ctx.close().catch(() => undefined);
  } catch {
    // 放不出声就算了，别影响提醒本身
  }
}

async function poll() {
  try {
    const rows = await dueReminders();
    let fresh = 0;
    for (const row of rows) {
      if (shown.value.some((one) => one.id === row.id)) continue;
      shown.value.push(row);
      fresh += 1;
    }
    if (fresh > 0) beep();
    // 最多同时显示 3 个
    if (shown.value.length > 3) shown.value = shown.value.slice(-3);
  } catch {
    // 读不到就算了，下一轮再试
  }
}

async function done(item: Reminder) {
  // 传 item.date：重复日程只把「这一次」标成完成，不会带成整条系列
  await eventSetDone(item.id, true, item.date);
  dismiss(item);
}

async function snooze(item: Reminder) {
  // 只推迟，不记「已提醒」：10 分钟后还要再弹一次
  await snoozeEvent(item.id, 10, item.date);
  shown.value = shown.value.filter((one) => one.id !== item.id);
}

async function dismiss(item: Reminder) {
  shown.value = shown.value.filter((one) => one.id !== item.id);
  try {
    await markNotified(item.id, item.date);
  } catch {
    // 忽略
  }
}

function when(item: Reminder): string {
  return `${item.date.slice(5)} ${item.start_time}–${item.end_time}`;
}

onMounted(async () => {
  // 旧版 reminder_sound：读一次设置决定要不要响（默认响）
  try {
    const all = await settingsAll();
    soundOn = (all.values || {})["reminder_sound"] !== "0";
  } catch {
    soundOn = true;
  }
  poll();
  timer = window.setInterval(poll, 30000);
});
onUnmounted(() => {
  if (timer) window.clearInterval(timer);
});
</script>

<template>
  <div class="toasts">
    <div v-for="item in shown" :key="item.id" class="toast">
      <div class="head">
        <IconBadge glyph="BELL" tone="rose" :size="26" />
        <div class="text">
          <div class="title">{{ item.title }}</div>
          <div class="meta">{{ when(item) }} · 提醒 {{ item.remind_at.slice(11, 16) }}</div>
        </div>
        <SoftButton :height="22" :font-size="11" @click="dismiss(item)">×</SoftButton>
      </div>
      <div class="actions">
        <!-- 三个选项：知道了（只关掉）/ 标记完成 / 稍后 10 分钟提醒 -->
        <SoftButton :height="26" :font-size="11.5" @click="dismiss(item)">我知道了</SoftButton>
        <SoftButton variant="primary" :height="26" :font-size="11.5" @click="done(item)">
          标记完成
        </SoftButton>
        <SoftButton :height="26" :font-size="11.5" @click="snooze(item)">稍后 10 分钟提醒</SoftButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.toasts {
  position: fixed;
  right: 14px;
  bottom: 14px;
  display: flex;
  flex-direction: column-reverse;
  gap: 8px;
  z-index: 60;
}
.toast {
  width: 372px;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 14px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.22);
}
.head {
  display: flex;
  align-items: flex-start;
  gap: 8px;
}
.text {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.title {
  font-size: 13.5px;
  font-weight: 620;
  color: var(--text);
  overflow-wrap: anywhere;
}
.meta {
  font-size: 11.5px;
  color: var(--muted);
}
.actions {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
  justify-content: flex-end;
}
/* 三个按钮一行放得下：字号略小一点，别让「稍后 10 分钟提醒」换行 */
.actions :deep(button) {
  font-size: 11px;
  white-space: nowrap;
}
</style>
