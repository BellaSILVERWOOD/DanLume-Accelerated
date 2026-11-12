<script setup lang="ts">
// 仪表板：与旧版 page_dashboard.py 的格子顺序一致
// 今天(2) 天气(2) / 即将到来(2) 教务信息(1) 最近作业(1) / ehall 卡 ×4 …
import { computed, onMounted, ref } from "vue";
import HeroWidget from "../components/HeroWidget.vue";
import ListWidget from "../components/ListWidget.vue";
import WidgetCard from "../components/WidgetCard.vue";
import MetricLine from "../components/MetricLine.vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import {
  ehallCards,
  ehallIngestPasted,
  ehallOpen,
  ehallRefreshOnline,
  eventsOn,
  recentAssignments,
  recentNotices,
  upcomingEvents,
  weather,
  weekNumber,
  type AssignmentRow,
  type EhallCard,
  type NoticeRow,
  type UpcomingItem,
  type WeatherInfo,
} from "../api";

const emit = defineEmits<{ (e: "open-page", key: string): void }>();

/**
 * 首页不显示的 ehall 卡。
 *
 * 用户 2026-11-11：「首页的空教室重定向到空教室卡片吧。就首页不用显示了。」
 * —— 空教室是个要选条件才有意义的交互卡（旧版 `INTERACTIVE_CARDS`），
 * 放在仪表板只会显示一句抓来的碎文字；现在它只出现在左导航的「空教室」页，
 * 首页只留一个跳过去的入口。
 */
const HIDDEN_EHALL = new Set(["empty_room"]);

const items = ref<UpcomingItem[]>([]);
const notices = ref<NoticeRow[]>([]);
const assignments = ref<AssignmentRow[]>([]);
const assignError = ref("");
const weatherInfo = ref<WeatherInfo>({
  ok: true,
  text: "17°C  晴 · 湿度 82% · 风速 5 km/h",
  city: "上海",
  cached: false,
  error: "",
});
const busy = ref(false);

const today = new Date();
const dayNumber = today.getDate();
const weekdayNames = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
const weekday = weekdayNames[today.getDay()];

/** 本地日期（不能用 toISOString：那是 UTC，早上 8 点前会差一天） */
function isoToday(): string {
  return `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(
    dayNumber,
  ).padStart(2, "0")}`;
}

/** 旧版显示的是当年的 ISO 周数（10-30 → 第 44 周），由 Rust 侧算好 */
const weekNo = ref(weekNumberFallback());
function weekNumberFallback(): number {
  const first = new Date(today.getFullYear(), 0, 1);
  const days = Math.floor((today.getTime() - first.getTime()) / 86400000);
  return Math.max(1, Math.ceil((days + first.getDay()) / 7));
}

/** 今天一共有几个日程（含已完成的）——「即将到来」只算还没结束的，不能拿它数 */
const todayTotal = ref(0);
const todayDone = ref(0);

/**
 * 「同步 ehall」：旧版是内嵌浏览器逐张抓；v2 改成**一次会话直连刷新**
 * （Rust 的 ehall_remote：带 UIS 会话取页面 → 复用粘贴抓取那套解析 → 写缓存）。
 * 会话失效时提示去登录；「粘贴抓取」保留为兜底。
 */
async function syncEhall() {
  const targets = ehall.value.filter((card) => card.source.trim());
  if (!targets.length) {
    syncHint.value = "先在 设置 → 仪表板 里填数据地址";
    return;
  }
  syncHint.value = "正在用会话直连刷新…";
  try {
    const result = await ehallRefreshOnline();
    if (result.need_login) {
      syncHint.value =
        "会话失效：请在 设置 → 统一身份认证 里「打开登录窗口」登录，再回来点「抓取会话」，然后重试同步";
      return;
    }
    if (!result.ok) {
      syncHint.value = "刷新失败：可以改用「粘贴抓取」兜底";
      return;
    }
    await loadCards();
    syncHint.value = `已刷新 ${result.refreshed}/${result.total} 张卡`;
  } catch (err) {
    syncHint.value = `刷新出错：${String(err)}`;
  }
}

/** 打开粘贴框（默认选第一张有地址的卡） */
function openPaste() {
  pasteKey.value = (ehall.value.find((card) => card.source.trim()) || ehall.value[0] || { key: "card" }).key;
  pasteText.value = "";
  pasteResult.value = "";
  pasteOpen.value = true;
}

async function submitPaste() {
  if (!pasteText.value.trim()) {
    pasteResult.value = "先粘贴页面内容";
    return;
  }
  const result = await ehallIngestPasted(pasteKey.value, pasteText.value);
  pasteResult.value = result.ok
    ? `已解析：${result.value || "（没找到明确数值，已存原文）"}`
    : `解析失败：${result.error}`;
  await loadCards();
}

async function refresh() {
  busy.value = true;
  try {
    items.value = await upcomingEvents(5);
    const todayRows = await eventsOn(isoToday());
    todayTotal.value = todayRows.length;
    todayDone.value = todayRows.filter((one) => one.done).length;
    notices.value = await recentNotices(5);
    const result = await recentAssignments(4);
    assignments.value = result.items || [];
    assignError.value = result.ok ? "" : result.error;
    weatherInfo.value = await weather();
    weekNo.value = await weekNumber();
    await loadCards();
  } finally {
    busy.value = false;
  }
}

onMounted(refresh);

function when(item: UpcomingItem): string {
  if (item.ongoing) return "进行中";
  return `${item.day.slice(5)} ${item.begin}`;
}

const EHALL_CARDS = [
  { title: "校园卡余额", glyph: "LOCK", tone: "teal", hint: "填一张卡页面的地址" },
  { title: "校车安排", glyph: "CLOCK", tone: "amber", hint: "填校车时刻页面的地址" },
  { title: "体育刷锻", glyph: "CHECK", tone: "rose", hint: "填刷锻页面的地址" },
  { title: "图书馆人数", glyph: "CONTACT", tone: "sky", hint: "填在馆人数页面的地址" },
] as const;

/** ehall 卡片：读旧版缓存（校园卡 / 校车 / 刷锻 / 图书馆 / ehall 通知）；空教室不在首页显示 */
const ehall = ref<EhallCard[]>([]);

async function loadCards() {
  const all = await ehallCards();
  ehall.value = all.filter((card) => !HIDDEN_EHALL.has(card.key));
}
const syncHint = ref("");
const pasteOpen = ref(false);
const pasteKey = ref("card");
const pasteText = ref("");
const pasteResult = ref("");

function cardGlyph(key: string) {
  const table: Record<string, string> = {
    card: "LOCK",
    bus: "CLOCK",
    sports: "CHECK",
    library_people: "CONTACT",
    ehall_todo: "MAIL",
    empty_room: "CALENDAR",
  };
  return (table[key] || "INFO") as never;
}

function cardTone(key: string): string {
  const table: Record<string, string> = {
    card: "teal",
    bus: "amber",
    sports: "rose",
    library_people: "sky",
    ehall_todo: "violet",
    empty_room: "teal",
  };
  return table[key] || "teal";
}

function cardMeta(card: EhallCard): string {
  if (!card.ok) return card.source ? "还没同步" : "还没填地址";
  if (card.age_minutes >= 0) {
    return card.age_minutes < 60
      ? `${card.age_minutes} 分钟前`
      : `${Math.round(card.age_minutes / 60)} 小时前`;
  }
  return "已同步";
}
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">仪表板</h1>
        <div class="muted">今天要做什么，一眼看完</div>
        <div class="chips">
          <span>今天</span><span class="dot">·</span><span>日程</span>
          <span class="dot">·</span><span>天气</span>
          <span class="dot">·</span><span>教务与作业</span>
        </div>
      </div>
      <div class="spacer" />
      <div class="head-actions">
        <SoftButton :height="32" :font-size="12.5">登录 eHall</SoftButton>
        <SoftButton :height="32" :font-size="12.5" glyph="REFRESH" :disabled="busy" @click="syncEhall">
          同步 ehall
        </SoftButton>
        <SoftButton :height="32" :font-size="12.5" glyph="DOCUMENT" @click="openPaste">
          粘贴抓取
        </SoftButton>
        <SoftButton :height="32" :font-size="12.5" glyph="REFRESH" @click="refresh">
          刷新
        </SoftButton>
      </div>
    </div>

    <div class="grid">
      <HeroWidget
        class="span2"
        title="今天"
        glyph="CALENDAR"
        tone="sky"
        :meta="`${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(dayNumber).padStart(2, '0')}`"
        :value="String(dayNumber)"
        :sub="`${weekday} · 第 ${weekNo} 周`"
        :hint="todayTotal ? `今晚好，今天 ${todayTotal} 个日程` : '今晚好，今天没有日程'"
      />
      <HeroWidget
        class="span2"
        title="天气"
        glyph="SUN"
        tone="teal"
        :meta="weatherInfo.city"
        :value="weatherInfo.text.split(' ')[0] || '--'"
        :sub="weatherInfo.text.split('  ').slice(1).join(' · ') || '取不到天气'"
        :hint="weatherInfo.error || '每秒刷新一次（缓存 10 分钟）'"
      />

      <ListWidget
        class="span2"
        title="即将到来"
        glyph="CLOCK"
        tone="violet"
        meta="未来 3 天"
        :hint="items.length ? '' : '未来 3 天没有日程'"
      >
        <MetricLine
          v-for="item in items"
          :key="item.id"
          :title="item.title"
          :meta="when(item)"
          :tone="item.color"
          :meta-active="item.ongoing"
        />
      </ListWidget>

      <ListWidget
        title="教务信息"
        glyph="BELL"
        tone="amber"
        meta="最近 5 条"
        :hint="notices.length ? '' : '还没有抓到通知（「通知」页开启监控后会存到这里）'"
      >
        <MetricLine
          v-for="notice in notices"
          :key="notice.id"
          :title="notice.title"
          :meta="(notice.published_at || notice.fetched_at || '').slice(0, 10)"
          tone="amber"
          link
        />
      </ListWidget>

      <ListWidget
        title="最近作业"
        glyph="DOWNLOAD"
        tone="rose"
        meta="来自 eLearning"
        :hint="assignments.length || assignError ? assignError : '点右上角「同步」读一次 eLearning 的 To Do'"
      >
        <MetricLine
          v-for="work in assignments"
          :key="work.title"
          :title="work.title"
          :meta="work.due"
          tone="rose"
          link
          @click="emit('open-page', 'homework')"
        />
        <div v-if="!assignments.length" class="jump">
          作业页里已经内嵌了 eLearning 的提交窗口，
          <a @click="emit('open-page', 'homework')">去作业页</a>
        </div>
      </ListWidget>
    </div>

    <div class="section-label">
      ehall 数据　登录一次 ehall 就能取；地址在 设置 → 仪表板 里填
      <span v-if="syncHint" class="sync-hint">{{ syncHint }}</span>
      <a class="jump-link" @click="emit('open-page', 'classroom')">空教室 →</a>
    </div>
    <div class="grid">
      <WidgetCard
        v-for="card in ehall"
        :key="card.key"
        :title="card.title"
        :glyph="cardGlyph(card.key)"
        :tone="cardTone(card.key)"
        :meta="cardMeta(card)"
        :min-height="128"
      >
        <div class="empty-value" :class="{ ok: card.ok }">
          {{ card.ok ? card.value || "已同步" : "还没取到" }}
        </div>
        <div v-if="card.ok && card.lines.length" class="card-lines">
          {{ card.lines.slice(0, 3).join(" · ") }}
        </div>
        <div v-else class="tiny">{{ card.error || "点「同步 ehall」" }}</div>
      </WidgetCard>
    </div>

    <div v-if="pasteOpen" class="paste-mask">
      <div class="paste-card">
        <div class="paste-head">
          <span class="h2">粘贴抓取</span>
          <span class="spacer" />
          <SoftButton :height="26" :font-size="11.5" @click="pasteOpen = false">关闭</SoftButton>
        </div>
        <div class="tiny">
          在 eHall / 校园卡 / 图书馆页面里 <b>Ctrl+A 全选 → Ctrl+C</b>，粘到下面；解析结果会写进对应卡片。
        </div>
        <select v-model="pasteKey">
          <option v-for="card in ehall" :key="card.key" :value="card.key">{{ card.title }}</option>
        </select>
        <textarea v-model="pasteText" rows="8" placeholder="把页面文字粘到这里…" />
        <div class="paste-foot">
          <span class="tiny">{{ pasteResult }}</span>
          <span class="spacer" />
          <SoftButton variant="primary" :height="28" :font-size="12" @click="submitPaste">
            解析并保存
          </SoftButton>
        </div>
      </div>
    </div>

    <div class="foot">
      <Pill :text="`${items.length} 个日程 · ${assignments.length} 个作业待交`" :font-size="11.5" />
    </div>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.chips {
  display: flex;
  gap: 6px;
  font-size: 12px;
  color: var(--faint);
  margin-top: 2px;
}
.chips .dot {
  opacity: 0.6;
}
/* 右上角的按钮成组：窗口变窄时整组一起换行，不会只剩一个「刷新」孤零零掉到下一排 */
.head-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  justify-content: flex-end;
}
.grid {
  /* 用 flex 换行代替固定 4 列：卡片有 240px 的下限，宽度不够就自动换行到下一排，
     不会像 4 列网格那样被压到一百多像素、中文字逐字竖排。
     中栏 + 右栏同时打开时，这里会自然从一排 3 张变成 2 张/1 张。 */
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: stretch;
}
.grid > * {
  flex: 1 0 260px;
  min-width: 0;
}
.grid > .span2 {
  flex: 2 0 340px;
}
.section-label {
  font-size: 12px;
  color: var(--muted);
  margin-top: 4px;
}
.empty-value {
  font-size: 17px;
  font-weight: 600;
  color: var(--text-soft);
}
.fill {
  min-height: 1px;
}
.paste-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.26);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 50;
}
.paste-card {
  width: 620px;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 16px;
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  box-shadow: 0 18px 48px rgba(0, 0, 0, 0.28);
}
.paste-head,
.paste-foot {
  display: flex;
  align-items: center;
  gap: 8px;
}
.paste-head .h2 {
  font-size: 16px;
  font-weight: 650;
}
.paste-card textarea {
  width: 100%;
  resize: vertical;
  font-size: 12.5px;
  line-height: 1.6;
}
.sync-hint {
  color: var(--accent);
  margin-left: 8px;
}
.foot {
  display: flex;
  justify-content: flex-end;
  padding-top: 2px;
}
.jump {
  font-size: 12px;
  color: var(--muted);
  line-height: 1.6;
  padding-top: 4px;
}
.jump a,
.jump-link {
  color: var(--accent);
  cursor: pointer;
  text-decoration: underline;
  text-underline-offset: 2px;
}
.jump-link {
  margin-left: 10px;
}
</style>
