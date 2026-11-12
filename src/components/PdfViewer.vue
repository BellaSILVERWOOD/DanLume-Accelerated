<script setup lang="ts">
// 内置 PDF 阅读（对应旧版 page_docs.py / page_latex.py 里的 QPdfView）
//
// 两个坑（用户 2026-11-11 报「物种起源读起来很慢」「预览里出现 Error: Transport destroyed」）：
// 1. **以前一次性把每一页都画出来**：22 MB、四百多页的书等于四百多张 canvas，内存爆掉、读完要好几分钟。
//    现在只画「视口附近那几页」，其余位置用占位块撑住高度，滚动到再画（和 PDF 阅读器一个道理）。
// 2. 换文件时上一个文档被 `destroy()`，而它还在渲染 → pdf.js 抛 "Transport destroyed" 并被
//    写进错误提示。现在所有 catch 都先看 `token` 还是不是自己，不是就直接忽略；
//    换文件时也会先把旧的渲染任务取消掉。
//
// 数据来源也不再走 base64（22 MB 文件要变成 30 MB 字符串过一遍 IPC）：优先用 Tauri 的
// asset 协议把本地文件当 URL 交给 pdf.js，它自己按需读；asset 协议不可用时才退回 base64。
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import * as pdfjs from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readFileBase64 } from "../api";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

// 中文 / 日文 / 韩文 PDF 用的是「预定义 CMap」编码（UniGB-UCS2-H 等）：
// 不给 pdf.js 这些 CMap，它连字符都解不出来，画面上就只剩数字和字母，
// 汉字全是空白。所以把 cmaps 与 standard_fonts 一起随包发布（public/pdfjs/）。
const ASSET_BASE = import.meta.env.BASE_URL || "/";
const CMAP_URL = `${ASSET_BASE}pdfjs/cmaps/`;
const STANDARD_FONT_URL = `${ASSET_BASE}pdfjs/standard_fonts/`;

const props = withDefaults(
  defineProps<{
    /** 要预览的文件路径；空字符串表示不显示 */
    path?: string;
    /** base64 数据（优先级高于 path，LaTeX 页直接给编译结果） */
    data?: string;
    /** multi=整篇连续滚动，single=一次一页 */
    mode?: "multi" | "single";
  }>(),
  { path: "", data: "", mode: "multi" },
);

/** 每页的占位尺寸（CSS 像素） */
type Slot = { number: number; width: number; height: number };

const pageCount = ref(0);
const pageIndex = ref(1);
const scale = ref(1.15);
const loading = ref(false);
const error = ref("");
const host = ref<HTMLElement | null>(null);
const pagesEl = ref<HTMLElement | null>(null);
const slots = ref<Slot[]>([]);
const doc = shallowRef<any>(null);

let token = 0;
/** 已经画好的页 / 正在画的页 */
const drawn = new Set<number>();
const drawing = new Set<number>();
/** 页号 → 宽高比（拿到真实尺寸后校正占位高度） */
const aspects = new Map<number, number>();
/** 页号 → 占位元素 */
const wrappers = new Map<number, HTMLDivElement>();
let raf = 0;
let renderToken = 0;

const percent = computed(() =>
  pageCount.value ? Math.round((pageIndex.value / pageCount.value) * 100) : 0,
);

function decode(base64: string): Uint8Array {
  const raw = atob(base64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i += 1) out[i] = raw.charCodeAt(i);
  return out;
}

function fitWidth(hostWidth: number): number {
  return Math.max(240, hostWidth - 24);
}

/** 按「适应宽度 × 缩放」算一页画多大 */
function pageBox(aspect: number): { width: number; height: number } {
  const stage = host.value;
  const width = fitWidth(stage ? stage.clientWidth : 800) * scale.value;
  return { width: Math.floor(width), height: Math.floor(width * aspect) };
}

async function destroy() {
  renderToken += 1;
  drawn.clear();
  drawing.clear();
  wrappers.clear();
  slots.value = [];
  pageCount.value = 0;
  pageIndex.value = 1;
  if (pagesEl.value) pagesEl.value.replaceChildren();
  const current = doc.value;
  doc.value = null;
  if (current) {
    try {
      await current.destroy();
    } catch {
      /* pdf.js 有时会在销毁时抛「Transport destroyed」，忽略 */
    }
  }
}

/** 画某一页 */
async function drawPage(number: number) {
  const current = doc.value;
  const hostEl = host.value;
  const wrapper = wrappers.get(number);
  if (!current || !hostEl || !wrapper) return;
  if (drawn.has(number) || drawing.has(number)) return;
  drawing.add(number);
  const myRender = renderToken;
  try {
    const page = await current.getPage(number);
    if (myRender !== renderToken) return;
    const base = page.getViewport({ scale: 1 });
    aspects.set(number, base.height / base.width);
    const box = pageBox(base.height / base.width);
    const viewport = page.getViewport({ scale: box.width / base.width });
    const canvas = document.createElement("canvas");
    // 预览清晰度：按「设备像素比 × 2」渲染，再按 CSS 尺寸缩小显示。
    // 以前只按 devicePixelRatio（这台机器 1.25）画，看 PDF 就明显糊（用户 2026-11-11 报的）。
    const ratio = Math.min(3, Math.max(2, (window.devicePixelRatio || 1) * 2));
    canvas.width = Math.floor(viewport.width * ratio);
    canvas.height = Math.floor(viewport.height * ratio);
    canvas.style.width = `${Math.floor(viewport.width)}px`;
    canvas.style.height = `${Math.floor(viewport.height)}px`;
    canvas.className = "paper";
    const context = canvas.getContext("2d");
    if (context) {
      context.scale(ratio, ratio);
      context.imageSmoothingEnabled = true;
      context.imageSmoothingQuality = "high";
      await page.render({ canvasContext: context, viewport } as any).promise;
    }
    if (myRender !== renderToken) return;
    // 真实高度可能和估算的略有出入：只替换内容；占位块高度只增不减，避免相邻页叠在一起
    wrapper.replaceChildren(canvas);
    const realHeight = Math.floor(viewport.height);
    const placeholder = parseInt(wrapper.style.height || "0", 10) || 0;
    if (realHeight > placeholder) wrapper.style.height = `${realHeight}px`;
    drawn.add(number);
  } catch (err) {
    // 换文件 / 卸载导致的取消不算错误
    if (myRender === renderToken) {
      const text = String(err);
      if (!/Transport destroyed|Rendering cancelled|Worker was destroyed/i.test(text)) {
        error.value = text;
      }
    }
  } finally {
    drawing.delete(number);
  }
}

/** 让视口附近的页画出来；顺带更新「第几页」 */
function syncVisible() {
  const hostEl = host.value;
  if (!hostEl || !doc.value) return;
  const top = hostEl.scrollTop;
  const bottom = top + hostEl.clientHeight;
  let current = 1;
  let found = false;
  for (const slot of slots.value) {
    const wrapper = wrappers.get(slot.number);
    if (!wrapper) continue;
    const elTop = wrapper.offsetTop;
    const elBottom = elTop + wrapper.offsetHeight;
    if (!found && elBottom > top + 40) {
      current = slot.number;
      found = true;
    }
    // 视口上下各多画一页，滚动时不至于看到空白
    if (elBottom > top - hostEl.clientHeight && elTop < bottom + hostEl.clientHeight) {
      void drawPage(slot.number);
    }
  }
  if (found && current !== pageIndex.value) pageIndex.value = current;
}

function scheduleSync() {
  if (raf) return;
  raf = window.requestAnimationFrame(() => {
    raf = 0;
    syncVisible();
  });
}

/** 建占位块：每页一个 div，高度按「适应宽度 × 缩放 × 宽高比」估 */
function buildSlots() {
  const container = pagesEl.value;
  if (!container) return;
  const aspect = aspects.get(1) || 1.414;
  const box = pageBox(aspect);
  const list: Slot[] = [];
  const fragment = document.createDocumentFragment();
  wrappers.clear();
  for (let number = 1; number <= pageCount.value; number += 1) {
    const known = aspects.get(number);
    const height = Math.floor(box.width * (known || aspect));
    const wrapper = document.createElement("div");
    wrapper.className = "slot";
    wrapper.dataset.page = String(number);
    wrapper.style.width = `${box.width}px`;
    wrapper.style.height = `${height}px`;
    wrappers.set(number, wrapper);
    fragment.appendChild(wrapper);
    list.push({ number, width: box.width, height });
  }
  container.replaceChildren(fragment);
  slots.value = list;
}

/** 缩放 / 换文件之后重建尺寸（已画的会按新宽度重画） */
async function repaint() {
  const current = doc.value;
  if (!current) return;
  const myRender = renderToken;
  // 已知的宽高比先补齐，占位高度才准
  const aspect = aspects.get(1) || 1.414;
  const box = pageBox(aspect);
  drawn.clear();
  drawing.clear();
  for (const slot of slots.value) {
    const wrapper = wrappers.get(slot.number);
    if (!wrapper) continue;
    wrapper.replaceChildren();
    wrapper.style.width = `${box.width}px`;
    const height = Math.floor(box.width * (aspects.get(slot.number) || aspect));
    wrapper.style.height = `${height}px`;
    slot.width = box.width;
    slot.height = height;
  }
  if (myRender !== renderToken) return;
  syncVisible();
}

async function load() {
  const my = ++token;
  error.value = "";
  if (!props.path && !props.data) {
    await destroy();
    return;
  }
  loading.value = true;
  try {
    await destroy();
    const params: Record<string, unknown> = {
      cMapUrl: CMAP_URL,
      cMapPacked: true,
      standardFontDataUrl: STANDARD_FONT_URL,
      useSystemFonts: true,
      // 只加载需要的页，别一上来就把整本解完
      disableAutoFetch: true,
      disableStream: false,
      rangeChunkSize: 512 * 1024,
    };
    let task: any = null;
    let loaded: any = null;
    if (props.data) {
      params.data = decode(props.data);
      task = pdfjs.getDocument(params);
      loaded = await task.promise;
    } else {
      // ① 先走本地文件 URL（asset 协议）：pdf.js 自己按需读，不用把整本书搬成 base64
      try {
        params.url = convertFileSrc(props.path);
        task = pdfjs.getDocument(params);
        loaded = await task.promise;
      } catch (err) {
        // ② 退路：老的 base64 方式（asset 协议在某些机器上不可用）
        if (my !== token) return;
        try {
          await task?.destroy?.();
        } catch {
          /* 忽略 */
        }
        const fallbackParams: Record<string, unknown> = {
          cMapUrl: CMAP_URL,
          cMapPacked: true,
          standardFontDataUrl: STANDARD_FONT_URL,
          useSystemFonts: true,
        };
        const file = await readFileBase64(props.path);
        fallbackParams.data = decode(file.data);
        task = pdfjs.getDocument(fallbackParams);
        loaded = await task.promise;
      }
    }
    if (my !== token) {
      try {
        await loaded.destroy();
      } catch {
        /* 忽略 */
      }
      return;
    }
    doc.value = loaded;
    pageCount.value = loaded.numPages;
    loading.value = false;
    // 先拿第一页的真实宽高比，占位高度才接近真实
    try {
      const first = await loaded.getPage(1);
      const base = first.getViewport({ scale: 1 });
      aspects.set(1, base.height / base.width);
    } catch {
      /* 拿不到就用 A4 比例 */
    }
    buildSlots();
    syncVisible();
  } catch (err) {
    if (my !== token) return;
    loading.value = false;
    error.value = String(err);
  }
}

function zoom(delta: number) {
  const next = Math.min(3, Math.max(0.5, Math.round((scale.value + delta) * 100) / 100));
  if (next === scale.value) return;
  scale.value = next;
  void repaint();
}

function fitToWidth() {
  scale.value = 1;
  void repaint();
}

function goto(delta: number) {
  const next = Math.min(pageCount.value || 1, Math.max(1, pageIndex.value + delta));
  if (!next) return;
  pageIndex.value = next;
  const wrapper = wrappers.get(next);
  const hostEl = host.value;
  if (wrapper && hostEl) {
    hostEl.scrollTo({ top: wrapper.offsetTop - 8, behavior: "smooth" });
    scheduleSync();
  }
}

watch(() => [props.path, props.data, props.mode], load, { immediate: true });
onMounted(() => {
  window.addEventListener("resize", handleResize);
});
onBeforeUnmount(() => {
  token = -1;
  window.removeEventListener("resize", handleResize);
  if (raf) window.cancelAnimationFrame(raf);
  void destroy();
});

let resizeTimer = 0;
function handleResize() {
  window.clearTimeout(resizeTimer);
  resizeTimer = window.setTimeout(() => {
    if (doc.value) void repaint();
  }, 220);
}

defineExpose({ goto, zoom, pageCount, pageIndex, repaint, fitToWidth });
</script>

<template>
  <div class="pdf">
    <div class="bar">
      <span class="name">{{ pageCount ? `${pageIndex} / ${pageCount} 页 · ${percent}%` : "" }}</span>
      <span class="spacer" />
      <button class="mini" :disabled="pageIndex <= 1" @click="goto(-1)">上一页</button>
      <button class="mini" :disabled="!pageCount || pageIndex >= pageCount" @click="goto(1)">
        下一页
      </button>
      <button class="mini" @click="zoom(-0.15)">缩小</button>
      <button class="mini" @click="zoom(0.15)">放大</button>
      <button class="mini" @click="fitToWidth">适应宽度</button>
    </div>
    <div ref="host" class="stage" @scroll.passive="scheduleSync">
      <div v-if="loading" class="muted pad">正在读 PDF…</div>
      <div v-else-if="error" class="muted pad">{{ error }}</div>
      <div ref="pagesEl" class="pages" />
    </div>
  </div>
</template>

<style scoped>
.pdf {
  display: flex;
  flex-direction: column;
  min-height: 0;
  height: 100%;
}
.bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 0 6px;
}
.bar .name {
  font-size: 11.5px;
  color: var(--muted);
}
.spacer {
  flex: 1;
}
.mini {
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text-soft);
  border-radius: 7px;
  font-size: 11.5px;
  padding: 3px 8px;
  text-align: center;
  cursor: pointer;
}
.mini:disabled {
  opacity: 0.45;
  cursor: default;
}
.stage {
  flex: 1;
  min-height: 0;
  overflow: auto;
  background: var(--card-alt);
  border: 1px solid var(--border-soft);
  border-radius: 10px;
}
.pages {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 12px;
}
.pad {
  padding: 16px;
}
</style>

<style>
/* canvas / 占位块是运行时创建的，scoped 样式命中不到，单独放全局 */
.pdf .pages .paper {
  display: block;
  background: #fff;
  border-radius: 4px;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.16);
}
.pdf .pages .slot {
  display: flex;
  align-items: flex-start;
  justify-content: center;
  background: rgba(0, 0, 0, 0.03);
  border-radius: 4px;
}
</style>
