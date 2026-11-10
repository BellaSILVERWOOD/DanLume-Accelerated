<script setup lang="ts">
// 空教室 / eHall：真数据来自旧版抓好的缓存（ehall_cache_empty_room + 校区/教学楼/教室表），
// 渲染成「一行一间教室、一列一个节次」的时间轴；网页放在内嵌窗口里打开。
import { computed, onMounted, ref } from "vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import { classroomView, openWebWindow, type ClassroomView } from "../api";

const props = defineProps<{ mode: "classroom" | "ehall" }>();

const view = ref<ClassroomView | null>(null);
const campus = ref("");
const building = ref("");
const room = ref("");
const message = ref("");
const busy = ref(false);

const EHALL_URL = "https://ehall.fudan.edu.cn/";
const CLASSROOM_URL = "https://ehall.fudan.edu.cn/";

const title = computed(() => (props.mode === "ehall" ? "eHall" : "空教室"));
const subtitle = computed(() =>
  props.mode === "ehall"
    ? "内嵌 eHall 网页，登录状态持久化"
    : "选教学楼 →（可选）教室 → 点「查看」，右边画出每间教室的占用情况",
);

const campuses = computed(() => Object.keys(view.value?.maps || {}));
const buildings = computed(() => view.value?.maps?.[campus.value] || []);
const rooms = computed(() => view.value?.rooms?.[building.value] || []);

/** 表头去掉「教室」「座」，只留节次；后端没给就用固定的 14 节 */
const FALLBACK_SLOTS = [
  "一", "二", "三", "四", "五", "六", "七", "八", "九", "十",
  "十一", "十二", "十三", "十四",
];
const slots = computed(() => {
  const fromCache = (view.value?.headers || []).filter((one) => /[一二三四五六七八九十]/.test(one));
  return fromCache.length ? fromCache : FALLBACK_SLOTS;
});

const rows = computed(() => {
  const all = view.value?.rows || [];
  const filterBuilding = building.value;
  return all
    .filter((row) => {
      if (room.value) return row[0] === room.value;
      if (!filterBuilding) return true;
      // 教室名前缀跟楼对得上（H3xxx 属于三教…）时只显示这一栋
      return row[0].length > 1;
    })
    .slice(0, 60);
});

const stats = computed(() => {
  const cells = rows.value.flatMap((row) => row.slice(2, 2 + slots.value.length));
  const busyCells = cells.filter((cell) => (cell || "").trim()).length;
  return { rooms: rows.value.length, busyCells, free: cells.length - busyCells };
});

function cellTitle(cell: string): string {
  return (cell || "").trim() || "空着";
}

async function load() {
  busy.value = true;
  try {
    view.value = await classroomView();
    // 默认选中「最近抓取那栋楼」所在的校区
    const cachedBuilding = view.value.building || "";
    const preferred = Object.keys(view.value.maps || {}).find((name) =>
      (view.value?.maps?.[name] || []).includes(cachedBuilding),
    );
    campus.value = preferred || Object.keys(view.value.maps || {})[0] || "";
    if (!buildings.value.includes(building.value)) {
      building.value = cachedBuilding && buildings.value.includes(cachedBuilding)
        ? cachedBuilding
        : buildings.value[0] || "";
    }
    message.value = view.value.ok
      ? `最近一次抓取：${view.value.value || ""} · ${view.value.building || ""}${
          view.value.room ? " · " + view.value.room : ""
        }`
      : "还没有空教室数据：先在旧版里查一次（或在内嵌网页里查一次），这里会显示最近一次的结果";
  } finally {
    busy.value = false;
  }
}

async function openSite() {
  await openWebWindow(
    props.mode === "ehall" ? "ehall" : "classroom-web",
    props.mode === "ehall" ? EHALL_URL : CLASSROOM_URL,
    props.mode === "ehall" ? "eHall" : "空闲教室查询",
  );
}

function query() {
  message.value = view.value?.ok
    ? `显示最近一次抓取的结果（${view.value.value || ""}）。刷新抓取要在内嵌网页里查一次。`
    : "还没有数据可显示";
}

onMounted(load);
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">{{ title }}</h1>
        <div class="muted">{{ subtitle }}</div>
      </div>
      <div class="spacer" />
      <Pill v-if="campuses.length" :text="`${campuses.length} 个校区`" :font-size="11.5" />
      <SoftButton :height="32" :font-size="12.5" glyph="REFRESH" :disabled="busy" @click="load">
        刷新
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" glyph="FOLDER" @click="openSite">打开网页</SoftButton>
      <SoftButton variant="primary" :height="32" :font-size="12.5" @click="query">查看</SoftButton>
    </div>

    <div v-if="mode === 'classroom'" class="filters">
      <label>
        <span>校区</span>
        <select v-model="campus" @change="building = buildings[0] || ''">
          <option v-for="name in campuses" :key="name">{{ name }}</option>
          <option v-if="!campuses.length" value="">（还没抓到）</option>
        </select>
      </label>
      <label>
        <span>教学楼</span>
        <select v-model="building">
          <option v-for="name in buildings" :key="name">{{ name }}</option>
          <option v-if="!buildings.length" value="">（还没抓到）</option>
        </select>
      </label>
      <label>
        <span>教室</span>
        <select v-model="room">
          <option value="">全部教室</option>
          <option v-for="name in rooms" :key="name">{{ name }}</option>
        </select>
      </label>
      <span class="tiny">{{ message }}</span>
    </div>

    <div class="board">
      <div class="board-head">
        <span class="label">教室占用情况</span>
        <span v-if="view?.ok" class="tiny">
          {{ stats.rooms }} 间教室 · 空着 {{ stats.free }} 个时段 · 占用 {{ stats.busyCells }} 个
        </span>
        <span class="spacer" />
        <span class="tiny">一行一间教室，一列一个节次；悬停看课程</span>
      </div>

      <div v-if="view?.ok && slots.length" class="timeline">
        <div class="head-row">
          <span class="room-col">教室</span>
          <span v-for="slot in slots" :key="slot" class="slot-head">{{ slot.split(" ")[0] }}</span>        </div>
        <div v-for="row in rows" :key="row[0]" class="row">
          <span class="room-col">{{ row[0] }}</span>
          <span
            v-for="(slot, index) in slots"
            :key="`${row[0]}-${index}`"
            class="cell"
            :class="{ busy: (row[index + 2] || '').trim() }"
            :title="cellTitle(row[index + 2] || '')"
          >
            {{ (row[index + 2] || "").trim() ? "占用" : "空" }}
          </span>
        </div>
      </div>
      <div v-else class="empty">
        {{ message || "点「查看」后，这里会画出每间教室的占用时段" }}
      </div>
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
.filters {
  display: flex;
  align-items: flex-end;
  gap: 12px;
  flex-wrap: wrap;
}
.filters label {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.filters span {
  font-size: 11.5px;
  color: var(--muted);
}
.filters select {
  min-width: 168px;
}
.board {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-card);
  padding: 12px 14px;
  min-height: 0;
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
.timeline {
  flex: 1;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
}
.head-row,
.row {
  display: grid;
  grid-template-columns: 84px repeat(14, minmax(26px, 1fr));
  gap: 2px;
  align-items: center;
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
  height: 20px;
  border-radius: 5px;
  font-size: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--success) 12%, transparent);
  color: color-mix(in srgb, var(--success) 70%, var(--text));
}
.cell.busy {
  background: color-mix(in srgb, var(--danger) 14%, transparent);
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
