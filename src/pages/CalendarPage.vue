<script setup lang="ts">
// 日历：月视图（默认）+ 周视图（行=节次、列=周一到周日），右侧当日面板
import { computed, onMounted, ref, watch } from "vue";
import Card from "../components/Card.vue";
import SoftButton from "../components/SoftButton.vue";
import IconGlyph from "../components/IconGlyph.vue";
import MetricLine from "../components/MetricLine.vue";
import Pill from "../components/Pill.vue";
import { eventIndex, settingsAll, settingsSave, type EventRow } from "../api";

const view = ref<"month" | "week">("month");
const cursor = ref(new Date());
const selected = ref(new Date());
const index = ref<Record<string, EventRow[]>>({});

const WEEKDAYS = ["一", "二", "三", "四", "五", "六", "日"];
const iso = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
    d.getDate(),
  ).padStart(2, "0")}`;

const monthLabel = computed(
  () => `${cursor.value.getFullYear()} 年 ${cursor.value.getMonth() + 1} 月`,
);

/** 月视图 6×7 格：从周一开始 */
const cells = computed(() => {
  const first = new Date(cursor.value.getFullYear(), cursor.value.getMonth(), 1);
  const offset = (first.getDay() + 6) % 7;
  const start = new Date(first);
  start.setDate(first.getDate() - offset);
  const out: { date: Date; key: string; outside: boolean; events: EventRow[] }[] = [];
  for (let i = 0; i < 42; i += 1) {
    const date = new Date(start);
    date.setDate(start.getDate() + i);
    const key = iso(date);
    out.push({
      date,
      key,
      outside: date.getMonth() !== cursor.value.getMonth(),
      events: index.value[key] || [],
    });
  }
  return out;
});

/** 周视图：当前周的 7 天 */
const weekDays = computed(() => {
  const base = new Date(selected.value);
  const offset = (base.getDay() + 6) % 7;
  const monday = new Date(base);
  monday.setDate(base.getDate() - offset);
  return Array.from({ length: 7 }, (_, i) => {
    const date = new Date(monday);
    date.setDate(monday.getDate() + i);
    const key = iso(date);
    return { date, key, events: index.value[key] || [] };
  });
});

/** 旧版周视图的行 = 节次 */
const PERIODS = [
  ["第 1 节", "08:00"],
  ["第 2 节", "08:50"],
  ["第 3 节", "09:55"],
  ["第 4 节", "10:45"],
  ["第 5 节", "11:50"],
  ["第 6 节", "13:30"],
  ["第 7 节", "14:20"],
  ["第 8 节", "15:25"],
  ["第 9 节", "16:15"],
  ["第 10 节", "17:05"],
  ["第 11 节", "18:30"],
  ["第 12 节", "19:20"],
] as const;

const selectedEvents = computed(
  () => index.value[iso(selected.value)] || [],
);

const doneCount = computed(
  () => selectedEvents.value.filter((one) => one.done).length,
);

const minuteOf = (clock: string) => {
  const [h, m] = clock.split(":").map(Number);
  return (h || 0) * 60 + (m || 0);
};

/** 一个日程落在哪个节次：开始时间之前（含）最近的那一节 */
function periodIndex(clock: string): number {
  const target = minuteOf(clock);
  let found = 0;
  PERIODS.forEach((period, i) => {
    if (minuteOf(period[1]) <= target) found = i;
  });
  return found;
}

/** 一个日程占几节：按时长算（至少 1 节，最多到当天结束） */
function periodSpan(event: { start_time: string; end_time: string }): number {
  const start = minuteOf(event.start_time);
  let end = minuteOf(event.end_time);
  if (end <= start) end = start + 45;            // 跨夜 / 没填结束时间
  const first = periodIndex(event.start_time);
  let last = first;
  PERIODS.forEach((period, i) => {
    if (minuteOf(period[1]) < end) last = i;
  });
  return Math.max(1, last - first + 1);
}

/** 周视图：每行一个节次，每列一天；色块按占用的节数撑高 */
const weekRows = computed(() =>
  PERIODS.map((period, i) => ({
    period,
    cells: weekDays.value.map((day) => ({
      key: day.key,
      events: day.events
        .filter((event) => periodIndex(event.start_time) === i)
        .map((event) => ({ event, span: periodSpan(event) })),
    })),
  })),
);

async function load() {
  const start = new Date(cursor.value.getFullYear(), cursor.value.getMonth() - 1, 1);
  const end = new Date(cursor.value.getFullYear(), cursor.value.getMonth() + 2, 0);
  index.value = await eventIndex(iso(start), iso(end));
}

onMounted(async () => {
  // 旧版 calendar_view：记住上次看的是月还是周
  try {
    const all = await settingsAll();
    const stored = (all.values || {})["calendar_view"];
    if (stored === "week") view.value = "week";
    else if (stored === "month") view.value = "month";
  } catch {
    // 读不到就用默认月视图
  }
  await load();
});
watch(() => [cursor.value.getFullYear(), cursor.value.getMonth()], load);
watch(view, (value) => {
  void settingsSave({ calendar_view: value });
});

function shift(step: number) {
  if (view.value === "month") {
    cursor.value = new Date(cursor.value.getFullYear(), cursor.value.getMonth() + step, 1);
  } else {
    const next = new Date(selected.value);
    next.setDate(next.getDate() + step * 7);
    selected.value = next;
    cursor.value = next;
  }
}
function today() {
  const now = new Date();
  cursor.value = now;
  selected.value = now;
}
function select(date: Date) {
  selected.value = date;
  if (date.getMonth() !== cursor.value.getMonth()) {
    cursor.value = date;
  }
}
const isToday = (date: Date) => iso(date) === iso(new Date());

/** 月视图：鼠标滚轮翻月（向下=下个月，向上=上个月），带节流，避免一下翻好几个月 */
let wheelLock = 0;
function onWheel(event: WheelEvent) {
  if (view.value !== "month") return;
  const delta =
    Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? event.deltaY : event.deltaX;
  if (Math.abs(delta) < 6) return;
  event.preventDefault();
  const now = Date.now();
  if (now - wheelLock < 260) return;
  wheelLock = now;
  shift(delta > 0 ? 1 : -1);
}
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">日历</h1>
        <div class="muted">{{ monthLabel }}</div>
      </div>
      <div class="spacer" />
      <SoftButton :height="32" :font-size="12.5" @click="shift(-1)">
        <IconGlyph name="CHEVRON_LEFT" :size="12" />
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" @click="today">今天</SoftButton>
      <SoftButton :height="32" :font-size="12.5" @click="shift(1)">
        <IconGlyph name="CHEVRON_RIGHT" :size="12" />
      </SoftButton>
      <div class="switch">
        <button :class="{ on: view === 'month' }" @click="view = 'month'">月</button>
        <button :class="{ on: view === 'week' }" @click="view = 'week'">周</button>
      </div>
      <SoftButton variant="primary" :height="32" :font-size="12.5" glyph="ADD">
        新建日程
      </SoftButton>
    </div>

    <div class="body">
      <Card :padding="12" class="calendar-card">
        <div v-if="view === 'month'" class="weekday-row">
          <span v-for="name in WEEKDAYS" :key="name">{{ name }}</span>
        </div>
        <div
          v-if="view === 'month'"
          class="month-grid"
          title="滚轮向上 / 向下翻月"
          @wheel="onWheel"
        >
          <button
            v-for="cell in cells"
            :key="cell.key"
            class="day-cell"
            :class="{
              outside: cell.outside,
              today: isToday(cell.date),
              selected: cell.key === iso(selected),
            }"
            @click="select(cell.date)"
          >
            <span class="num">{{ cell.date.getDate() }}</span>
            <span class="events">
              <span
                v-for="event in cell.events.slice(0, 3)"
                :key="event.id"
                class="event-chip"
                :class="`tone-${event.color}`"
                >{{ event.title }}</span
              >
              <span v-if="cell.events.length > 3" class="more">
                +{{ cell.events.length - 3 }}
              </span>
            </span>
          </button>
        </div>

        <div v-else class="week-grid">
          <div class="week-head">
            <span class="period-col"></span>
            <span
              v-for="day in weekDays"
              :key="day.key"
              class="day-head"
              :class="{ today: isToday(day.date) }"
            >
              {{ WEEKDAYS[(day.date.getDay() + 6) % 7] }}
              <b>{{ day.date.getDate() }}</b>
            </span>
          </div>
          <div v-for="row in weekRows" :key="row.period[0]" class="week-row">
            <span class="period-col">
              <b>{{ row.period[0] }}</b>
              <i>{{ row.period[1] }}</i>
            </span>
            <span v-for="cell in row.cells" :key="cell.key" class="week-cell">
              <span
                v-for="item in cell.events"
                :key="item.event.id"
                class="event-block"
                :class="`tone-${item.event.color}`"
                :style="{ height: `calc(${item.span} * 34px - 4px)` }"
                :title="`${item.event.start_time}–${item.event.end_time}`"
              >
                {{ item.event.title }}
              </span>
            </span>
          </div>
        </div>
      </Card>

      <Card :padding="12" class="side-card">
        <div class="side-head">
          <span class="h2">
            {{ iso(selected).slice(5) }} 当日
          </span>
          <span class="spacer" />
          <Pill :text="`${selectedEvents.length} 个 · ${doneCount} 已完成`" :font-size="11.5" />
        </div>
        <div v-if="!selectedEvents.length" class="muted empty">
          这一天还没有日程，双击日历上的日期可以新建
        </div>
        <MetricLine
          v-for="event in selectedEvents"
          :key="event.id"
          :title="event.title"
          :meta="`${event.start_time}–${event.end_time}`"
          :tone="event.color"
        />
        <div class="side-foot">
          <SoftButton :height="30" :font-size="12.5" glyph="ADD">新建日程</SoftButton>
        </div>
      </Card>
    </div>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
}
.switch {
  display: flex;
  padding: 2px;
  background: var(--card-alt);
  border-radius: var(--radius-pill);
}
.switch button {
  border: none;
  background: transparent;
  color: var(--muted);
  font-family: inherit;
  font-size: 12px;
  padding: 0 12px;
  height: 24px;
  border-radius: var(--radius-pill);
  cursor: pointer;
}
.switch button.on {
  background: var(--card);
  color: var(--text);
  font-weight: 600;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
}
.body {
  flex: 1;
  display: flex;
  gap: 12px;
  min-height: 0;
}
.calendar-card {
  flex: 1;
  min-width: 0;
}
.side-card {
  width: 300px;
  flex: none;
  gap: 6px;
}
.weekday-row {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  gap: 2px;
  padding: 0 0 6px;
  font-size: 12px;
  color: var(--muted);
  text-align: center;
}
.month-grid {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  grid-auto-rows: minmax(78px, 1fr);
  gap: 2px;
}
.day-cell {
  display: flex;
  flex-direction: column;
  gap: 3px;
  align-items: stretch;
  text-align: left;
  border: 1px solid transparent;
  border-radius: 9px;
  background: transparent;
  padding: 5px 6px;
  font-family: inherit;
  cursor: pointer;
  min-width: 0;
  overflow: hidden;
}
.day-cell:hover {
  background: var(--card-hover);
}
.day-cell.outside .num {
  color: var(--faint);
}
.day-cell .num {
  font-size: 12.5px;
  color: var(--text-soft);
}
.day-cell.today .num {
  color: #fff;
  background: var(--accent);
  border-radius: 999px;
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.day-cell.selected {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 7%, transparent);
}
.events {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.event-chip {
  font-size: 10.5px;
  padding: 1px 5px;
  border-radius: 5px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.more {
  font-size: 10px;
  color: var(--muted);
}
.week-grid {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.week-head,
.week-row {
  display: grid;
  grid-template-columns: 64px repeat(7, minmax(0, 1fr));
  gap: 2px;
  min-height: 34px;
}
.day-head {
  font-size: 11.5px;
  color: var(--muted);
  text-align: center;
  padding: 3px 0;
}
.day-head.today {
  color: var(--accent);
  font-weight: 650;
}
.period-col {
  display: flex;
  flex-direction: column;
  gap: 1px;
  font-size: 10.5px;
  color: var(--muted);
  padding: 2px 4px;
}
.period-col i {
  font-style: normal;
  color: var(--faint);
}
.week-cell {
  border-radius: 7px;
  background: color-mix(in srgb, var(--card-alt) 55%, transparent);
  padding: 2px;
  min-width: 0;
}
.event-block {
  display: block;
  font-size: 10.5px;
  padding: 2px 5px;
  border-radius: 5px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.side-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.side-head .h2 {
  font-size: 15px;
  font-weight: 640;
}
.empty {
  font-size: 12.5px;
  line-height: 1.6;
}
.side-foot {
  display: flex;
  justify-content: flex-end;
  margin-top: 6px;
}
/* 5 种日程配色（与旧版 EVENT_TINTS 一致） */
.tone-violet {
  background: #f3edfe;
  color: #6c3fa6;
}
.tone-teal {
  background: #e2f5f7;
  color: #00707f;
}
.tone-amber {
  background: #fdf1df;
  color: #8a5300;
}
.tone-rose {
  background: #fee9ee;
  color: #b1123c;
}
.tone-sky {
  background: #e4f0ff;
  color: #0058b8;
}
html[data-mode="dark"] .tone-violet {
  background: #352b47;
  color: #d7b8ff;
}
html[data-mode="dark"] .tone-teal {
  background: #103235;
  color: #7fe0dd;
}
html[data-mode="dark"] .tone-amber {
  background: #3a2b15;
  color: #ffc563;
}
html[data-mode="dark"] .tone-rose {
  background: #3c1f2b;
  color: #ff8fae;
}
html[data-mode="dark"] .tone-sky {
  background: #10283f;
  color: #7fbeff;
}
</style>
