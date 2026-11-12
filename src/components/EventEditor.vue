<script setup lang="ts">
// 新建 / 编辑日程（对应旧版 dialogs.py::EventDialog）。
// 重复规则只在「新建」或「编辑整串」时生效：点开的是重复日程的某一次时，
// 保存会作用到整串，但不会把起始日期改成那一天。
import { computed, ref, watch } from "vue";
import SoftButton from "./SoftButton.vue";
import ConfirmDialog from "./ConfirmDialog.vue";
import { addEvent, deleteEvent, updateEvent, type EventRow } from "../api";

const props = withDefaults(
  defineProps<{
    open: boolean;
    /** 新建时只要 date；编辑时给整行（重复日程给的是展开出来的那一天） */
    initial?: Partial<EventRow> | null;
  }>(),
  { initial: null },
);

const emit = defineEmits<{
  (event: "close"): void;
  (event: "saved"): void;
}>();

const REMIND_OPTIONS: { value: number | null; label: string }[] = [
  { value: null, label: "不提醒" },
  { value: 0, label: "日程开始时" },
  { value: 5, label: "提前 5 分钟" },
  { value: 15, label: "提前 15 分钟" },
  { value: 30, label: "提前 30 分钟" },
  { value: 60, label: "提前 1 小时" },
  { value: 1440, label: "提前 1 天" },
];

const REPEAT_OPTIONS: { value: string; label: string }[] = [
  { value: "", label: "不重复" },
  { value: "daily", label: "每天" },
  { value: "weekly", label: "每周（同一天）" },
  { value: "monthly", label: "每月（同一日）" },
  { value: "yearly", label: "每年（同月同日）" },
];

const COLORS: { value: string; label: string; dot: string }[] = [
  { value: "violet", label: "紫", dot: "#AF52DE" },
  { value: "teal", label: "青", dot: "#30B0C7" },
  { value: "amber", label: "橙", dot: "#FF9500" },
  { value: "rose", label: "粉", dot: "#FF2D55" },
  { value: "sky", label: "蓝", dot: "#007AFF" },
];

const title = ref("");
const description = ref("");
const date = ref("");
const startTime = ref("09:00");
const endTime = ref("10:00");
const remind = ref<number | null>(15);
const repeat = ref("");
const color = ref("violet");
const error = ref("");
const busy = ref(false);

const editing = computed(() => Number(props.initial?.id || 0) > 0);
/** 点开的是重复日程的某一次：日期字段只读，避免把整串的起始日改掉 */
const occurrence = computed(() => !!props.initial?.repeat_of);

function isoToday(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(
    now.getDate(),
  ).padStart(2, "0")}`;
}

function clockAfter(minutes: number): string {
  const now = new Date();
  const at = new Date(now.getTime() + minutes * 60000);
  return `${String(at.getHours()).padStart(2, "0")}:${String(at.getMinutes()).padStart(2, "0")}`;
}

function reset() {
  const initial = props.initial || {};
  error.value = "";
  busy.value = false;
  if (!initial.id) {
    // 新建：默认「下一个整点开始，一小时」
    title.value = "";
    description.value = "";
    date.value = initial.date || isoToday();
    const now = new Date();
    const hour = Math.min(23, now.getHours() + 1);
    startTime.value = `${String(hour).padStart(2, "0")}:${now.getMinutes() < 45 ? "00" : "30"}`;
    endTime.value = clockAfter(60);
    remind.value = 15;
    repeat.value = "";
    color.value = "violet";
    return;
  }
  title.value = initial.title || "";
  description.value = initial.description || "";
  date.value = (initial.date || isoToday()).slice(0, 10);
  startTime.value = (initial.start_time || "09:00").slice(0, 5);
  endTime.value = (initial.end_time || "10:00").slice(0, 5);
  remind.value = initial.remind_minutes ?? null;
  repeat.value = initial.repeat || "";
  color.value = initial.color || "violet";
}

watch(
  () => [props.open, props.initial] as const,
  () => {
    if (props.open) reset();
  },
  { immediate: true },
);

async function save() {
  if (busy.value) return;
  if (!title.value.trim()) {
    error.value = "先写个标题";
    return;
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date.value)) {
    error.value = "日期要 YYYY-MM-DD";
    return;
  }
  if (endTime.value <= startTime.value) {
    error.value = "结束时间要晚于开始时间";
    return;
  }
  busy.value = true;
  error.value = "";
  const payload: Record<string, unknown> = {
    title: title.value.trim(),
    description: description.value,
    start_time: startTime.value,
    end_time: endTime.value,
    remind_minutes: remind.value,
    color: color.value,
    repeat: repeat.value,
  };
  if (remind.value === null) payload.clear_remind = true;
  if (!occurrence.value) payload.date = date.value;
  try {
    if (editing.value) {
      await updateEvent(Number(props.initial?.id), payload);
    } else {
      await addEvent(payload);
    }
    busy.value = false;
    emit("saved");
    emit("close");
  } catch (err) {
    busy.value = false;
    error.value = String(err);
  }
}

/** 删除前先问一句（用户 2026-11-11：删日程要弹「删除后无法恢复」） */
const confirmOpen = ref(false);

function remove() {
  if (busy.value) return;
  confirmOpen.value = true;
}

async function doRemove() {
  if (busy.value) return;
  busy.value = true;
  try {
    await deleteEvent(Number(props.initial?.id));
    busy.value = false;
    emit("saved");
    emit("close");
  } catch (err) {
    busy.value = false;
    error.value = String(err);
  }
}
</script>

<template>
  <div v-if="open" class="mask" @click.self="emit('close')">
    <div class="dialog">
      <div class="head">
        <div class="titles">
          <h2 class="h2">{{ editing ? "编辑日程" : "新建日程" }}</h2>
          <div class="muted small">
            {{ occurrence
              ? "这是重复日程的一次：保存会作用到整串（起始日期不变）"
              : editing
                ? "改完提醒时间会自动重算"
                : "标题 + 时间就够了，其它可以留空" }}
          </div>
        </div>
      </div>

      <label class="field">
        <span class="label">标题</span>
        <input v-model="title" class="input" placeholder="例如：项目周会 / 微积分A" maxlength="80" />
      </label>

      <div class="row">
        <label class="field grow">
          <span class="label">日期</span>
          <input v-model="date" class="input" type="date" :disabled="occurrence" />
        </label>
        <label class="field">
          <span class="label">开始</span>
          <input v-model="startTime" class="input" type="time" />
        </label>
        <label class="field">
          <span class="label">结束</span>
          <input v-model="endTime" class="input" type="time" />
        </label>
      </div>

      <div class="row">
        <label class="field grow">
          <span class="label">提醒</span>
          <select v-model="remind" class="input">
            <option v-for="item in REMIND_OPTIONS" :key="String(item.value)" :value="item.value">
              {{ item.label }}
            </option>
          </select>
        </label>
        <label class="field grow">
          <span class="label">重复</span>
          <select v-model="repeat" class="input">
            <option v-for="item in REPEAT_OPTIONS" :key="item.value" :value="item.value">
              {{ item.label }}
            </option>
          </select>
        </label>
        <div class="field">
          <span class="label">颜色</span>
          <div class="dots">
            <button
              v-for="item in COLORS"
              :key="item.value"
              class="dot"
              :class="{ on: color === item.value }"
              :title="item.label"
              :style="{ background: item.dot }"
              @click="color = item.value"
            />
          </div>
        </div>
      </div>

      <label class="field">
        <span class="label">备注</span>
        <textarea
          v-model="description"
          class="input area"
          rows="3"
          placeholder="可选：地点、要带什么、交付物"
        />
      </label>

      <div v-if="error" class="error">{{ error }}</div>

      <div class="foot">
        <SoftButton v-if="editing" variant="danger" :height="32" :font-size="12.5" @click="remove">
          删除
        </SoftButton>
        <span class="spacer" />
        <SoftButton :height="32" :font-size="12.5" @click="emit('close')">取消</SoftButton>
        <SoftButton
          variant="primary"
          :height="32"
          :font-size="12.5"
          glyph="CHECK"
          :disabled="busy"
          @click="save"
        >
          保存
        </SoftButton>
      </div>
    </div>

    <!-- 删日程：先确认（删了就找不回来） -->
    <ConfirmDialog
      v-model:open="confirmOpen"
      title="确定删除这条日程吗？"
      :message="`「${title || '这条日程'}」删除后无法恢复。`"
      confirm-text="确认删除"
      @confirm="doRemove"
    />
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  background: rgba(12, 16, 20, 0.42);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 70;
}
.dialog {
  width: 560px;
  max-width: calc(100vw - 48px);
  max-height: calc(100vh - 64px);
  overflow: auto;
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 16px;
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  box-shadow: 0 24px 64px rgba(0, 0, 0, 0.3);
}
.head .muted.small {
  font-size: 11.5px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 5px;
  min-width: 0;
}
.field.grow {
  flex: 1;
}
.label {
  font-size: 11.5px;
  color: var(--muted);
}
.input {
  height: 34px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: 9px;
  background: var(--card-alt);
  color: var(--text);
  font-family: inherit;
  font-size: 13px;
  min-width: 0;
}
.input:disabled {
  opacity: 0.6;
}
.input.area {
  height: auto;
  padding: 8px 10px;
  resize: vertical;
  line-height: 1.55;
}
.row {
  display: flex;
  gap: 10px;
  align-items: flex-end;
}
.dots {
  display: flex;
  gap: 7px;
  height: 34px;
  align-items: center;
}
.dot {
  width: 18px;
  height: 18px;
  border-radius: 999px;
  border: 2px solid transparent;
  cursor: pointer;
  padding: 0;
  outline: none;
}
.dot.on {
  border-color: var(--text);
}
.error {
  font-size: 12px;
  color: var(--danger, #d33);
}
.foot {
  display: flex;
  align-items: center;
  gap: 8px;
}
.spacer {
  flex: 1;
}
</style>
