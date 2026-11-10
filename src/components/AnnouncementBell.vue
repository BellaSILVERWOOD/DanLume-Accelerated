<script setup lang="ts">
// 标题栏右上角的「服务器公告」铃铛：未读徽标 + 点开一个弹层看列表。
//
// 消息来源两路（互为兜底）：
//   1. Rust 侧 `danlume://announcement` 事件（notify.rs 长轮询到新公告时推）；
//   2. 每 30 秒自己拉一次本地库（account_announcements）——万一这个环境没开
//      `core:event:listen` 权限，也照样能在几十秒内看到新公告。
import { computed, onMounted, onUnmounted, ref } from "vue";
import IconGlyph from "./IconGlyph.vue";
import {
  accountAnnouncements,
  accountMarkAnnouncementsRead,
  onAnnouncement,
  type ServerAnnouncement,
} from "../api";

const items = ref<ServerAnnouncement[]>([]);
const unread = ref(0);
const open = ref(false);
const note = ref("");
let timer: number | null = null;
let off: (() => void) | null = null;

const badge = computed(() => (unread.value > 99 ? "99+" : String(unread.value)));

async function load() {
  try {
    const result = await accountAnnouncements(30);
    items.value = result.items || [];
    unread.value = result.unread || 0;
  } catch (err) {
    note.value = String(err);
  }
}

/** 事件推来一条新公告：去重后插到最前面，并 +1 未读 */
function incoming(item: ServerAnnouncement) {
  if (!item || !item.id) return;
  if (items.value.some((one) => one.id === item.id)) return;
  items.value = [item, ...items.value].slice(0, 30);
  unread.value += 1;
}

async function toggle() {
  open.value = !open.value;
  if (!open.value) return;
  await load();
  if (unread.value > 0) {
    try {
      await accountMarkAnnouncementsRead();
      unread.value = 0;
    } catch (err) {
      note.value = String(err);
    }
  }
}

onMounted(async () => {
  await load();
  off = await onAnnouncement(incoming);
  timer = window.setInterval(load, 30000);
});

onUnmounted(() => {
  if (timer !== null) window.clearInterval(timer);
  if (off) off();
});

function levelText(level: string): string {
  if (level === "critical") return "紧急";
  if (level === "warning") return "重要";
  return "公告";
}
</script>

<template>
  <div class="bell-wrap">
    <button class="bell" :class="{ on: open }" title="服务器公告" @click="toggle">
      <IconGlyph name="BELL" :size="14" />
      <span v-if="unread > 0" class="badge">{{ badge }}</span>
    </button>
    <div v-if="open" class="panel">
      <div class="head">
        <span class="title">服务器公告</span>
        <span class="spacer" />
        <button class="link" @click="load">刷新</button>
        <button class="link" @click="open = false">关闭</button>
      </div>
      <div v-if="!items.length" class="empty">
        还没有收到服务器公告（登录后会自动同步）
      </div>
      <div v-else class="list">
        <div v-for="item in items" :key="item.id" class="row" :class="item.level">
          <div class="row-head">
            <span class="tag" :class="item.level">{{ levelText(item.level) }}</span>
            <span class="row-title">{{ item.title || "（无标题）" }}</span>
            <span v-if="!item.read_at" class="dot" title="未读">●</span>
          </div>
          <div v-if="item.body" class="body">{{ item.body }}</div>
          <div class="meta">
            {{ (item.created_at || item.received_at || "").replace("T", " ").slice(0, 16) }}
            <template v-if="item.pinned"> · 置顶</template>
          </div>
        </div>
      </div>
      <div v-if="note" class="note">{{ note }}</div>
    </div>
  </div>
</template>

<style scoped>
.bell-wrap {
  position: relative;
  display: inline-flex;
}
.bell {
  position: relative;
  width: 26px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-soft);
  border-radius: var(--radius-control, 7px);
  cursor: pointer;
}
.bell:hover,
.bell.on {
  background: var(--card-alt);
  color: var(--accent);
}
.badge {
  position: absolute;
  top: -2px;
  right: -3px;
  min-width: 14px;
  height: 14px;
  padding: 0 4px;
  border-radius: 999px;
  background: var(--danger);
  color: #fff;
  font-size: 9.5px;
  font-weight: 700;
  line-height: 14px;
  text-align: center;
}
.panel {
  position: absolute;
  top: 30px;
  right: 0;
  width: 330px;
  max-height: 380px;
  overflow-y: auto;
  z-index: 80;
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  box-shadow: var(--shadow-pop, 0 8px 28px -10px rgba(0, 0, 0, 0.28));
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
}
.title {
  font-weight: 600;
  color: var(--text);
}
.spacer {
  flex: 1;
}
.link {
  border: none;
  background: transparent;
  color: var(--accent);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.row {
  display: flex;
  flex-direction: column;
  /* 全局 .row 是 align-items:center，竖排会把公告标题居中，这里覆盖成左对齐 */
  align-items: stretch;
  gap: 3px;
  padding: 8px 10px;
  border-radius: 9px;
  background: var(--card-alt);
  border-left: 3px solid var(--accent);
}
.row.warning {
  border-left-color: var(--warning);
}
.row.critical {
  border-left-color: var(--danger);
}
.row-head {
  display: flex;
  align-items: center;
  gap: 6px;
}
.row-title {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text);
  flex: 1;
  min-width: 0;
}
.tag {
  font-size: 10px;
  padding: 0 6px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
}
.tag.warning {
  background: color-mix(in srgb, var(--warning) 14%, transparent);
  color: var(--warning);
}
.tag.critical {
  background: color-mix(in srgb, var(--danger) 14%, transparent);
  color: var(--danger);
}
.dot {
  color: var(--accent);
  font-size: 7px;
}
.body {
  font-size: 12px;
  color: var(--text-soft);
  line-height: 1.6;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.meta {
  font-size: 10.5px;
  color: var(--muted);
}
.empty {
  font-size: 12px;
  color: var(--muted);
  line-height: 1.7;
}
.note {
  font-size: 11px;
  color: var(--danger);
}
</style>
