<script setup lang="ts">
// 应用外壳：标题栏 + 左导航 + 页面区 + 右栏（与旧版 main_window.py 的布局一致）
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import TitleBar from "./layout/TitleBar.vue";
import SideNav, { type PageKey } from "./layout/SideNav.vue";
import RightDock from "./layout/RightDock.vue";
import DashboardPage from "./pages/DashboardPage.vue";
import CalendarPage from "./pages/CalendarPage.vue";
import SettingsPage from "./pages/SettingsPage.vue";
import MemosPage from "./pages/MemosPage.vue";
import MailPage from "./pages/MailPage.vue";
import HomeworkPage from "./pages/HomeworkPage.vue";
import RoomPage from "./pages/RoomPage.vue";
import LitPage from "./pages/LitPage.vue";
import LatexPage from "./pages/LatexPage.vue";
import NoticesPage from "./pages/NoticesPage.vue";
import ReminderToast from "./components/ReminderToast.vue";
import Mascot from "./components/Mascot.vue";
import AnnouncementBanners from "./components/AnnouncementBanners.vue";
import DocsPage from "./pages/DocsPage.vue";
import Placeholder from "./pages/Placeholder.vue";
import { apply as applyTheme, loadFromSettings } from "./theme";
import { bootInfo, eventsOn, focusStart, focusStatus, focusStop, mailAccountState, settingsAll, settingsSave, upcomingEvents, type UpcomingItem } from "./api";

const current = ref<PageKey>("dashboard");
const middleVisible = ref(true);
const dockVisible = ref(true);
// 窗口布局记忆（旧版 sidebar_open / middle_width / pane_dock / agenda_*）
const middleWidth = ref(760);
const pinned = ref(false);
// 中栏拖拽调宽（旧版 middle_width）
function startDrag(event: MouseEvent) {
  const startX = event.clientX;
  const startWidth = middleWidth.value;
  pinned.value = true;
  const onMove = (move: MouseEvent) => {
    const next = startWidth + (move.clientX - startX);
    middleWidth.value = Math.max(560, Math.min(1600, next));
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    document.body.style.cursor = "";
    saveLayout();
  };
  document.body.style.cursor = "col-resize";
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}
const layoutSwapped = ref(false);
const agendaLimit = ref(6);
const agendaDays = ref(7);
const agenda = ref<UpcomingItem[]>([]);
const todayEvents = ref<{ done: boolean }[]>([]);
const mailStatus = ref("邮箱未配置");
const dataDir = ref("");
// 右栏默认标签（DANLUME_START_PAGE=ai 时直接开在 AI，方便验收截图）
const dockTab = ref<"日程" | "AI" | "文献">("日程");

const PLACEHOLDERS: Record<string, { title: string; subtitle: string; notes: string[] }> = {
  mail: {
    title: "邮箱",
    subtitle: "收件箱 + 发件箱 + 工作汇报，登录一次就能收发",
    notes: ["收件箱（IMAP 多账户、HTML 正文、内联图片、附件小卡）", "工作汇报：勾日程 + 传成果文件"],
  },
  homework: {
    title: "作业",
    subtitle: "内嵌 eLearning：读取课程、一键下载、打开本地文件、截止写入日程",
    notes: ["「打开」按 本地索引 → 附件名 → 作业名 → 同课程周次编号 找文件"],
  },
  ehall: {
    title: "eHall",
    subtitle: "内嵌 eHall 网页，登录状态持久化",
    notes: ["掉登录可在页面里补"],
  },
  classroom: {
    title: "空教室",
    subtitle: "选教学楼 →（可选）教室 / 时间 → 点「查看」，右边画出每间教室的占用情况",
    notes: ["校区 → 教学楼 → 教室 三级联动，回车即查", "横向时间轴 + 悬停卡片 + 放大查看"],
  },
  lit: {
    title: "文献库",
    subtitle: "5 栏目录式管理：项目、星级、多选、自动概述、流程图、引用导出",
    notes: ["复制 GB/T 7714 / BibTeX、导出 EndNote、打开 PDF"],
  },
  latex: {
    title: "公式",
    subtitle: "文档编辑器 + LaTeX：左写右预览，按钮式排版、矩阵、宏包勾选",
    notes: ["重构版改用 WebView 里的 KaTeX 预览，比调 XeLaTeX 快一个量级"],
  },
};

const pageMeta = computed(() => PLACEHOLDERS[current.value]);
watch([middleVisible, dockVisible, middleWidth, agendaLimit, agendaDays, layoutSwapped], saveLayout);
const today = computed(() => {
  const now = new Date();
  const weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  return {
    day: String(now.getDate()),
    month: `${now.getMonth() + 1} 月`,
    week: weekdays[now.getDay()],
    done: todayEvents.value.filter((one) => one.done).length,
    total: todayEvents.value.length,
  };
});

async function loadShell() {
  // 先恢复窗口布局（旧版这些键都记着上次的状态）
  try {
    const all = await settingsAll();
    const values: Record<string, string> = all.values || {};
    // 安全网：如果「中栏」和「右栏」都被关掉，界面就只剩左导航、点什么都没反应，`n    // 所以两个都关时强制把中栏打开（这是之前把用户坑惨的那个状态）。`n    const middleOff = values["sidebar_open"] === "0";`n    const dockOff = values["pane_dock"] === "0";`n    if (middleOff && dockOff) {`n      middleVisible.value = true;`n      void settingsSave({ sidebar_open: "1" });`n    } else if (middleOff) {`n      middleVisible.value = false;`n    }
    if (values["pane_dock"] === "0") dockVisible.value = false;
    if (values["agenda_mode"] === "week") agendaDays.value = 7;
    const width = Number(values["middle_width"] || 0);
    if (width >= 560 && width < 2000) { middleWidth.value = width; pinned.value = true; }
    const limit = Number(values["agenda_count"] || 0);
    if (limit > 0 && limit <= 30) agendaLimit.value = limit;
    const days = Number(values["agenda_days"] || 0);
    if (days > 0 && days <= 90) agendaDays.value = days;
    layoutSwapped.value = values["layout_swapped"] === "1";
    if (values["agenda_open"] === "0") dockVisible.value = false;
    const mode = values["agenda_mode"] || "";
    if (mode === "ai") dockTab.value = "AI";
    else if (mode === "lit") dockTab.value = "文献";
    if (values["window_translucent"] === "1") document.documentElement.dataset.translucent = "1";
    else delete document.documentElement.dataset.translucent;
    // 旧版 theme_apple_v2：默认开（v2 的 macOS 观感），设 0 才回到老样子
    document.documentElement.dataset.apple = values["theme_apple_v2"] === "0" ? "0" : "1";
    // 吉祥物（旧版 mascot_on / mascot_style / mascot_image / mascot_pos）
    // 关键修复：启动时把设置里的主题（theme_mode/accent/bg/alpha...）真正应用上，
    // 之前只有进过设置页才生效，重启后主题会丢。
    loadFromSettings(values);
    applyTheme();
    mascotOn.value = values["mascot_on"] === "1";
    mascotStyle.value = values["mascot_style"] || "cat";
    mascotImage.value = values["mascot_image"] || "";
    mascotPos.value = values["mascot_pos"] || "";
    // 字体（旧版 font_zh / font_en）：中文与西文分别指定，没填就用令牌里的默认
    const zh = (values["font_zh"] || "").trim();
    const en = (values["font_en"] || "").trim();
    if (zh || en) {
      const families = [en, zh, "PingFang SC", "Microsoft YaHei UI", "Segoe UI", "system-ui", "sans-serif"]
        .filter((one) => one && one.length > 0)
        .join(", ");
      document.documentElement.style.setProperty("--font-ui", families);
    }
  } catch {
    // 读不到就用默认
  }
  // 本地日期（toISOString 是 UTC，早上 8 点前会差一天）
  const now = new Date();
  const key = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(
    now.getDate(),
  ).padStart(2, "0")}`;
  todayEvents.value = await eventsOn(key);
  agenda.value = await upcomingEvents(agendaLimit.value);
  try {
    const info = await bootInfo();
    dataDir.value = info.data_dir;
    if (info.start_page) {
      if (info.start_page === "ai") {
        dockVisible.value = true;
        dockTab.value = "AI";
      } else {
        current.value = info.start_page as PageKey;
      }
    }
  } catch {
    dataDir.value = "";
  }
  try {
    const account = await mailAccountState();
    mailStatus.value = account.imap
      ? "邮箱已连接"
      : account.smtp
        ? "邮箱（只能发）"
        : "邮箱未配置";
  } catch {
    mailStatus.value = "邮箱未配置";
  }
}

onMounted(loadShell);

// 吉祥物状态（旧版 mascot_* 四个键）
const mascotOn = ref(false);
const mascotStyle = ref("cat");
const mascotImage = ref("");
const mascotPos = ref("");

function onMascotMove(value: string) {
  mascotPos.value = value;
  void settingsSave({ mascot_pos: value });
}

function hideMascot() {
  mascotOn.value = false;
  void settingsSave({ mascot_on: "0" });
}

function saveLayout() {
  void settingsSave({
    sidebar_open: middleVisible.value ? "1" : "0",
    pane_dock: dockVisible.value ? "1" : "0",
    middle_width: String(middleWidth.value),
    agenda_count: String(agendaLimit.value),
    agenda_open: dockVisible.value ? "1" : "0",
    agenda_days: String(agendaDays.value),
    layout_swapped: layoutSwapped.value ? "1" : "0",
  });
}

/** 左导航点「AI」：不是切页面，而是把右栏打开并切到 AI 标签（和旧版一致） */
function onDockTab(value: string) {
  void settingsSave({ agenda_mode: value === "AI" ? "ai" : value === "文献" ? "lit" : "agenda" });
}

// ---------------------------------------------------------------- 专注（番茄钟）
// 左导航「专注模式」：没在专注就开一轮（默认 focus_minutes），在专注就结束。
const focus = ref<{ active: boolean; remaining: number; label: string; over: boolean }>({
  active: false,
  remaining: 0,
  label: "",
  over: false,
});

async function refreshFocus() {
  try {
    const state = await focusStatus();
    focus.value = {
      active: state.active,
      remaining: state.remaining,
      label: state.label || "",
      over: state.over,
    };
  } catch {
    // 读不到就不显示
  }
}

async function toggleFocus() {
  if (focus.value.active) {
    await focusStop();
    await refreshFocus();
    return;
  }
  let minutes = 25;
  try {
    const all = await settingsAll();
    const given = Number((all.values || {})["focus_minutes"] || 0);
    if (given > 0) minutes = given;
  } catch {
    // 用默认
  }
  await focusStart(minutes);
  await refreshFocus();
}

function focusText(): string {
  const seconds = Math.max(0, focus.value.remaining);
  const mm = Math.floor(seconds / 60);
  const ss = String(seconds % 60).padStart(2, "0");
  return `${mm}:${ss}${focus.value.over ? " · 已走神" : ""}`;
}

let focusTimer: number | null = null;
onMounted(() => {
  void refreshFocus();
  focusTimer = window.setInterval(refreshFocus, 15000);
});
onUnmounted(() => {
  if (focusTimer !== null) window.clearInterval(focusTimer);
});

function openAi() {
  dockVisible.value = true;
  dockTab.value = "AI";
}
function navigate(key: PageKey) {
  current.value = key;
  // 中栏被关掉时，点导航应该把中栏重新打开，否则点了没反应（用户会以为卡死）
  if (!middleVisible.value) {
    middleVisible.value = true;
    void settingsSave({ sidebar_open: "1" });
  }
}
function openAgenda() {
  current.value = "calendar";
}
</script>

<template>
  <div class="shell">
    <TitleBar
      :middle-visible="middleVisible"
      :dock-visible="dockVisible"
      :mail-status="mailStatus"
      @toggle-middle="middleVisible = !middleVisible"
      @toggle-dock="dockVisible = !dockVisible"
    />
    <div class="workspace" :class="{ 'middle-off': !middleVisible }">
      <SideNav
        :current="current"
        :dock-tab="dockTab"
        :dock-visible="dockVisible"
        :today="today"
        @navigate="navigate"
        @ai="openAi"
        @new-event="navigate('calendar')"
        @new-mail="navigate('mail')"
        @focus="toggleFocus"
      />
      <div
        class="main-column"
        v-show="middleVisible"
        :style="pinned ? { flex: '0 0 ' + middleWidth + 'px' } : {}"
      >
        <!-- 服务器公告：warning / critical 级别在这里挂横幅，可关掉 -->
        <AnnouncementBanners />
        <div v-if="current === 'dashboard'" class="page-scroll">
          <DashboardPage />
        </div>
        <div v-else-if="current === 'calendar'" class="page-scroll calendar-scroll">
          <CalendarPage />
        </div>
        <div v-else-if="current === 'mail'" class="page-scroll mailbox-scroll">
          <MailPage />
        </div>
        <div v-else-if="current === 'notices'" class="page-scroll">
          <NoticesPage />
        </div>
        <div v-else-if="current === 'memos'" class="page-scroll memos-scroll">
          <MemosPage />
        </div>
        <div v-else-if="current === 'docs'" class="page-scroll docs-scroll">
          <DocsPage />
        </div>
        <div v-else-if="current === 'homework'" class="page-scroll homework-scroll">
          <HomeworkPage />
        </div>
        <div v-else-if="current === 'ehall'" class="page-scroll room-scroll">
          <RoomPage mode="ehall" />
        </div>
        <div v-else-if="current === 'classroom'" class="page-scroll room-scroll">
          <RoomPage mode="classroom" />
        </div>
        <div v-else-if="current === 'lit'" class="page-scroll lit-scroll">
          <LitPage />
        </div>
        <div v-else-if="current === 'latex'" class="page-scroll latex-scroll">
          <LatexPage />
        </div>
        <div v-else-if="current === 'settings'" class="page-scroll settings-scroll">
          <SettingsPage />
        </div>
        <div v-else class="page-scroll">
          <Placeholder
            v-if="pageMeta"
            :title="pageMeta.title"
            :subtitle="pageMeta.subtitle"
            :notes="pageMeta.notes"
          />
        </div>
      </div>
      <div
        v-if="middleVisible && dockVisible"
        class="splitter"
        @mousedown.prevent="startDrag"
      />
      <RightDock
        v-if="dockVisible"
        :items="agenda"
        :initial-tab="dockTab"
        @tab="onDockTab"
        @open="openAgenda"
      />
    </div>
<button
  v-if="focus.active"
  class="focus-chip"
  :title="focus.label || '专注中'"
  @click="toggleFocus"
>
  {{ focusText() }}
</button>
    <Mascot
      v-if="mascotOn"
      :style="mascotStyle"
      :image="mascotImage"
      :pos="mascotPos"
      @move="onMascotMove"
      @hide="hideMascot"
    />
    <ReminderToast />
  </div>
</template>

<style scoped>
.calendar-scroll,
.settings-scroll,
.memos-scroll,
.docs-scroll,
.mailbox-scroll,
.homework-scroll,
.room-scroll,
.lit-scroll,
.latex-scroll {
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.calendar-scroll > :deep(*),
.memos-scroll > :deep(*),
.docs-scroll > :deep(*),
.mailbox-scroll > :deep(*),
.homework-scroll > :deep(*),
.room-scroll > :deep(*),
.lit-scroll > :deep(*),
.latex-scroll > :deep(*) {
  flex: 1;
  min-height: 0;
}
.splitter {
  flex: 0 0 5px;
  cursor: col-resize;
  background: transparent;
  align-self: stretch;
}
.splitter:hover {
  background: var(--accent-soft);
}
.focus-chip {
  position: fixed;
  right: 18px;
  bottom: 16px;
  z-index: 70;
  padding: 6px 14px;
  border-radius: 999px;
  border: 1px solid var(--hairline, var(--border-soft));
  background: var(--material-card, var(--card));
  backdrop-filter: var(--material-blur, none);
  color: var(--accent);
  font-family: var(--font-mono);
  font-size: 13px;
  box-shadow: var(--shadow-pop, none);
  cursor: pointer;
  text-align: center;
}

/* 中栏容器：必须 flex:1 才能撑满（之前没写这条，一旦 pinned 为 false 就没有宽度，页面全空白） */
.main-column {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.main-column > .page-scroll {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
}
/* 用 grid 固定三栏：左导航 / 中栏(占满剩余) / 右栏，避免 flex 收缩把中栏挤成 0 */
.workspace {
  display: grid;
  grid-template-columns: var(--sidebar-w, 208px) minmax(0, 1fr) auto;
  min-height: 0;
  flex: 1 1 auto;
}
/* 中栏关掉时只剩两列：让右栏（AI / 日程 / 文献）占满剩下的宽度，
   否则它会挤在 1fr 列的最左边、看起来像「缩到左边」，其它页面也没地方显示 */
.workspace.middle-off {
  grid-template-columns: var(--sidebar-w, 208px) minmax(0, 1fr);
}
.workspace.middle-off .dock {
  width: 100%;
}
.main-column {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.main-column > .page-scroll {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
}
.splitter {
  display: none;
}
/* 关键：workspace 自身必须撑满 shell 宽度，否则 grid 的 1fr 会被算成 0（中栏消失） */
.shell > .workspace,
.workspace {
  width: 100%;
  align-self: stretch;
}</style>
