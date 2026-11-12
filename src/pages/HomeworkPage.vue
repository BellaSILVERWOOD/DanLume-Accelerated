<script setup lang="ts">
// 作业：内嵌 eLearning 网页（对应旧版 page_homework.py 的 QStackedWidget）+ 课程作业清单。
//
// 旧版是「网页页 / 列表页」两页切换，默认停在网页页（作业要提交就得在网页里点），
// 这里用同样的做法：`view = 'web'` 时贴一个子 Webview，`view = 'list'` 时显示清单。
// 「打开」按 索引 → 附件名 → 作业名 → 同课程编号 找本地文件；截止时间一键写进日程。
import { computed, onMounted, ref } from "vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import IconBadge from "../components/IconBadge.vue";
import EmbedWebview from "../components/EmbedWebview.vue";
import {
  homeworkClassify,
  homeworkItems,
  homeworkSync,
  homeworkUnclassified,
  homeworkWriteDeadlines,
  openPath,
  type HomeworkCourse,
  type Unclassified,
} from "../api";

// 进 `/login` 而不是 `/`：已经登录时它直接跳回控制面板；没登录时它就是统一身份认证
// 的入口（注入的自动填表脚本会把学号密码填好并点登录）。`/` 在未登录时只是欢迎页，
// 已登录时却停在欢迎页上（用户看到「欢迎使用 eLearning」会以为没登进去）。
const ELEARNING_URL = "https://elearning.fudan.edu.cn/login";

const courses = ref<HomeworkCourse[]>([]);
const total = ref(0);
const downloads = ref("");
const keyword = ref("");
const message = ref("");
const busy = ref(false);
const syncedAt = ref("");
/** 和旧版一样：默认看网页（交作业、看课件都在网页里），点「看列表」才切到清单 */
const view = ref<"web" | "list">("web");

/**
 * 没归类的文件：从浏览器里手动下载的那些（`03《物种起源》.pdf`、Canvas 用附件 id 当
 * 名字的 `34FAF30F@…png`）不在官方文件清单里，自动整理认不出来，只能人工指定课程。
 */
const unclassified = ref<Unclassified | null>(null);
const coursePick = ref<Record<string, string>>({});
const classifying = ref("");

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
    void loadUnclassified();
  }
}

/** 下载目录根下还没进课程文件夹的文件 */
async function loadUnclassified() {
  try {
    const result = await homeworkUnclassified();
    unclassified.value = result;
    // 默认选第一个课程目录，省得每个文件都要点一次
    const next = { ...coursePick.value };
    for (const file of result.files || []) {
      if (!next[file.name] && result.courses.length) {
        next[file.name] = result.courses[0];
      }
    }
    coursePick.value = next;
  } catch {
    unclassified.value = null;
  }
}

/** 把某个文件挪进选中的课程目录 */
async function classify(file: string) {
  const root = unclassified.value?.root || downloads.value;
  const course = coursePick.value[file];
  if (!root || !course) return;
  classifying.value = file;
  try {
    const path = `${root.replace(/[\\/]$/, "")}\\${file}`;
    const result = await homeworkClassify(path, course);
    message.value = result.deduped
      ? `「${file}」在 ${course} 里已有一份同样大小的，根目录这份已删掉`
      : `已把「${file}」挪进 ${course}`;
    await load();
  } catch (err) {
    message.value = `归类失败：${String(err)}`;
  } finally {
    classifying.value = "";
  }
}

async function writeDeadlines() {
  try {
    const result = await homeworkWriteDeadlines(true);
    message.value = result.ok
      ? `已写入 ${result.added} 条、更新 ${result.updated} 条（跳过 ${result.skipped} 条）`
      : result.error;
  } finally {
    busy.value = false;
  }
}

/**
 * 「同步」：真去 eLearning 拉一次 To Do 再刷新列表。
 *
 * 以前这里的按钮只读缓存（旧版写进 `homework_assignments` 的那份），
 * 缓存过期就一直是旧数据——作业页看起来像「空壳」就是这个原因。
 */
async function sync() {
  if (busy.value) return;
  busy.value = true;
  message.value = "正在从 eLearning 同步作业…";
  try {
    const result = await homeworkSync();
    if (!result.ok) {
      message.value = `同步失败：${result.error}`;
      // 读不到多半是没登录：直接切到内嵌网页，让人在里面登录（旧版也是这个提示）
      if (/登录|会话|cookie/i.test(result.error || "")) openElearning();
      return;
    }
    syncedAt.value = new Date().toLocaleTimeString();
    await load();
    message.value = total.value
      ? `已同步：${courses.value.length} 门课、${total.value} 个待办（${syncedAt.value}）`
      : "同步成功，但 eLearning 里暂时没有待办作业";
  } catch (err) {
    message.value = `同步失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

async function openElearning() {
  // 旧版 open_login_page()：切回网页视图，手动登录 / 交作业都在这里
  view.value = "web";
  message.value = "已在网页里打开 eLearning：登录后可以交作业，切回「看列表」再点「同步」刷新清单";
}

function toggleView() {
  view.value = view.value === "web" ? "list" : "web";
  if (view.value === "list") {
    if (!courses.value.length) void load();
    else void loadUnclassified();
  }
}

function when(due: string): string {
  return (due || "").replace("T", " ").slice(0, 16);
}

function humanSize(size: number): string {
  if (size >= 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  if (size >= 1024) return `${Math.round(size / 1024)} KB`;
  return `${size} B`;
}

onMounted(load);
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">作业</h1>
        <div class="muted">内嵌 eLearning 网页：交作业、看课件在这个窗口里，不弹窗</div>
      </div>
      <div class="spacer" />
      <Pill :text="`${courses.length} 门课 · ${total} 个作业`" :font-size="11.5" />
      <SoftButton :height="32" :font-size="12.5" glyph="REFRESH" :disabled="busy" @click="sync">
        同步
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" glyph="FOLDER" @click="openPath(downloads)">
        打开下载目录
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" @click="toggleView">
        {{ view === "web" ? "看列表" : "看网页" }}
      </SoftButton>
      <SoftButton variant="primary" :height="32" :font-size="12.5" @click="writeDeadlines">
        截止写入日程
      </SoftButton>
    </div>

    <div class="bar">
      <input v-model="keyword" type="search" placeholder="搜索课程或作业名…" />
      <span v-if="message" class="muted">{{ message }}</span>
    </div>

    <div v-if="view === 'web'" class="web-host">
      <EmbedWebview :key="'elearn'" kind="elearning" :url="ELEARNING_URL" home-label="eLearning 首页" />
    </div>

    <div v-else class="list">
      <div v-if="unclassified && unclassified.files.length" class="unfiled">
        <div class="unfiled-head">
          <span class="course-name">还没归类的文件</span>
          <Pill :text="`${unclassified.files.length} 个`" :font-size="11" />
          <span class="spacer" />
          <span class="none">{{ unclassified.root }}</span>
        </div>
        <div class="none">
          这些文件直接躺在下载目录下（多半是从浏览器里手动下的，官方文件清单里没有同名记录）
        </div>
        <div v-for="file in unclassified.files" :key="file.name" class="unfiled-row">
          <span class="title">{{ file.name }}</span>
          <span class="due">{{ humanSize(file.size) }}</span>
          <select v-model="coursePick[file.name]">
            <option v-for="one in unclassified.courses" :key="one">{{ one }}</option>
          </select>
          <SoftButton
            :height="26"
            :font-size="11.5"
            :disabled="classifying === file.name"
            @click="classify(file.name)"
          >
            {{ classifying === file.name ? "挪动中…" : "挪进去" }}
          </SoftButton>
        </div>
      </div>

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
  min-height: 0;
}
.web-host {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
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
.unfiled {
  background: color-mix(in srgb, var(--amber, #f0b429) 8%, var(--card));
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-card);
  padding: 12px 14px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.unfiled-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.unfiled-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 0;
  border-top: 1px dashed var(--border-soft);
}
.unfiled-row select {
  height: 26px;
  max-width: 260px;
  font-size: 11.5px;
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
