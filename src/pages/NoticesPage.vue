<script setup lang="ts">
// 通知：教务 / eHall 通知流（对应旧版 page_notices.py）
// 外加「服务器公告」分区：自家账号服务器发来的公告（notify.rs 长轮询落到 announcements 表）
import { computed, onMounted, onUnmounted, ref } from "vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import {
  accountAnnouncements,
  accountCheckAnnouncements,
  accountMarkAnnouncementsRead,
  markAllNoticesRead,
  markNoticeRead,
  noticeList,
  onAnnouncement,
  openPath,
  refreshNotices,
  type NoticeItem,
  type ServerAnnouncement,
} from "../api";

const items = ref<NoticeItem[]>([]);
const unread = ref(0);
const onlyUnread = ref(false);
const busy = ref(false);
const message = ref("");

// ---- 服务器公告
const serverItems = ref<ServerAnnouncement[]>([]);
const serverUnread = ref(0);
const serverNote = ref("");
const serverBusy = ref(false);
const showServer = ref(true);
let off: (() => void) | null = null;

const countText = computed(
  () => `${items.value.length} 条${unread.value ? ` · ${unread.value} 条未读` : ""}`,
);
const serverCountText = computed(
  () =>
    `${serverItems.value.length} 条${serverUnread.value ? ` · ${serverUnread.value} 条未读` : ""}`,
);

async function loadServer() {
  try {
    const result = await accountAnnouncements(100);
    serverItems.value = result.items || [];
    serverUnread.value = result.unread || 0;
    const notify = result.notify;
    if (notify?.unsupported) {
      serverNote.value = "服务器还没有 /announcements 接口，会自动重试";
    } else if (notify?.last_error) {
      serverNote.value = `公告通道：${notify.last_error}`;
    }
  } catch (err) {
    serverNote.value = String(err);
  }
}

async function checkServer() {
  serverBusy.value = true;
  try {
    const result = await accountCheckAnnouncements();
    serverNote.value = result.ok
      ? `已检查：新增 ${result.added} 条`
      : `检查失败：${result.error || "未知错误"}`;
    await loadServer();
  } finally {
    serverBusy.value = false;
  }
}

async function readAllServer() {
  await accountMarkAnnouncementsRead();
  await loadServer();
}

function serverLevelText(level: string): string {
  if (level === "critical") return "紧急";
  if (level === "warning") return "重要";
  return "公告";
}

async function load() {
  const result = await noticeList(onlyUnread.value);
  items.value = result.items;
  unread.value = result.unread;
}

async function refresh() {
  busy.value = true;
  try {
    const result = await refreshNotices();
    message.value = result.ok
      ? `抓取完成：新增 ${result.added} 条`
      : result.error || "抓取失败";
    await load();
  } finally {
    busy.value = false;
  }
}

async function readAll() {
  const count = await markAllNoticesRead();
  message.value = `已把 ${count} 条标记为已读`;
  await load();
}

async function open(item: NoticeItem) {
  if (!item.read_at) {
    await markNoticeRead(item.id);
    await load();
  }
  if (item.url) {
    // 通知原文在 eHall 里；这里先用系统浏览器打开（内嵌 eHall 页在第四阶段）
    window.open(item.url, "_blank");
  }
}

onMounted(async () => {
  await load();
  await loadServer();
  off = await onAnnouncement(() => {
    void loadServer();
  });
});
onUnmounted(() => {
  if (off) off();
});
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">通知</h1>
        <div class="muted">教务、活动等通知汇总（日程不在此处）</div>
      </div>
      <div class="spacer" />
      <Pill :text="countText" :font-size="11.5" />
      <SoftButton :height="32" :font-size="12.5" glyph="CHECK" @click="readAll">
        全部标为已读
      </SoftButton>
      <SoftButton
        variant="primary"
        :height="32"
        :font-size="12.5"
        glyph="REFRESH"
        :disabled="busy"
        @click="refresh"
      >
        {{ busy ? "抓取中…" : "刷新抓取" }}
      </SoftButton>
    </div>

    <div class="filter">
      <label>
        <input v-model="onlyUnread" type="checkbox" @change="load" />
        只看未读
      </label>
      <span v-if="message" class="muted">{{ message }}</span>
    </div>

    <!-- 服务器公告（自家账号服务器发来的，读本地 announcements 表） -->
    <section class="server">
      <div class="server-head">
        <button class="fold" @click="showServer = !showServer">
          {{ showServer ? "▾" : "▸" }}
        </button>
        <span class="sec-title">服务器公告</span>
        <Pill :text="serverCountText" :font-size="11" />
        <span class="spacer" />
        <span v-if="serverNote" class="muted tiny">{{ serverNote }}</span>
        <SoftButton
          :height="28"
          :font-size="12"
          glyph="CHECK"
          :disabled="serverBusy"
          @click="readAllServer"
        >
          全部已读
        </SoftButton>
        <SoftButton
          :height="28"
          :font-size="12"
          glyph="REFRESH"
          :disabled="serverBusy"
          @click="checkServer"
        >
          {{ serverBusy ? "检查中…" : "立刻检查" }}
        </SoftButton>
      </div>
      <div v-if="showServer" class="server-list">
        <div v-if="!serverItems.length" class="empty">
          还没有服务器公告（在 设置 → 账户 里登录后会自动同步）
        </div>
        <div
          v-for="item in serverItems"
          :key="item.id"
          class="server-row"
          :class="[item.level, { unread: !item.read_at }]"
        >
          <span class="tag" :class="item.level">{{ serverLevelText(item.level) }}</span>
          <span class="text">
            <span class="title">
              {{ item.title || "（无标题）" }}
              <span v-if="item.pinned" class="pin">置顶</span>
            </span>
            <span v-if="item.body" class="sbody">{{ item.body }}</span>
            <span class="meta">
              {{ (item.created_at || item.received_at || "").replace("T", " ").slice(0, 16) }}
              <template v-if="item.expires_at">
                · 到期 {{ item.expires_at.slice(0, 10) }}
              </template>
              <template v-if="!item.read_at"> · 未读</template>
            </span>
          </span>
        </div>
      </div>
    </section>

    <div class="list">
      <div v-if="!items.length" class="empty">
        还没有抓到通知，点右上角「刷新抓取」（先在 设置 → 仪表板 里填通知来源地址）
      </div>
      <button
        v-for="item in items"
        :key="item.id"
        class="row"
        :class="{ unread: !item.read_at }"
        @click="open(item)"
      >
        <span class="dot">●</span>
        <span class="text">
          <span class="title">{{ item.title }}</span>
          <span class="meta">
            {{ item.source === "notice" ? "通知" : item.source }} ·
            {{ item.read_at ? `已读 ${item.read_at.slice(5, 16)}` : "未读" }}
          </span>
        </span>
        <span class="date">{{ (item.published_at || item.fetched_at || "").slice(0, 10) }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
}
.filter {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 12.5px;
  color: var(--muted);
}
.filter label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
/* ---- 服务器公告分区 ---- */
.server {
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-card);
  background: var(--card);
  padding: 8px 12px 10px;
}
.server-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
}
.fold {
  border: none;
  background: transparent;
  color: var(--muted);
  cursor: pointer;
  font-size: 12px;
  padding: 0 2px;
  font-family: inherit;
}
.sec-title {
  font-weight: 600;
  color: var(--text);
}
.server-head .spacer {
  flex: 1;
}
.server-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 220px;
  overflow-y: auto;
}
.server-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 7px 9px;
  border-radius: 9px;
  background: var(--card-alt);
  border-left: 3px solid var(--accent);
}
.server-row.warning {
  border-left-color: var(--warning);
}
.server-row.critical {
  border-left-color: var(--danger);
}
.server-row .text {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.server-row .title {
  font-size: 13px;
  color: var(--text);
  overflow-wrap: anywhere;
}
.server-row.unread .title {
  font-weight: 620;
}
.pin {
  margin-left: 6px;
  font-size: 10px;
  padding: 0 6px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  color: var(--accent);
}
.sbody {
  font-size: 12px;
  color: var(--text-soft);
  line-height: 1.6;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.tag {
  flex: none;
  font-size: 10px;
  padding: 1px 6px;
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
.list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding-right: 4px;
}
.row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 10px 13px;
  border-radius: var(--radius-card);
  border: 1px solid var(--border-soft);
  background: var(--card);
  text-align: left;
  font-family: inherit;
  cursor: pointer;
}
.row:hover {
  background: var(--card-hover);
}
.dot {
  font-size: 7px;
  line-height: 2;
  color: var(--faint);
}
.row.unread .dot {
  color: var(--accent);
}
.text {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}
.title {
  font-size: 14px;
  color: var(--text);
  overflow-wrap: anywhere;
}
.row.unread .title {
  font-weight: 600;
}
.meta {
  font-size: 11.5px;
  color: var(--muted);
}
.date {
  font-size: 12px;
  color: var(--muted);
  white-space: nowrap;
}
.empty {
  font-size: 12.5px;
  color: var(--muted);
  line-height: 1.7;
  padding: 8px 2px;
}
/* 列表行一律左对齐（button 默认居中） */
.list .row,
.list .row > span {
  text-align: left;
}
</style>
