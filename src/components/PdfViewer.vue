<script setup lang="ts">
// 内置 PDF 阅读（对应旧版 page_docs.py / page_latex.py 里的 QPdfView）
// 用 pdf.js 把每一页渲染到 canvas：支持翻页、缩放、整篇连续滚动。
import { onBeforeUnmount, ref, shallowRef, watch } from "vue";
import * as pdfjs from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
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

const pages = ref<number[]>([]);
const pageCount = ref(0);
const pageIndex = ref(1);
const scale = ref(1.15);
const loading = ref(false);
const error = ref("");
const host = ref<HTMLElement | null>(null);
const pagesEl = ref<HTMLElement | null>(null);
const doc = shallowRef<any>(null);
let token = 0;

function decode(base64: string): Uint8Array {
  const raw = atob(base64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i += 1) out[i] = raw.charCodeAt(i);
  return out;
}

async function destroy() {
  pages.value = [];
  pageCount.value = 0;
  pageIndex.value = 1;
  if (pagesEl.value) pagesEl.value.replaceChildren();
  if (doc.value) {
    try {
      await doc.value.destroy();
    } catch {
      /* 忽略销毁异常 */
    }
    doc.value = null;
  }
}

async function paint() {
  const current = doc.value;
  const stage = host.value;
  const target = pagesEl.value;
  if (!current || !stage || !target) return;
  const my = token;
  const width = Math.max(240, stage.clientWidth - 24);
  const list = props.mode === "single" ? [pageIndex.value] : pages.value;
  const nodes: HTMLCanvasElement[] = [];
  for (const number of list) {
    const page = await current.getPage(number);
    if (my !== token) return;
    const base = page.getViewport({ scale: 1 });
    const fit = Math.min(2.5, Math.max(0.4, (width / base.width) * scale.value));
    const viewport = page.getViewport({ scale: fit });
    const canvas = document.createElement("canvas");
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.floor(viewport.width * ratio);
    canvas.height = Math.floor(viewport.height * ratio);
    canvas.style.width = `${Math.floor(viewport.width)}px`;
    canvas.style.height = `${Math.floor(viewport.height)}px`;
    canvas.className = "paper";
    const context = canvas.getContext("2d");
    if (context) {
      context.scale(ratio, ratio);
      await page.render({ canvasContext: context, viewport } as any).promise;
    }
    nodes.push(canvas);
  }
  if (my !== token) return;
  target.replaceChildren(...nodes);
}

async function load() {
  const my = ++token;
  error.value = "";
  pages.value = [];
  pageCount.value = 0;
  if (pagesEl.value) pagesEl.value.replaceChildren();
  if (!props.path && !props.data) {
    await destroy();
    return;
  }
  loading.value = true;
  try {
    await destroy();
    let bytes: Uint8Array;
    if (props.data) {
      bytes = decode(props.data);
    } else {
      const file = await readFileBase64(props.path);
      bytes = decode(file.data);
    }
    const task = pdfjs.getDocument({
      data: bytes,
      cMapUrl: CMAP_URL,
      cMapPacked: true,
      standardFontDataUrl: STANDARD_FONT_URL,
      useSystemFonts: true,
    });
    const loaded = await task.promise;
    if (my !== token) {
      loaded.destroy();
      return;
    }
    doc.value = loaded;
    pageCount.value = loaded.numPages;
    pages.value = Array.from({ length: loaded.numPages }, (_, i) => i + 1);
    pageIndex.value = 1;
    loading.value = false;
    await paint();
  } catch (err) {
    loading.value = false;
    error.value = String(err);
  }
}

function zoom(delta: number) {
  scale.value = Math.min(3, Math.max(0.5, Math.round((scale.value + delta) * 100) / 100));
  paint();
}

function goto(delta: number) {
  const next = Math.min(pageCount.value || 1, Math.max(1, pageIndex.value + delta));
  if (next === pageIndex.value) return;
  pageIndex.value = next;
  if (props.mode === "single") paint();
}

watch(() => [props.path, props.data, props.mode], load, { immediate: true });
onBeforeUnmount(() => {
  token = -1;
  destroy();
});

defineExpose({ goto, zoom, pageCount, pageIndex });
</script>

<template>
  <div class="pdf">
    <div class="bar">
      <span class="name">{{ pageCount ? `${pageIndex} / ${pageCount} 页` : "" }}</span>
      <span class="spacer" />
      <button class="mini" :disabled="pageIndex <= 1" @click="goto(-1)">上一页</button>
      <button class="mini" :disabled="pageIndex >= pageCount" @click="goto(1)">下一页</button>
      <button class="mini" @click="zoom(-0.15)">缩小</button>
      <button class="mini" @click="zoom(0.15)">放大</button>
      <button class="mini" @click="scale = 1.15, paint()">适应宽度</button>
    </div>
    <div ref="host" class="stage">
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
.paper {
  display: block;
  background: #fff;
  border-radius: 4px;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.16);
}
.pad {
  padding: 16px;
}
</style>

<style>
/* canvas 是运行时创建的，scoped 样式命中不到，单独放全局 */
.pdf .pages .paper {
  display: block;
  background: #fff;
  border-radius: 4px;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.16);
}
</style>
