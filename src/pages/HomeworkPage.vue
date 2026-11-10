<script setup lang="ts">
// 作业：读 eLearning 缓存清单（旧版存的 homework_assignments），
// 「打开」按 索引 → 附件名 → 作业名 → 同课程编号 找本地文件；截止时间一键写进日程。
import { computed, onMounted, ref } from "vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import IconBadge from "../components/IconBadge.vue";
import {
  homeworkItems,
  homeworkWriteDeadlines,
  openPath,
  openWebWindow,
  type HomeworkCourse,
} from "../api";

const courses = ref<HomeworkCourse[]>([]);
const total = ref(0);
const downloads = ref("");
const keyword = ref("");
const message = ref("");
const busy = ref(false);

const visible = computed(() => {
  const text = keyword.value.trim().toLowerCase();
  return courses.value
    .map((course) => ({
      ...course,
      items: course.items.filter(
        (item) =>
          !text ||
          course.course.toLowerCase().includes(text) ||
          item.title.toLowerCase().includes(text),
      ),
    }))
    .filter((course) => course.items.length);
});

async function load() {
  busy.value = true;
  message.value = "正在读取作业清单…";
  try {
    const result = await homeworkItems();
    courses.value = result.courses || [];
    total.value = result.total || 0;
    downloads.value = result.downloads || "";
    message.value = total.value
      ? `已读到 ${courses.value.length} 门课、${total.value} 个作业`
      : "还没有读到作业：旧版里点一次「读取我的课程」，或者这里用 eLearning 网页读一次";
  } catch (err) {
    courses.value = [];
    total.value = 0;
    message.value = `读取作业失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

async function writeDeadlines() {
  busy.value = true;
  message.value = "正在把截止时间写进日程…";
  try {
    const result = await homeworkWriteDeadlines(true);
    message.value = result.ok
      ? `已写入 ${result.added} 条、更新 ${result.updated} 条（跳过 ${result.skipped} 条）`
      : result.error;
  } finally {
    busy.value = false;
  }
}

async function openElearning() {
  await openWebWindow("elearning", "https://elearning.fudan.edu.cn/", "eLearning");
}

function when(due: string): string {
  return (due || "").replace("T", " ").slice(0, 16);
}

onMounted(load);
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">作业</h1>
        <div class="muted">eLearning 的课程作业与截止时间 · 调试标记 R2</div>
      </div>
      <div class="spacer" />
      <Pill :text="`${courses.length} 门课 · ${total} 个作业`" :font-size="11.5" />
      <SoftButton :height="32" :font-size="12.5" glyph="REFRESH" @click="load">刷新</SoftButton>
      <SoftButton :height="32" :font-size="12.5" glyph="FOLDER" @click="openPath(downloads)">
        打开下载目录
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" @click="openElearning">看网页</SoftButton>
      <SoftButton variant="primary" :height="32" :font-size="12.5" @click="writeDeadlines">
        截止写入日程
      </SoftButton>
    </div>

    <div class="bar">
      <input v-model="keyword" type="search" placeholder="搜索课程或作业名…" />
      <span v-if="message" class="muted">{{ message }}</span>
    </div>

    <div class="list">
      <div v-for="course in visible" :key="course.course" class="course">
        <div class="course-head">
          <span class="course-name">{{ course.course || "（未分组）" }}</span>
          <Pill :text="`${course.items.length} 个作业`" :font-size="11" />
        </div>
        <div v-for="item in course.items" :key="`${course.course}-${item.title}`" class="row">
          <span class="title">· {{ item.title }}</span>
          <span class="due">{{ when(item.due) }}</span>
          <span class="actions">
            <button
              v-for="file in item.files"
              :key="file"
              class="chip"
              :title="file"
              @click="openPath(file)"
            >
              <IconBadge glyph="DOCUMENT" tone="teal" :size="18" />
              {{ file.split("\\").pop() }}
            </button>
            <span v-if="!item.files.length" class="none">本地还没有文件</span>
          </span>
        </div>
      </div>
      <div v-if="!visible.length" class="empty">{{ message || "没有作业" }}</div>
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
.bar {
  display: flex;
  align-items: center;
  gap: 10px;
}
.bar input {
  width: 320px;
}
.list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.course {
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-card);
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.course-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.course-name {
  font-size: 14px;
  font-weight: 640;
}
.row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 0;
  border-top: 1px dashed var(--border-soft);
}
.row:first-of-type {
  border-top: none;
}
.title {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  color: var(--text-soft);
  overflow-wrap: anywhere;
}
.due {
  font-size: 11.5px;
  color: var(--muted);
  white-space: nowrap;
}
.actions {
  display: flex;
  align-items: center;
  gap: 5px;
  flex-wrap: wrap;
  max-width: 46%;
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 2px 8px 2px 3px;
  border-radius: 8px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  font-family: inherit;
  font-size: 11.5px;
  color: var(--text);
  cursor: pointer;
  max-width: 240px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.chip:hover {
  background: var(--card-hover);
}
.none {
  font-size: 11.5px;
  color: var(--faint);
}
.empty {
  font-size: 12.5px;
  color: var(--muted);
  padding: 8px 2px;
}
</style>
