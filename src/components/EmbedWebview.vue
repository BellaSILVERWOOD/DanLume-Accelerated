<script setup lang="ts">
// 内嵌网页（对应旧版 web_panel.EmbeddedWebPanel）：把 eHall / eLearning 贴在页面里，不弹窗。
//
// 做法：页面里放一个占位 div，把它相对窗口的坐标/尺寸交给 Rust 的子 Webview；
// 滚动、切页、窗口缩放都重新算一次，离开页面时关掉（否则子 Webview 会浮在最上面）。
import { onMounted, onUnmounted, ref, watch } from "vue";
import SoftButton from "./SoftButton.vue";
import {
  embedBounds,
  embedClose,
  embedCloseTab,
  embedNavigate,
  embedNewTab,
  embedOpen,
  embedReload,
  embedSelectTab,
  embedTabs,
  openUrl,
  uisGrabProfile,
  type EmbedTab,
} from "../api";

const props = withDefaults(
  defineProps<{
    kind: string;
    url: string;
    height?: string;
    homeLabel?: string;
  }>(),
  { height: "100%", homeLabel: "回到首页" },
);

const slot = ref<HTMLElement | null>(null);
const ready = ref(false);
const message = ref("");
const currentUrl = ref(props.url);
const sessionMsg = ref("");

/* ---- 多标签页（M5，对应旧版 `EmbeddedWebPanel` 的 QTabBar） ---- */
const tabs = ref<EmbedTab[]>([]);
const activeTab = ref(0);
const tabsEnabled = ref(false);
const tabNote = ref("");
let tabTimer = 0;
let tabBusy = false;

/** 问一次 Rust：几个标签页、当前第几个、有没有网页想开新窗口 */
async function pollTabs() {
  if (!ready.value || tabBusy) return;
  tabBusy = true;
  try {
    const state = await embedTabs(props.kind);
    tabsEnabled.value = !!state.enabled;
    if (!state.ok) return;
    tabs.value = state.tabs || [];
    activeTab.value = state.active ?? 0;
    const pending = (state.pending || "").trim();
    if (pending) {
      // 网页里点了 target=_blank：多标签页开着就再开一个，关着就原地跳
      if (tabsEnabled.value) {
        await newTab(pending);
      } else {
        currentUrl.value = pending;
        await embedNavigate(props.kind, pending);
        void pollTabs();
      }
    }
    // 标签页被关光了：把首页开回来，别留一块空白
    if (!tabs.value.length) {
      ready.value = false;
      await open();
    }
  } catch {
    /* 下一轮再问 */
  } finally {
    tabBusy = false;
  }
}

function startPolling() {
  stopPolling();
  tabTimer = window.setInterval(pollTabs, 1500);
  void pollTabs();
}

function stopPolling() {
  if (tabTimer) window.clearInterval(tabTimer);
  tabTimer = 0;
}

/** 开一个新标签页（不给地址就开这个面板的首页） */
async function newTab(url?: string) {
  const box = rect();
  if (!box) return;
  const target = (url || "").trim() || props.url;
  const state = await embedNewTab(props.kind, target, box.x, box.y, box.width, box.height);
  if (state.ok) {
    tabs.value = state.tabs || [];
    activeTab.value = state.active ?? 0;
    tabNote.value = "";
    currentUrl.value = target;
    await syncBounds();
    watchSlowLoad();
  } else {
    tabNote.value = state.error || "开新标签页失败";
  }
}

/** 切标签页 */
async function switchTab(index: number) {
  if (index === activeTab.value) return;
  const state = await embedSelectTab(props.kind, index);
  if (!state.ok) {
    tabNote.value = state.error || "切不过去";
    return;
  }
  tabs.value = state.tabs || [];
  activeTab.value = state.active ?? index;
  const tab = tabs.value.find((one) => one.index === activeTab.value);
  if (tab?.url) currentUrl.value = tab.url;
  await syncBounds();
}

/** 关标签页；最后一个关掉就整块重开首页 */
async function closeTab(index: number) {
  const state = await embedCloseTab(props.kind, index);
  if (!state.ok) {
    tabNote.value = state.error || "关不掉";
    return;
  }
  tabs.value = state.tabs || [];
  activeTab.value = state.active ?? 0;
  if (state.closed || !tabs.value.length) {
    ready.value = false;
    await open();
    return;
  }
  await syncBounds();
}
/** J8-6：8 秒还没加载出来 → 提示「可能被挡」，并给一个浏览器打开的入口 */
const slow = ref(false);
let slowTimer = 0;
let observer: ResizeObserver | null = null;
let raf = 0;
let grabTimers: number[] = [];
let unlistenLoad: (() => void) | null = null;

/** 开始计时：这个子 Webview 到底有没有把页面加载出来，只有 Rust 那边知道 */
function watchSlowLoad() {
  window.clearTimeout(slowTimer);
  slow.value = false;
  slowTimer = window.setTimeout(() => {
    if (!ready.value) return;
    slow.value = true;
    // 这条只是提醒，别一直挂着：8 秒是「可能被挡」，再过 12 秒就自己收掉
    // （页面其实已经出来了、但 embed:loaded 事件没收到时会一直停在那里 —— 2026-11-12 实测）
    window.setTimeout(() => {
      slow.value = false;
    }, 12000);
  }, 8000);
}

function clearSlow() {
  window.clearTimeout(slowTimer);
  slow.value = false;
}

/** 内嵌网页登录完成后，从主窗口 cookie 抓一次 UIS 会话（Canvas / 教务才有登录态） */
async function grabSession() {
  const result = await uisGrabProfile();
  if (result.ok) {
    sessionMsg.value = `会话已保存（${result.cookies || 0} 个 cookie${result.verdict ? ` · ${result.verdict}` : ""}）`;
  } else {
    sessionMsg.value = result.error || "还没登录";
  }
}

function scheduleGrabs() {
  grabTimers.forEach((id) => window.clearTimeout(id));
  // 登录跳转 + 页面渲染要几秒，分两次抓，避免第一次太早
  grabTimers = [window.setTimeout(grabSession, 8000), window.setTimeout(grabSession, 25000)];
}

/** 占位 div 在窗口里的位置（CSS 像素，与 Rust 的逻辑像素一致） */
function rect() {
  const el = slot.value;
  if (!el) return null;
  const box = el.getBoundingClientRect();
  if (box.width < 40 || box.height < 40) return null;
  return { x: box.left, y: box.top, width: box.width, height: box.height };
}

async function syncBounds() {
  const box = rect();
  if (!box || !ready.value) return;
  await embedBounds(props.kind, box.x, box.y, box.width, box.height);
}

function scheduleSync() {
  if (raf) return;
  raf = window.requestAnimationFrame(async () => {
    raf = 0;
    await syncBounds();
  });
}

async function open() {
  const box = rect();
  if (!box) {
    message.value = "这块区域还没排版好，稍等一下再试";
    return;
  }
  const result = await embedOpen(props.kind, props.url, box.x, box.y, box.width, box.height);
  if (!result.ok) {
    message.value = result.error || "内嵌网页打不开";
    return;
  }
  ready.value = true;
  message.value = "";
  currentUrl.value = props.url;
  await syncBounds();
  scheduleGrabs();
  watchSlowLoad();
  startPolling();
}

async function reload() {
  if (!ready.value) {
    await open();
    return;
  }
  await embedReload(props.kind);
  scheduleGrabs();
  watchSlowLoad();
}

async function goHome() {
  currentUrl.value = props.url;
  if (!ready.value) {
    await open();
    return;
  }
  await embedNavigate(props.kind, props.url);
  watchSlowLoad();
}

async function close() {
  if (ready.value) {
    await embedClose(props.kind);
    ready.value = false;
  }
  clearSlow();
}

/** 在系统浏览器里打开当前地址（内嵌被挡 / 要登录网银之类就靠它） */
function openInBrowser() {
  void openUrl(currentUrl.value || props.url);
}

onMounted(async () => {
  await open();
  // Rust 那边子 Webview「页面开出来了」会发 embed:loaded，收到就把 8 秒倒计时撤掉
  try {
    const { listen } = await import("@tauri-apps/api/event");
    unlistenLoad = await listen<{ kind: string; event: string }>("embed:loaded", (event) => {
      if (event.payload?.kind !== props.kind) return;
      if (event.payload?.event === "finished") clearSlow();
    });
  } catch {
    /* 浏览器预览里没有事件系统 */
  }
  observer = new ResizeObserver(scheduleSync);
  if (slot.value) observer.observe(slot.value);
  window.addEventListener("resize", scheduleSync);
  window.addEventListener("scroll", scheduleSync, true);
});
onUnmounted(async () => {
  stopPolling();
  window.removeEventListener("resize", scheduleSync);
  window.removeEventListener("scroll", scheduleSync, true);
  observer?.disconnect();
  observer = null;
  grabTimers.forEach((id) => window.clearTimeout(id));
  grabTimers = [];
  clearSlow();
  unlistenLoad?.();
  unlistenLoad = null;
  await close();
});

// 换地址（比如设置里改了 ehall_url）就重新加载一次
watch(
  () => props.url,
  async (next) => {
    if (!next) return;
    currentUrl.value = next;
    if (!ready.value) {
      await open();
      return;
    }
    await embedNavigate(props.kind, next);
    watchSlowLoad();
  },
);

defineExpose({ reload, goHome, open });
</script>

<template>
  <div class="embed">
    <!-- 多标签页：一个面板里好几个网页，点标签切、× 关、（+）开新的 -->
    <div v-if="tabsEnabled && tabs.length" class="tabs">
      <div class="tab-strip">
        <button
          v-for="tab in tabs"
          :key="tab.index"
          class="tab"
          :class="{ on: tab.index === activeTab }"
          :title="tab.url || tab.title"
          @click="switchTab(tab.index)"
        >
          <span class="tab-title">{{ tab.title || "新标签页" }}</span>
          <span class="tab-x" title="关掉这个标签页" @click.stop="closeTab(tab.index)">×</span>
        </button>
      </div>
      <button class="tab-add" title="新标签页" @click="newTab()">＋</button>
      <span v-if="tabNote" class="tiny">{{ tabNote }}</span>
    </div>
    <div class="embed-head">
      <span class="muted tiny">{{ currentUrl }}</span>
      <span class="spacer" />
      <SoftButton :height="28" :font-size="12" @click="goHome">{{ homeLabel }}</SoftButton>
      <span v-if="sessionMsg" class="tiny session">{{ sessionMsg }}</span>
      <SoftButton :height="28" :font-size="12" @click="grabSession">抓取会话</SoftButton>
      <SoftButton :height="28" :font-size="12" @click="reload">刷新</SoftButton>
    </div>
    <!-- J8-6：8 秒还没出来 → 多半是这个站点不让内嵌，给个「浏览器打开」的出口 -->
    <div v-if="slow" class="slow">
      <span class="slow-text">
        8 秒了还没出来：这个站点可能不允许内嵌（或被网络挡住）。
      </span>
      <span class="slow-spacer" />
      <SoftButton :height="26" :font-size="12" @click="openInBrowser">用浏览器打开</SoftButton>
      <SoftButton :height="26" :font-size="12" @click="clearSlow">还等着</SoftButton>
    </div>
    <div ref="slot" class="embed-slot">
      <div v-if="!ready" class="embed-hint">
        {{ message || "正在打开网页…" }}
      </div>
    </div>
  </div>
</template>

<style scoped>
.embed {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.embed-head {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
}
/* 标签页条：跟 Windows 浏览器一样一排，横向能滚 */
.tabs {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.tab-strip {
  display: flex;
  align-items: center;
  gap: 3px;
  overflow-x: auto;
  scrollbar-width: thin;
  min-width: 0;
  flex: 1;
}
.tab {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 26px;
  padding: 0 6px 0 9px;
  border-radius: 8px 8px 0 0;
  border: 1px solid var(--border-soft);
  border-bottom: none;
  background: var(--card-alt, transparent);
  color: var(--muted);
  font-size: 11.5px;
  max-width: 190px;
  flex: 0 0 auto;
  cursor: pointer;
}
.tab.on {
  background: var(--card);
  color: var(--text);
  font-weight: 600;
}
.tab-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tab-x {
  flex: 0 0 auto;
  width: 15px;
  text-align: center;
  border-radius: 5px;
  color: var(--muted);
  line-height: 15px;
}
.tab-x:hover {
  background: color-mix(in srgb, var(--danger) 20%, transparent);
  color: var(--danger);
}
.tab-add {
  flex: 0 0 auto;
  width: 26px;
  height: 26px;
  border-radius: 8px 8px 0 0;
  border: 1px solid var(--border-soft);
  border-bottom: none;
  background: var(--card-alt, transparent);
  color: var(--muted);
  font-size: 13px;
  cursor: pointer;
}
.tab-add:hover {
  color: var(--text);
}
.embed-slot {
  flex: 1;
  min-height: 160px;
  border-radius: 12px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  position: relative;
  overflow: hidden;
}
.session {
  color: var(--muted);
}
.slow {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 9px;
  border-radius: 10px;
  border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--border-soft));
  background: color-mix(in srgb, var(--accent) 8%, var(--card));
  font-size: 12px;
  color: var(--text-soft);
}
.slow-text {
  flex: 0 1 auto;
}
.slow-spacer {
  flex: 1;
}
.embed-hint {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--muted);
  font-size: 12.5px;
}
</style>
