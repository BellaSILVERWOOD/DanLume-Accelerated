<script setup lang="ts">
// 空教室（对应旧版 page_classroom.py）：选教学楼 →（可选）教室 → 查询 → 画时间轴。
//
// 用户 2026-11-11：「空教室页面就是空教室。ehall 的网页只要放在 ehall 页面里面就可以了。」
// 所以这里**不显示**学校网页：查询时把「空闲教室查询」页开在窗口之外（看不见），
// 由 Rust 在页面里选好校区/教学楼/教室、点「查询」，再把表格读回来画成时间轴。
// 旧版 `classroom_show_web=0` 时也是这个效果（网页收起来，抓取照样跑）。
import { computed, onMounted, ref } from "vue";
import Card from "../components/Card.vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import {
  classroomQuery,
  classroomScrape,
  classroomView,
  settingsAll,
  settingsSave,
  type ClassroomRoom,
  type ClassroomView,
} from "../api";

const EHALL_HOME = "https://ehall.fudan.edu.cn/";
const URL_KEY = "ehall_empty_room_url";

const view = ref<ClassroomView | null>(null);
const rooms = ref<ClassroomRoom[]>([]);
const periods = ref<string[]>([]);
const campus = ref("");
const building = ref("");
const room = ref("");
const message = ref("");
const busy = ref(false);
const url = ref(EHALL_HOME);
const urlDraft = ref(EHALL_HOME);

const campuses = computed(() => Object.keys(view.value?.maps || {}));
const buildings = computed(() => view.value?.maps?.[campus.value] || []);
const roomOptions = computed(() => view.value?.rooms?.[building.value] || []);

/** 表头：节次名（后端没给就用「第 N 节」） */
const slotLabels = computed(() => {
  const fromCache = (periods.value || []).filter((one) => one && one.trim());
  if (fromCache.length) return fromCache;
  const headers = (view.value?.headers || []).filter((one) => /[一二三四五六七八九十\d]/.test(one));
  if (headers.length) return headers;
  return Array.from({ length: 14 }, (_, index) => `第 ${index + 1} 节`);
});

/** 表头只写「窄得下」的那一段：`08:00-08:45` → `08:00`，`第 3 节` 原样 */
function slotHead(label: string): string {
  const text = (label || "").trim();
  if (!text) return "";
  const dash = text.search(/[-–—~]/);
  if (dash > 0) return text.slice(0, dash).trim();
  return text;
}

/** 只显示选中的那间教室 */
const shown = computed(() => {
  if (!rooms.value.length) return [];
  if (room.value) {
    const hit = rooms.value.filter((one) => one.name === room.value);
    if (hit.length) return hit;
  }
  return rooms.value;
});

const stats = computed(() => {
  let busyCells = 0;
  let free = 0;
  for (const one of shown.value) {
    for (const cell of one.cells || []) {
      if (cell.occupied) busyCells += 1;
      else free += 1;
    }
  }
  return { rooms: shown.value.length, busyCells, free };
});

function cellText(cell: { label?: string; raw?: string; occupied?: boolean }): string {
  if (!cell?.occupied) return "空";
  return (cell.label || cell.raw || "占用").slice(0, 24);
}

function applyRadio(data: ClassroomView) {
  const cachedBuilding = data.building || "";
  const preferred = Object.keys(data.maps || {}).find((name) =>
    (data.maps?.[name] || []).includes(cachedBuilding),
  );
  if (!campus.value) campus.value = preferred || Object.keys(data.maps || {})[0] || "";
  if (!building.value) building.value = cachedBuilding || buildings.value[0] || "";
}

async function load() {
  busy.value = true;
  try {
    const data = await classroomView();
    view.value = data;
    rooms.value = data.grid || [];
    periods.value = data.periods || [];
    applyRadio(data);
    if (data.ok) {
      message.value = `最近一次查询：${data.value || ""}${
        data.building ? " · " + data.building : ""
      }${data.room ? " · " + data.room : ""}`;
    } else {
      message.value = "还没有空教室数据：选好校区/教学楼后点「查询」";
    }
  } finally {
    busy.value = false;
  }
}

async function loadSettings() {
  try {
    const all = await settingsAll();
    const values: Record<string, string> = all.values || {};
    // 地址优先级与旧版一致：手填的查询页 → 仪表板「空教室」卡的取数地址（旧版
    // `ehall_bridge._source_url("empty_room")` 就是它）→ eHall 首页
    const stored = (values[URL_KEY] || values["ehall_json_empty_room"] || "").trim();
    url.value = stored || EHALL_HOME;
    urlDraft.value = url.value;
  } catch {
    url.value = EHALL_HOME;
    urlDraft.value = EHALL_HOME;
  }
}

async function saveHome() {
  const current = (urlDraft.value || "").trim() || url.value;
  if (!current) return;
  url.value = current;
  await settingsSave({ [URL_KEY]: current, ehall_json_empty_room: current });
  message.value = "已记住这个查询页地址，下次直接用它查";
}

/** 查询：后台页面选条件 → 点「查询」→ 读表格 */
async function query() {
  if (busy.value) return;
  if (!building.value && !campuses.value.length) {
    message.value = "还没读到校区/教学楼：先点「设为首页」填查询页地址，再查一次";
    return;
  }
  busy.value = true;
  message.value = `正在查询 ${campus.value || ""} ${building.value || ""}${
    room.value ? " " + room.value : ""
  }…`;
  try {
    const result = await classroomQuery(campus.value, building.value, room.value, url.value);
    if (result.ok && result.rooms?.length) {
      rooms.value = result.rooms;
      periods.value = result.periods || [];
      message.value = `${result.value || "查到结果"}（${result.building || campus.value || "全部"}）`;
      // 顺手刷新下拉（第一次查询后才知道有哪些楼/教室）
      view.value = await classroomView();
      applyRadio(view.value);
    } else {
      message.value = result.error || "没查到教室表：确认查询页地址、并在网页里登录 eHall";
    }
  } catch (err) {
    message.value = `查询失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

/** 只重新读一次当前后台页面（不重新选条件） */
async function scrape() {
  if (busy.value) return;
  busy.value = true;
  message.value = "正在读页面…";
  try {
    const result = await classroomScrape(building.value, room.value);
    if (result.ok && result.rooms?.length) {
      rooms.value = result.rooms;
      periods.value = result.periods || [];
      message.value = `${result.value || "抓到结果"}（${result.building || "全部教学楼"}）`;
    } else {
      message.value = result.error || "页面上没找到教室表";
    }
  } catch (err) {
    message.value = `抓取失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  await loadSettings();
  await load();
});
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">空教室</h1>
        <div class="muted">
          {{ message || "选校区 → 教学楼 →（可选）教室 → 点「查询」，右边画出每间教室的占用情况" }}
        </div>
      </div>
      <div class="spacer" />
      <Pill v-if="rooms.length" :text="`${rooms.length} 间教室`" :font-size="11.5" />
      <SoftButton :height="32" :font-size="12.5" :disabled="busy" @click="scrape">重新读取</SoftButton>
      <SoftButton :height="32" :font-size="12.5" @click="saveHome">设为首页</SoftButton>
      <SoftButton :height="32" :font-size="12.5" :disabled="busy" @click="load">刷新</SoftButton>
      <SoftButton variant="primary" :height="32" :font-size="12.5" :disabled="busy" @click="query">
        查询
      </SoftButton>
    </div>

    <div class="url-row">
      <input v-model="urlDraft" type="text" placeholder="空闲教室查询页面的地址" @keyup.enter="saveHome" />
    </div>

    <Card :padding="12" class="board">
      <div class="board-head">
        <span class="label">教室占用情况</span>
        <span v-if="shown.length" class="tiny">
          {{ stats.rooms }} 间 · 空着 {{ stats.free }} 个时段 · 占用 {{ stats.busyCells }} 个
        </span>
        <span class="spacer" />
        <span class="tiny">一行一间教室，一列一个节次；悬停看课程</span>
      </div>

      <div class="filters">
        <label>
          <span>校区</span>
          <select v-model="campus" @change="building = buildings[0] || ''">
            <option v-for="name in campuses" :key="name">{{ name }}</option>
            <option v-if="!campuses.length" value="">（还没读到）</option>
          </select>
        </label>
        <label>
          <span>教学楼</span>
          <select v-model="building">
            <option v-for="name in buildings" :key="name">{{ name }}</option>
            <option v-if="!buildings.length" value="">（还没读到）</option>
          </select>
        </label>
        <label>
          <span>教室</span>
          <select v-model="room">
            <option value="">全部教室</option>
            <option v-for="name in roomOptions" :key="name">{{ name }}</option>
          </select>
        </label>
      </div>

      <div v-if="shown.length" class="timeline">
        <div class="grid-row head">
          <span class="room-col">教室</span>
          <span v-for="(slot, index) in slotLabels" :key="`h-${index}`" class="slot-head">
            {{ slotHead(slot) }}
          </span>
        </div>
        <div v-for="one in shown" :key="one.name" class="grid-row">
          <span class="room-col">{{ one.name }}</span>
          <span
            v-for="(cell, index) in one.cells"
            :key="`${one.name}-${index}`"
            class="cell"
            :class="{ busy: cell.occupied }"
            :title="cell.occupied ? `${cell.label || ''} ${cell.raw || ''}` : '空着'"
          >
            {{ cellText(cell) }}
          </span>
        </div>
      </div>
      <div v-else class="empty">
        {{ message || "选好校区/教学楼后点「查询」，这里会画出每间教室的占用时段" }}
      </div>
    </Card>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
  min-height: 0;
}
.url-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.url-row input {
  flex: 1;
  height: 32px;
}
.board {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.board-head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border-soft);
}
.label {
  font-size: 13px;
  font-weight: 640;
  color: var(--text-soft);
}
.filters {
  display: flex;
  align-items: flex-end;
  gap: 10px;
  flex-wrap: wrap;
}
.filters label {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.filters span {
  font-size: 11.5px;
  color: var(--muted);
}
.filters select {
  min-width: 148px;
  height: 28px;
  flex: 0 0 auto;
}
.timeline {
  flex: 1;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
}
.grid-row {
  display: grid;
  grid-template-columns: 84px repeat(14, minmax(26px, 1fr));
  gap: 2px;
  align-items: center;
}
.grid-row.head {
  position: sticky;
  top: 0;
  background: var(--card);
  padding-bottom: 2px;
  z-index: 1;
}
.slot-head {
  font-size: 10.5px;
  color: var(--muted);
  text-align: center;
}
.room-col {
  font-size: 11.5px;
  color: var(--text-soft);
  white-space: nowrap;
}
.cell {
  height: 22px;
  border-radius: 5px;
  font-size: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  white-space: nowrap;
  padding: 0 2px;
  background: color-mix(in srgb, var(--success) 12%, transparent);
  color: color-mix(in srgb, var(--success) 70%, var(--text));
}
/* 有课的格子：课程名从左边开始读，放不下就在**末尾**省略。
   以前是 flex 居中 + overflow:hidden，字比格子宽时两头都被切掉，
   屏幕上只剩中间那两三个字（「化教」之类），根本认不出是哪门课。 */
.cell.busy {
  display: block;
  line-height: 22px;
  text-align: left;
  text-overflow: ellipsis;
  padding: 0 5px;
  background: color-mix(in srgb, var(--danger) 16%, transparent);
  color: color-mix(in srgb, var(--danger) 80%, var(--text));
}
.empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 12.5px;
  color: var(--muted);
  text-align: center;
  line-height: 1.7;
  padding: 20px;
}
.spacer {
  flex: 1;
}
</style>
