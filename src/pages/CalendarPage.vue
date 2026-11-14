<script setup lang="ts">
// 日历：月视图（默认）+ 周视图（行=节次、列=周一到周日），右侧当日面板
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import Card from "../components/Card.vue";
import SoftButton from "../components/SoftButton.vue";
import IconGlyph from "../components/IconGlyph.vue";
import MetricLine from "../components/MetricLine.vue";
import Pill from "../components/Pill.vue";
import {
  examsImport,
  eventIndex,
  eventSeriesDone,
  eventSetDone,
  homeworkMarkDone,
  homeworkSync,
  homeworkUpcoming,
  importantDelete,
  importantList,
  importantSave,
  openUrl,
  settingsAll,
  settingsSave,
  type AssignmentDue,
  type EventRow,
  type ImportantDate,
} from "../api";
import { openInAppBrowser } from "../lib/appBus";
import EventEditor from "../components/EventEditor.vue";
import ConfirmDialog from "../components/ConfirmDialog.vue";

const view = ref<"month" | "week">("month");
const cursor = ref(new Date());
const selected = ref(new Date());
const index = ref<Record<string, EventRow[]>>({});

const WEEKDAYS = ["一", "二", "三", "四", "五", "六", "日"];
const iso = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
    d.getDate(),
  ).padStart(2, "0")}`;

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

/** 顶上那行标题：月视图是年月，周视图给出一周的范围（不然按「上一周」看不出变化） */
const headLabel = computed(() => {
  if (view.value === "month") {
    return `${cursor.value.getFullYear()} 年 ${cursor.value.getMonth() + 1} 月`;
  }
  const first = weekDays.value[0];
  const last = weekDays.value[6];
  const fmt = (d: Date) => `${d.getMonth() + 1} 月 ${d.getDate()} 日`;
  return `${first.date.getFullYear()} 年 ${fmt(first.date)} – ${fmt(last.date)}`;
});

/** 旧版周视图的行 = 节次（与 Python `config.PERIOD_TIMES` 的 14 节逐条一致） */
const PERIODS = [
  ["第 1 节", "08:00"],
  ["第 2 节", "08:55"],
  ["第 3 节", "09:55"],
  ["第 4 节", "10:50"],
  ["第 5 节", "11:45"],
  ["第 6 节", "13:30"],
  ["第 7 节", "14:25"],
  ["第 8 节", "15:25"],
  ["第 9 节", "16:20"],
  ["第 10 节", "17:15"],
  ["第 11 节", "18:30"],
  ["第 12 节", "19:25"],
  ["第 13 节", "20:20"],
  ["第 14 节", "21:15"],
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

/** 一节课的起止分钟（结束早于开始就按 45 分钟算） */
function spanOf(event: { start_time: string; end_time: string }) {
  const start = minuteOf(event.start_time);
  let end = minuteOf(event.end_time);
  if (!Number.isFinite(end) || end <= start) end = start + 45;
  return { start, end: Math.min(end, 24 * 60) };
}

/** 周视图每小时占多少像素（1 分钟 1 像素 → 一小时 60 像素） */
const PX_PER_MIN = 1;

/**
 * 周视图的时间轴范围。
 *
 * 默认就是**整天上课时段 08:00–22:00**（14 节全在里面），有更早/更晚的日程再往外扩——
 * 以前只按本周日程算边界，于是第 13/14 节的刻度落在白色背景外面（用户看到的就是「背景不够长」）。
 */
const weekRange = computed(() => {
  let from = 8 * 60;
  let to = 22 * 60;
  weekDays.value.forEach((day) => {
    day.events.forEach((event) => {
      const { start, end } = spanOf(event);
      from = Math.min(from, start - 15);
      to = Math.max(to, end + 15);
    });
  });
  return {
    from: Math.max(0, Math.floor(from / 60) * 60),
    to: Math.min(24 * 60, Math.ceil(to / 60) * 60),
  };
});

const weekHeight = computed(() => (weekRange.value.to - weekRange.value.from) * PX_PER_MIN);

/** 左侧节次刻度：按真实时间落在轴上的位置（不再是等分格子） */
const periodTicks = computed(() =>
  PERIODS.map((period) => ({
    label: period[0],
    clock: period[1],
    top: (minuteOf(period[1]) - weekRange.value.from) * PX_PER_MIN,
  })).filter((tick) => tick.top >= -6 && tick.top <= weekHeight.value),
);

/** 每小时一条浅色参考线 */
const hourLines = computed(() => {
  const out: number[] = [];
  const first = Math.ceil(weekRange.value.from / 60) * 60;
  for (let minute = first; minute <= weekRange.value.to; minute += 60) {
    out.push((minute - weekRange.value.from) * PX_PER_MIN);
  }
  return out;
});

/** 时间轴上一个点（分钟）→ 像素 */
const yOf = (minute: number) => (minute - weekRange.value.from) * PX_PER_MIN;

/**
 * 一天里的日程块：**位置和高度都按分钟算**（用户要求的「跟时长成精确关系」）。
 *
 * 宽度只在该「互相重叠的那一组」里分泳道：
 * 以前是按整天算最大泳道数，只要这一天里任何两条日程撞上，**全天所有日程都被压成一半宽**
 * （用户截图里 08:00 的线性代数明明没有冲突也是半宽，就是这个）。
 */
function dayBlocks(day: { key: string; events: EventRow[] }) {
  // 同一个日程可能被重复列出（同 id + 同一天 + 同起止），去重后再算泳道；
  // 否则重复项会跟自己「重叠」，把块压成一半宽（用户看到的窄条就是这个）。
  const seen = new Set<string>();
  const items = day.events
    .filter((event) => {
      const key = `${event.id}|${event.date}|${event.start_time}|${event.end_time}`;
      if (seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    })
    .map((event) => ({ event, ...spanOf(event) }))
    .sort((a, b) => a.start - b.start || a.end - b.end);
  const out: {
    key: string;
    event: EventRow;
    top: number;
    height: number;
    lane: number;
    lanes: number;
    clock: string;
  }[] = [];
  // 把时间上互相重叠的日程切成「组」，组内才需要分泳道
  let group: typeof items = [];
  let groupEnd = -1;
  const flush = () => {
    if (!group.length) {
      return;
    }
    const laneEnds: number[] = [];
    const placed = group.map((item) => {
      let lane = laneEnds.findIndex((busyUntil) => busyUntil <= item.start);
      if (lane < 0) {
        lane = laneEnds.length;
        laneEnds.push(0);
      }
      laneEnds[lane] = item.end;
      return { item, lane };
    });
    const lanes = Math.max(1, laneEnds.length);
    for (const { item, lane } of placed) {
      out.push({
        key: `${item.event.id}-${item.event.date}`,
        event: item.event,
        top: yOf(item.start),
        height: Math.max(16, (item.end - item.start) * PX_PER_MIN - 1),
        lane,
        lanes,
        clock: `${item.event.start_time}–${item.event.end_time}`,
      });
    }
    group = [];
    groupEnd = -1;
  };
  for (const item of items) {
    if (group.length && item.start >= groupEnd) {
      flush();
    }
    group.push(item);
    groupEnd = Math.max(groupEnd, item.end);
  }
  flush();
  return out;
}

/** 一周的日程块（每天各自算重叠分组） */
const weekColumns = computed(() =>
  weekDays.value.map((day, di) => ({
    day,
    index: di,
    blocks: dayBlocks(day),
  })),
);

async function load() {
  const start = new Date(cursor.value.getFullYear(), cursor.value.getMonth() - 1, 1);
  const end = new Date(cursor.value.getFullYear(), cursor.value.getMonth() + 2, 0);
  index.value = await eventIndex(iso(start), iso(end));
}

// ------------------------------------------------------------ 重要日（倒计时）
// 对应旧版 important.py + important_dialog.py：以前 Rust 只有表、没有界面
const importantItems = ref<ImportantDate[]>([]);
const importantBusy = ref(false);
const importantMsg = ref("");
const draftTitle = ref("");
const draftDate = ref("");
const draftNote = ref("");

async function loadImportant() {
  try {
    const result = await importantList();
    importantItems.value = (result.items || []).filter((one) => one.days >= -1);
  } catch {
    importantItems.value = [];
  }
}

async function addImportant() {
  if (importantBusy.value) return;
  const title = draftTitle.value.trim();
  const day = draftDate.value.trim();
  if (!title || !day) {
    importantMsg.value = "标题和日期都要填（日期可以写 明天 / +7 / 2026-06-20）";
    return;
  }
  importantBusy.value = true;
  try {
    const result = await importantSave({ title, date: day, note: draftNote.value.trim() });
    if (!result.ok) {
      importantMsg.value = result.error || "保存失败";
      return;
    }
    importantMsg.value = `已加入：${result.date}`;
    draftTitle.value = "";
    draftDate.value = "";
    draftNote.value = "";
    await loadImportant();
  } finally {
    importantBusy.value = false;
  }
}

/** 删重要日：也先确认（删了找不回来） */
const pendingImportant = ref<ImportantDate | null>(null);
const importantConfirmOpen = ref(false);

function removeImportant(item: ImportantDate) {
  pendingImportant.value = item;
  importantConfirmOpen.value = true;
}

async function doRemoveImportant() {
  const item = pendingImportant.value;
  pendingImportant.value = null;
  if (!item) return;
  importantBusy.value = true;
  try {
    await importantDelete(item.id);
    await loadImportant();
  } finally {
    importantBusy.value = false;
  }
}

/**
 * 勾 / 取消「完成」。
 * 传 event.date：重复日程展开出来的那一次只影响这一天，不会把整条系列带成已完成。
 */
async function toggleDone(event: EventRow) {
  await eventSetDone(event.id, !event.done, event.date);
  await load();
}

/** 整条重复系列一起完成 / 取消（对应旧版「这一串都做完了」的诉求） */
async function toggleSeries(event: EventRow) {
  await eventSeriesDone(event.id, !event.done);
  await load();
}

// ------------------------------------------------------------ 新建 / 编辑
const editorOpen = ref(false);
const editorInitial = ref<Partial<EventRow> | null>(null);

function openNew(day?: Date) {
  editorInitial.value = { date: iso(day || selected.value) };
  editorOpen.value = true;
}

function openEdit(event: EventRow) {
  editorInitial.value = { ...event };
  editorOpen.value = true;
}

// 每分钟重读一次：过期日程会被「过期自动完成」勾掉（旧版 refresh_all 也是定时刷）
let refreshTimer: number | null = null;
// ---- 导入考试安排（J2-1）：教务「我的考试」→ 日历 + 重要日
const examItems = ref<import("../api").ExamItem[]>([]);
const examBusy = ref(false);
const examMsg = ref("");
const examOpen = ref(false);

/** 先读一遍（不写），把结果摆出来给人确认 */
async function previewExams() {
  if (examBusy.value) return;
  examBusy.value = true;
  examMsg.value = "正在打开教务的「我的考试」…（弹出的窗口里如果要求登录，先登录一次）";
  examOpen.value = true;
  examItems.value = [];
  try {
    const result = await examsImport(false, true);
    if (!result.ok) {
      examMsg.value = result.error || "没读到考试安排";
      return;
    }
    examItems.value = result.items || [];
    examMsg.value = examItems.value.length
      ? `读到 ${examItems.value.length} 场考试，确认后写进日历（提前 1 小时提醒）和「重要日」`
      : "这个学期还没读到考试安排";
  } catch (err) {
    examMsg.value = `读取失败：${String(err)}`;
  } finally {
    examBusy.value = false;
  }
}

/** 确认写入 */
async function commitExams() {
  if (examBusy.value) return;
  examBusy.value = true;
  examMsg.value = "正在写入日历…";
  try {
    const result = await examsImport(true, true);
    if (!result.ok) {
      examMsg.value = result.error || "写入失败";
      return;
    }
    const added = result.result?.added ?? 0;
    const skipped = result.result?.skipped ?? 0;
    examMsg.value = `写好了：新增 ${added} 条，跳过 ${skipped} 条（已存在的）`;
    await load();
    await loadImportant();
    examOpen.value = false;
  } catch (err) {
    examMsg.value = `写入失败：${String(err)}`;
  } finally {
    examBusy.value = false;
  }
}

// ---- 作业截止（用户 2026-11-12：eLearning 的 /dash 上有截止时间，摆到日历页右侧）
const assignments = ref<AssignmentDue[]>([]);
const assignmentMsg = ref("");
const assignmentBusy = ref(false);
const assignmentDays = ref(14);
const assignmentSyncedAt = ref("");

async function loadAssignments() {
  try {
    const result = await homeworkUpcoming(assignmentDays.value);
    assignments.value = result.items || [];
    assignmentSyncedAt.value = result.synced_at || "";
  } catch {
    assignments.value = [];
  }
}

/** 「同步」：拉一次 eLearning 的 To Do（需要先登录过 eLearning），顺便写进日程 */
async function syncAssignments() {
  if (assignmentBusy.value) return;
  assignmentBusy.value = true;
  assignmentMsg.value = "正在同步 eLearning 的作业…";
  try {
    const result = await homeworkSync();
    if (!result.ok) {
      const error = result.error || "";
      // 401 = eLearning 那边没有登录态，这句比原始报错有用得多
      if (error.includes("401")) {
        assignmentMsg.value =
          "eLearning 还没登录（401）：去左侧「作业」页点一次登录，登录好再回来点「同步」";
      } else {
        assignmentMsg.value = `同步失败：${error || "未知原因"}`;
      }
      return;
    }
    await loadAssignments();
    assignmentMsg.value = `同步好了：${assignments.value.length} 个待交作业`;
  } catch (err) {
    assignmentMsg.value = `同步失败：${String(err)}`;
  } finally {
    assignmentBusy.value = false;
  }
}

/** 刷新：只重读本地缓存（不联网）；要拉最新数据用「同步」 */
async function refreshAssignments() {
  await loadAssignments();
  assignmentMsg.value = assignments.value.length
    ? `已刷新：${assignments.value.length} 个待交作业`
    : "已刷新：没有待交作业";
}

/** 标成「已完成/已提交」：从卡片移除，并且不再提醒 */
async function markAssignmentDone(item: AssignmentDue) {
  try {
    await homeworkMarkDone(item.key, true);
    await loadAssignments();
    assignmentMsg.value = `「${item.title}」已标记完成，不再提醒`;
  } catch (err) {
    assignmentMsg.value = `标记失败：${String(err)}`;
  }
}

/** 「还有 5 小时 / 还有 2 天 3 小时 / 已过 30 分钟」—— 文案由 Rust 统一给，前后端一致 */
function dueText(item: AssignmentDue): string {
  return item.left_text || (item.overdue ? "已过期" : "");
}

function openAssignment(item: AssignmentDue) {
  // 用户 2026-11-12：用应用内置的浏览器（作业页里那个内嵌 eLearning），不弹系统浏览器
  if (item.url) openInAppBrowser(item.url);
}

onMounted(async () => {
  await loadAssignments();
  refreshTimer = window.setInterval(() => {
    if (document.visibilityState === "visible") void load();
  }, 60_000);
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
  await loadImportant();
});
onUnmounted(() => {
  if (refreshTimer !== null) window.clearInterval(refreshTimer);
  refreshTimer = null;
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

/** 周视图里点了某一天：选它（右侧当日面板跟着变），并保证这一周的数据已加载 */
function selectDay(date: Date, event?: Event) {
  event?.stopPropagation();
  select(date);
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
        <div class="muted">{{ headLabel }}</div>
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
      <SoftButton variant="primary" :height="32" :font-size="12.5" glyph="ADD" @click="openNew()">
        新建日程
      </SoftButton>
    </div>

    <div class="body">
      <Card :padding="12" class="calendar-card">
        <!-- 周视图比卡片高：内容放进自己的滚动区，别让日程块溢到卡片外面 -->
        <div class="calendar-scroll">
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
            @dblclick="openNew(cell.date)"
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
            <span class="head-spacer"></span>
            <button
              v-for="day in weekDays"
              :key="day.key"
              class="day-head"
              :class="{
                today: isToday(day.date),
                selected: day.key === iso(selected),
              }"
              @click="selectDay(day.date, $event)"
              @dblclick="openNew(day.date)"
            >
              {{ WEEKDAYS[(day.date.getDay() + 6) % 7] }}
              <b>{{ day.date.getDate() }}</b>
            </button>
          </div>
          <div class="week-body" :style="{ height: `${weekHeight}px` }">
            <!-- 左侧时间刻度：节次标在它在时间轴上的真实位置 -->
            <div class="scale-col">
              <span
                v-for="tick in periodTicks"
                :key="`tick-${tick.clock}`"
                class="scale-tick"
                :style="{ top: `${tick.top}px` }"
              >
                <b>{{ tick.label }}</b>
                <i>{{ tick.clock }}</i>
              </span>
            </div>
            <!-- 每天一列：背景网格 + 按分钟定位的日程块 -->
            <div
              v-for="col in weekColumns"
              :key="`col-${col.day.key}`"
              class="day-col"
              :class="{
                today: isToday(col.day.date),
                'is-selected': col.day.key === iso(selected),
              }"
              @click="selectDay(col.day.date, $event)"
              @dblclick="openNew(col.day.date)"
            >
              <span
                v-for="top in hourLines"
                :key="`line-${col.day.key}-${top}`"
                class="hour-line"
                :style="{ top: `${top}px` }"
              />
              <div
                v-for="blk in col.blocks"
                :key="blk.key"
                class="event-block"
                :class="`tone-${blk.event.color}`"
                :style="{
                  top: `${blk.top}px`,
                  height: `${blk.height}px`,
                  left: `calc(${(blk.lane * 100) / blk.lanes}% + 2px)`,
                  width: `calc(${100 / blk.lanes}% - 4px)`,
                }"
                :title="`${blk.event.title} ${blk.clock}`"
                @click.stop="openEdit(blk.event)"
              >
                <b class="blk-time">{{ blk.clock }}</b>
                <span class="blk-title">{{ blk.event.title }}</span>
              </div>
            </div>
          </div>
        </div>
        </div>
      </Card>

      <!-- 右侧竖排：当日日程 + 重要日（两张卡同一列，避免挤掉日历的宽度） -->
      <div class="side-stack">
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
        <div
          v-for="(event, at) in selectedEvents"
          :key="`${event.id}-${event.date}-${at}`"
          class="ev-row"
          @dblclick="openEdit(event)"
        >
          <button
            class="ev-check"
            :class="{ on: event.done }"
            :title="event.done ? '取消完成' : '标记完成'"
            @click="toggleDone(event)"
          >
            <IconGlyph v-if="event.done" name="CHECK" :size="11" color="var(--card)" />
          </button>
          <MetricLine
            :title="event.title"
            :meta="`${event.start_time}–${event.end_time}${event.repeat ? ' · 重复' : ''}`"
            :tone="event.color"
          />
          <button
            v-if="event.repeat"
            class="ev-series"
            :title="event.done ? '整条重复系列一起取消完成' : '整条重复系列一起标完成'"
            @click="toggleSeries(event)"
          >
            整串
          </button>
        </div>
        <div class="side-foot">
          <SoftButton :height="30" :font-size="12.5" glyph="ADD" @click="openNew()">新建日程</SoftButton>
        </div>
      </Card>

      <!-- 重要日（倒计时）：对应旧版 important.py / important_dialog.py -->
      <Card :padding="12" class="side-card important-card">
        <div class="side-head">
          <span class="h2">重要日</span>
          <span class="spacer" />
          <SoftButton :height="24" :font-size="11" :disabled="examBusy" @click="previewExams">
            {{ examBusy ? "读取中…" : "导入考试" }}
          </SoftButton>
          <Pill :text="`${importantItems.length} 个`" :font-size="11.5" />
        </div>
        <div v-if="!importantItems.length" class="muted empty">
          还没有重要日。填「考试 / 答辩 / 截止」这类日子，这里会显示还差几天。
        </div>
        <div
          v-for="item in importantItems"
          :key="item.id"
          class="important-row"
          :class="`tone-${item.color}`"
        >
          <div class="imp-main">
            <b>{{ item.title }}</b>
            <span class="imp-date">{{ item.date.slice(5) }}{{ item.note ? ` · ${item.note}` : "" }}</span>
          </div>
          <span class="imp-left">{{ item.left }}</span>
          <button class="imp-del" title="删掉这条" @click="removeImportant(item)">×</button>
        </div>
        <div class="important-add">
          <input v-model="draftTitle" type="text" placeholder="标题（如 期末考）" />
          <input v-model="draftDate" type="text" placeholder="日期：2026-06-20 / 明天 / +7" />
          <input v-model="draftNote" type="text" placeholder="备注（可空）" />
          <SoftButton :height="28" :font-size="12" :disabled="importantBusy" @click="addImportant">
            加入
          </SoftButton>
        </div>
        <div v-if="importantMsg" class="hint">{{ importantMsg }}</div>
      </Card>

      <!-- 作业截止：读 eLearning `/dash` 的 To Do（用户 2026-11-12 要求摆在这里） -->
      <Card :padding="12" class="side-card assignment-card">
        <div class="side-head">
          <span class="h2">作业截止</span>
          <span class="spacer" />
          <SoftButton :height="24" :font-size="11" :disabled="assignmentBusy" @click="refreshAssignments">
            刷新
          </SoftButton>
          <Pill :text="`${assignments.length} 个待交`" :font-size="11.5" />
        </div>
        <div v-if="!assignments.length" class="muted empty">
          还没有作业数据。点下面的「同步」从 eLearning 拉一次（要先在 eLearning 页登录）。
        </div>
        <div
          v-for="item in assignments"
          :key="`${item.course}-${item.title}-${item.due}`"
          class="assign-row"
          :class="{ late: item.overdue, today: item.days_left === 0 }"
          :title="item.url ? '点一下在浏览器里打开这次作业' : ''"
          @click="openAssignment(item)"
        >
          <div class="assign-main">
            <b>{{ item.title }}</b>
            <span class="assign-meta">
              {{ item.course || "（未注明课程）" }} · 截止 {{ item.due.slice(5, 16) }}
            </span>
          </div>
          <span class="assign-left" :class="{ soon: item.minutes_left > 0 && item.minutes_left <= 1440 }">
            {{ dueText(item) }}
          </span>
          <!-- 已经交了就别再提醒（用户 2026-11-12） -->
          <button
            class="assign-done"
            title="标记已完成/已提交（不再提醒）"
            @click.stop="markAssignmentDone(item)"
          >
            ✓
          </button>
        </div>
        <div class="assign-foot">
          <SoftButton :height="28" :font-size="12" :disabled="assignmentBusy" @click="syncAssignments">
            {{ assignmentBusy ? "同步中…" : "同步 eLearning" }}
          </SoftButton>
          <span class="hint tiny">截止前 24 小时提醒一次</span>
        </div>
        <div v-if="assignmentMsg" class="hint">{{ assignmentMsg }}</div>
      </Card>
      </div>
    </div>

    <EventEditor
      :open="editorOpen"
      :initial="editorInitial"
      @close="editorOpen = false"
      @saved="load()"
    />

    <!-- 导入考试：先读出来给人看，确认后才写日历 -->
    <div v-if="examOpen" class="exam-mask" @click.self="examOpen = false">
      <div class="exam-card">
        <div class="exam-head">
          <b>导入考试安排</b>
          <span class="spacer" />
          <button class="exam-x" title="关闭" @click="examOpen = false">×</button>
        </div>
        <div class="hint">{{ examMsg }}</div>
        <div v-if="examItems.length" class="exam-list">
          <div v-for="item in examItems" :key="`${item.title}-${item.date}-${item.start_time}`" class="exam-row">
            <div class="exam-main">
              <b>{{ item.title }}</b>
              <span class="exam-meta">
                {{ item.date }} {{ item.start_time }}–{{ item.end_time || "?" }}
                <template v-if="item.location"> · {{ item.location }}</template>
                <template v-if="item.seat"> · 座位 {{ item.seat }}</template>
              </span>
            </div>
            <span class="exam-tag">{{ item.confidence === "table" ? "表格" : "正文" }}</span>
          </div>
        </div>
        <div class="exam-foot">
          <SoftButton :height="30" :font-size="12" @click="examOpen = false">取消</SoftButton>
          <SoftButton
            variant="primary"
            :height="30"
            :font-size="12"
            :disabled="examBusy || !examItems.length"
            @click="commitExams"
          >
            加入日历 + 重要日
          </SoftButton>
        </div>
      </div>
    </div>

    <!-- 删重要日：先确认 -->
    <ConfirmDialog
      v-model:open="importantConfirmOpen"
      title="确定删掉这个重要日吗？"
      :message="`「${pendingImportant?.title || '这一条'}」删除后无法恢复。`"
      confirm-text="确认删除"
      @confirm="doRemoveImportant"
    />
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
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
/* 日历内容自己的滚动区（周视图 14 节比卡片高，必须在卡片里滚） */
.calendar-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
}
.side-card {
  width: 300px;
  flex: none;
  gap: 6px;
}
/* 右侧竖排容器：当日 + 重要日两张卡，宽度固定 300，不抢日历的地方 */
.side-stack {
  width: 300px;
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}
.side-stack .side-card {
  width: auto;
}
/* 导入考试的弹层 */
.exam-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.32);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 80;
}
.exam-card {
  width: 560px;
  max-width: calc(100vw - 80px);
  max-height: 70vh;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 14px 16px;
  border-radius: 16px;
  background: var(--card);
  box-shadow: var(--shadow-pop, 0 12px 32px rgba(0, 0, 0, 0.25));
}
.exam-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
}
.exam-x {
  border: none;
  background: transparent;
  color: var(--muted);
  font-size: 15px;
  cursor: pointer;
  padding: 0 4px;
}
.exam-list {
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.exam-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 7px 9px;
  border: 1px solid var(--border-soft);
  border-radius: 10px;
}
.exam-main {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.exam-main b {
  font-size: 12.5px;
}
.exam-meta {
  font-size: 11px;
  color: var(--muted);
}
.exam-tag {
  margin-left: auto;
  flex: 0 0 auto;
  font-size: 10.5px;
  color: var(--muted);
}
.exam-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

/* 作业截止卡片 */
.assignment-card .assign-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 9px;
  cursor: pointer;
}
.assignment-card .assign-row:hover {
  background: var(--card-hover);
}
.assignment-card .assign-row.today {
  background: color-mix(in srgb, var(--accent) 9%, transparent);
}
.assignment-card .assign-row.late {
  opacity: 0.65;
}
.assign-main {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}
.assign-main b {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.assign-meta {
  font-size: 11px;
  color: var(--muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.assign-left {
  margin-left: auto;
  flex: 0 0 auto;
  font-size: 10.5px;
  line-height: 1.3;
  color: var(--accent);
  font-weight: 600;
  /* 「还有 2 天 3 小时」别折行 */
  white-space: nowrap;
  padding-top: 1px;
}
.assign-row.late .assign-left {
  color: var(--faint);
}
.assign-left.soon {
  color: var(--rose, #c0392b);
}
.assign-done {
  flex: 0 0 auto;
  width: 20px;
  height: 20px;
  border-radius: 999px;
  border: 1px solid var(--border-soft);
  background: transparent;
  color: var(--muted);
  font-size: 11px;
  line-height: 1;
  cursor: pointer;
  padding: 0;
}
.assign-done:hover {
  border-color: var(--accent);
  color: var(--accent);
}
.assign-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 6px;
}

/* 重要日（倒计时）卡片 */
.important-card {
  max-height: 300px;
  overflow: auto;
}
.important-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 8px;
  border-radius: 8px;
  font-size: 12px;
  border-left: 3px solid currentColor;
}
.imp-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  line-height: 1.3;
}
.imp-main b {
  font-weight: 620;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.imp-date {
  font-size: 10.5px;
  opacity: 0.75;
}
.imp-left {
  flex: none;
  font-size: 11px;
  font-weight: 620;
}
.imp-del {
  flex: none;
  border: 0;
  background: transparent;
  color: inherit;
  font-size: 14px;
  line-height: 1;
  cursor: pointer;
  opacity: 0.55;
}
.imp-del:hover {
  opacity: 1;
}
.important-add {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-top: 4px;
}
.important-add input {
  font-size: 11.5px;
  padding: 5px 8px;
}
/* 当日列表里每一行：左边「完成」小方框 + 标题/时间 */
.ev-row {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  min-width: 0;
}
.ev-check {
  flex: none;
  width: 17px;
  height: 17px;
  margin-top: 3px;
  padding: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1.2px solid var(--border-strong, var(--border));
  border-radius: 5px;
  background: transparent;
  color: var(--card);
  cursor: pointer;
}
.ev-check:hover {
  border-color: var(--accent);
}
.ev-check.on {
  background: var(--accent);
  border-color: var(--accent);
}
.ev-series {
  flex: none;
  height: 20px;
  padding: 0 6px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--muted);
  font-family: inherit;
  font-size: 10.5px;
  cursor: pointer;
}
.ev-series:hover {
  border-color: var(--accent);
  color: var(--accent);
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
.week-head {
  display: grid;
  /* 和 .week-body 用完全相同的列定义，否则表头会比格子宽、两边对不上 */
  grid-template-columns: 64px repeat(7, minmax(0, 1fr));
  gap: 2px;
  min-height: 34px;
}
/* 周视图主体：左侧时间刻度 + 7 列，高度按分钟换算（见 script 里的 PX_PER_MIN） */
.week-body {
  display: grid;
  grid-template-columns: 64px repeat(7, minmax(0, 1fr));
  gap: 2px;
  position: relative;
}
.scale-col {
  position: relative;
}
.scale-tick {
  position: absolute;
  right: 4px;
  display: flex;
  flex-direction: column;
  line-height: 1.15;
  font-size: 10px;
  color: var(--muted);
  transform: translateY(-2px);
  white-space: nowrap;
}
.scale-tick b {
  font-weight: 620;
  font-size: 10.5px;
}
.scale-tick i {
  font-style: normal;
  color: var(--faint);
}
.day-col {
  position: relative;
  border-radius: 7px;
  background: color-mix(in srgb, var(--card-alt) 45%, transparent);
  cursor: pointer;
  overflow: hidden;
}
.day-col:hover {
  background: color-mix(in srgb, var(--accent) 5%, transparent);
}
.day-col.today {
  background: color-mix(in srgb, var(--accent) 4%, transparent);
}
.day-col.is-selected {
  background: color-mix(in srgb, var(--accent) 8%, transparent);
  outline: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
}
.hour-line {
  position: absolute;
  left: 0;
  right: 0;
  height: 1px;
  background: color-mix(in srgb, var(--border-soft) 70%, transparent);
  pointer-events: none;
}
.event-block {
  position: absolute;
  z-index: 2;
  padding: 2px 5px;
  border-radius: 5px;
  font-size: 10.5px;
  line-height: 1.25;
  overflow: hidden;
  cursor: pointer;
  border-left: 3px solid currentColor;
}
.blk-time {
  display: block;
  font-size: 9.5px;
  font-weight: 620;
  opacity: 0.9;
}
.blk-title {
  display: block;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.day-head {
  font-size: 11.5px;
  color: var(--muted);
  text-align: center;
  padding: 3px 0;
  border: 0;
  background: transparent;
  border-radius: 7px;
  cursor: pointer;
  font-family: inherit;
}
.day-head:hover {
  background: color-mix(in srgb, var(--accent) 8%, transparent);
}
.day-head.today {
  color: var(--accent);
  font-weight: 650;
}
.day-head.selected {
  color: var(--accent);
  font-weight: 650;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}
.head-spacer {
  display: block;
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
