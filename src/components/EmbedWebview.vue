<script setup lang="ts">
// 内嵌网页（对应旧版 web_panel.EmbeddedWebPanel）：把 eHall / eLearning 贴在页面里，不弹窗。
//
// 做法：页面里放一个占位 div，把它相对窗口的坐标/尺寸交给 Rust 的子 Webview；
// 滚动、切页、窗口缩放都重新算一次，离开页面时关掉（否则子 Webview 会浮在最上面）。
import { onMounted, onUnmounted, ref, watch } from "vue";
import SoftButton from "./SoftButton.vue";
import { embedBounds, embedClose, embedNavigate, embedOpen, embedReload, openUrl, uisGrabProfile } from "../api";

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
    if (ready.value) slow.value = true;
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
