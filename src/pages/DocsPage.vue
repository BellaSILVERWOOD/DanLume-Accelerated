<script setup lang="ts">
// 文档：左边目录树、右边预览（对应旧版 page_docs.py）
//
// 树的做法：不递归挂组件，而是把「当前展开状态」展平成一串可见行（rows），
// 每行带 depth 做缩进。展开状态、已加载的子项都按绝对路径存在两个 Record 里，
// 所以同一个目录只会挂一次、也只会请求一次，天然不会递归套自己。
import { computed, nextTick, onMounted, ref, watch } from "vue";
import SoftButton from "../components/SoftButton.vue";
import IconBadge from "../components/IconBadge.vue";
import PdfViewer from "../components/PdfViewer.vue";
import { usePanelFullscreen } from "../layout/fullscreen";
import {
  defaultDocsDir,
  listDir,
  openPath,
  pickFiles,
  readTextFile,
  revealPath,
  settingsAll,
  settingsSave,
  type DocEntry,
} from "../api";

const dir = ref("");
/** 当前目录（树根）的条目 */
const entries = ref<DocEntry[]>([]);
/** 文件夹 → 子项。按绝对路径缓存，展开过一次就不再调 listDir */
const childrenByPath = ref<Record<string, DocEntry[]>>({});
/** 文件夹 → 是否展开。按绝对路径记，切到别的目录再回来还是原样 */
const expanded = ref<Record<string, boolean>>({});
/** 正在读取子项的文件夹（路径 → true） */
const loadingPath = ref<Record<string, boolean>>({});
const selected = ref<DocEntry | null>(null);
const preview = ref("");
const message = ref("");
const busy = ref(false);
const shortcuts = ref<{ label: string; path: string }[]>([]);

// 全屏阅读（旧版 page_docs.toggle_fullscreen）：收起标题行 / 目录树，让阅读区铺满，
// 同时 App.vue 会把标题栏 / 左导航 / 右栏藏起来；Esc 退出。
const { active: fullscreen, toggle: toggleFullscreen } = usePanelFullscreen();
const viewer = ref<any>(null);

/**
 * 全屏样式写成**内联样式**：以前用 `.page.full .body { display: block }` 这类选择器，
 * 实测没生效（阅读区被挤成 200px，全是换行的字），内联样式优先级最高、不会再被别处覆盖。
 */
const overlayStyle = computed(() =>
  fullscreen.value
    ? {
        position: "fixed" as const,
        inset: "0",
        zIndex: 55,
        padding: "14px 16px 12px",
        background: "var(--card)",
        height: "auto",
      }
    : undefined,
);
const bodyStyle = computed(() => (fullscreen.value ? { display: "block" } : undefined));
const previewStyle = computed(() =>
  fullscreen.value ? { width: "100%", minWidth: "0", height: "100%" } : undefined,
);

// 进出全屏后阅读区宽度变了：让 PDF 按新宽度重新排一次（否则还是按旧宽度画的窄页）
watch(fullscreen, async () => {
  await nextTick();
  window.setTimeout(() => {
    viewer.value?.repaint?.();
  }, 80);
});

/** 展平之后的一行：真实条目 + 缩进层级；placeholder 是“空目录”占位 */
type TreeRow = {
  entry: DocEntry;
  depth: number;
  placeholder?: boolean;
};

function human(size: number): string {
  if (size >= 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  if (size >= 1024) return `${Math.round(size / 1024)} KB`;
  return `${size} B`;
}

function badgeTone(kind: string): string {
  if (kind === "PDF") return "rose";
  if (kind === "Word") return "sky";
  if (kind === "PPT") return "amber";
  if (kind === "Excel") return "teal";
  if (kind === "图片") return "violet";
  return "teal";
}

/** 路径归一化：统一反斜杠并小写，只用来判断“是不是同一个目录” */
function normPath(path: string): string {
  return path.replace(/\//g, "\\").toLowerCase();
}

/** 某目录自身 + 它所有上级目录；子项里出现这些说明成环，直接跳过 */
function selfAndAncestors(path: string): Set<string> {
  const out = new Set<string>();
  const parts = normPath(path).split("\\");
  for (let i = 1; i <= parts.length; i += 1) {
    const one = parts.slice(0, i).join("\\");
    if (one) out.add(one);
  }
  return out;
}

/** 可见行：顶层 → 展开的文件夹逐层展开（同一路径只出现一次） */
const rows = computed<TreeRow[]>(() => {
  const out: TreeRow[] = [];
  const seen = new Set<string>();
  const walk = (list: DocEntry[], depth: number) => {
    for (const entry of list) {
      const key = normPath(entry.path);
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({ entry, depth });
      if (!entry.is_dir || !expanded.value[entry.path]) continue;
      const kids = childrenByPath.value[entry.path];
      if (!kids) continue;
      if (kids.length) {
        walk(kids, depth + 1);
      } else {
        // 展开了一个空文件夹：给一行占位，不至于看着像“没反应”
        out.push({
          entry: { ...entry, name: "这个目录是空的", path: `${entry.path}∅` },
          depth: depth + 1,
          placeholder: true,
        });
      }
    }
  };
  walk(entries.value, 0);
  return out;
});

const hasFolder = computed(() => rows.value.some((row) => row.entry.is_dir));
const expandedCount = computed(
  () => Object.values(expanded.value).filter(Boolean).length,
);

const parentPath = computed(() => {
  const parts = dir.value.replace(/\\/g, "/").split("/");
  parts.pop();
  return parts.join("\\");
});

const isPdf = computed(() => {
  const one = selected.value;
  if (!one) return false;
  return one.kind === "PDF" || one.name.toLowerCase().endsWith(".pdf");
});

async function openDir(path: string) {
  busy.value = true;
  try {
    const result = await listDir(path);
    dir.value = result.dir || path;
    entries.value = result.entries || [];
    message.value = "";
    selected.value = null;
    preview.value = "";
    // 进了别的目录就别继续全屏了，不然退出全屏按钮在标题栏里（被藏了）找不着
    if (fullscreen.value) toggleFullscreen();
  } catch (err) {
    message.value = String(err);
  } finally {
    busy.value = false;
  }
}

/** 懒加载某个文件夹的子项（已缓存就直接返回，不重复请求） */
async function loadChildren(entry: DocEntry): Promise<DocEntry[]> {
  const path = entry.path;
  const cached = childrenByPath.value[path];
  if (cached) return cached;
  loadingPath.value = { ...loadingPath.value, [path]: true };
  try {
    const result = await listDir(path);
    const blocked = selfAndAncestors(path);
    const kids = (result.entries || []).filter(
      (one) => !blocked.has(normPath(one.path)),
    );
    childrenByPath.value = { ...childrenByPath.value, [path]: kids };
    return kids;
  } catch (err) {
    message.value = String(err);
    childrenByPath.value = { ...childrenByPath.value, [path]: [] };
    return [];
  } finally {
    const rest = { ...loadingPath.value };
    delete rest[path];
    loadingPath.value = rest;
  }
}

/** 点文件夹：只切换展开状态，不触发预览 */
async function toggleFolder(entry: DocEntry) {
  const path = entry.path;
  const next = !expanded.value[path];
  expanded.value = { ...expanded.value, [path]: next };
  if (next) await loadChildren(entry);
}

/** 全部展开：广度优先把已知的文件夹逐层展开（最多 300 个，防万一的环） */
async function expandAll() {
  busy.value = true;
  try {
    const seen = new Set<string>();
    const queue: DocEntry[] = entries.value.filter((one) => one.is_dir);
    let guard = 0;
    while (queue.length && guard < 300) {
      const node = queue.shift();
      if (!node) break;
      guard += 1;
      const key = normPath(node.path);
      if (seen.has(key)) continue;
      seen.add(key);
      expanded.value = { ...expanded.value, [node.path]: true };
      const kids = await loadChildren(node);
      for (const kid of kids) {
        if (kid.is_dir) queue.push(kid);
      }
    }
  } finally {
    busy.value = false;
  }
}

/** 全部收起：只清展开状态，子项缓存留着，再展开就不用重新读盘 */
function collapseAll() {
  expanded.value = {};
}

async function choose(entry: DocEntry) {
  if (entry.is_dir) {
    await toggleFolder(entry);
    return;
  }
  selected.value = entry;
  // 旧版 last_doc：记住最后看的那个文件，下次进来直接回到它
  void settingsSave({ last_doc: entry.path });
  preview.value = "";
  if (entry.kind === "PDF") return;
  if (["文本", "文件"].includes(entry.kind) || entry.kind === "") {
    try {
      if (entry.kind === "文本") {
        preview.value = await readTextFile(entry.path);
      } else {
        preview.value = "";
      }
    } catch (err) {
      preview.value = String(err);
    }
  }
}

/** 双击：文件夹进去当新的根（保持旧版“点文件夹就进目录”的手感），文件交给系统程序 */
async function openEntry(entry: DocEntry) {
  if (entry.is_dir) {
    await openDir(entry.path);
    return;
  }
  await openPath(entry.path);
}

async function pickFile() {
  // 原生「选文件」对话框：选中就用系统程序打开
  const picked = await pickFiles();
  if (!picked.length) return;
  await openPath(picked[0]);
}

onMounted(async () => {
  const info = await defaultDocsDir();
  shortcuts.value = [
    { label: "作业下载目录", path: info.downloads },
    { label: "数据目录", path: info.data },
  ];
  // 先试着回到上次看的文件（旧版 last_doc）
  let restored = false;
  try {
    const all = await settingsAll();
    const last = (all.values || {})["last_doc"] || "";
    if (last) {
      const slash = Math.max(last.lastIndexOf("\\"), last.lastIndexOf("/"));
      const parent = slash > 0 ? last.slice(0, slash) : "";
      if (parent) {
        await openDir(parent);
        const hit = entries.value.find((one) => one.path === last);
        if (hit) {
          await choose(hit);
          restored = true;
        }
      }
    }
  } catch {
    // 读不到就按默认目录打开
  }
  if (!restored) await openDir(info.downloads || info.data);
});
</script>

<template>
  <div class="page" :class="{ full: fullscreen }" :style="overlayStyle">
    <div v-show="!fullscreen" class="page-head">
      <div class="titles">
        <h1 class="h1">文档</h1>
        <div class="muted">PDF / Word / PPT / Excel 文件都能直接看</div>
      </div>
      <div class="spacer" />
      <SoftButton :height="32" :font-size="12.5" glyph="FOLDER" @click="pickFile">
        打开文件
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" glyph="FOLDER" @click="openPath(dir)">
        打开下载目录
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" @click="selected && openPath(selected.path)">
        用系统程序打开
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" glyph="MAXIMIZE" @click="toggleFullscreen">
        {{ fullscreen ? "退出全屏" : "全屏" }}
      </SoftButton>
    </div>

    <div class="body" :style="bodyStyle">
      <div v-show="!fullscreen" class="side">
        <div class="side-title">分类 / 最近目录</div>
        <button
          v-for="item in shortcuts"
          :key="item.path"
          class="side-row"
          @click="openDir(item.path)"
        >
          <span class="name">{{ item.label }}</span>
          <span class="path">{{ item.path }}</span>
        </button>
        <div class="side-foot">
          <div class="path-full">{{ dir }}</div>
        </div>
      </div>

      <div v-show="!fullscreen" class="list">
        <div class="list-head">
          <button class="up" :disabled="!parentPath" @click="openDir(parentPath)">
            ↑ 上一层
          </button>
          <button class="up" :disabled="!hasFolder" @click="expandAll">全部展开</button>
          <button class="up" :disabled="!expandedCount" @click="collapseAll">
            全部收起
          </button>
          <span class="spacer" />
          <span class="tiny">{{ entries.length }} 项{{ busy ? " · 读取中…" : "" }}</span>
        </div>
        <div class="rows">
          <template v-for="row in rows" :key="row.entry.path">
            <div
              v-if="row.placeholder"
              class="empty placeholder"
              :style="{ paddingLeft: 12 + row.depth * 16 + 'px' }"
            >
              {{ row.entry.name }}
            </div>
            <button
              v-else
              class="row"
              :class="{ on: selected?.path === row.entry.path, folder: row.entry.is_dir }"
              :style="{ paddingLeft: 9 + row.depth * 16 + 'px' }"
              @click="choose(row.entry)"
              @dblclick="openEntry(row.entry)"
            >
              <span class="twist">
                {{
                  row.entry.is_dir
                    ? loadingPath[row.entry.path]
                      ? "…"
                      : expanded[row.entry.path]
                        ? "▾"
                        : "▸"
                    : ""
                }}
              </span>
              <IconBadge
                :glyph="row.entry.is_dir ? 'FOLDER' : 'DOCUMENT'"
                :tone="row.entry.is_dir ? 'amber' : badgeTone(row.entry.kind)"
                :size="24"
              />
              <span class="name">{{ row.entry.name }}</span>
              <span class="kind">{{ row.entry.kind }}</span>
              <span class="size">{{ row.entry.is_dir ? "" : human(row.entry.size) }}</span>
              <span class="date">{{ row.entry.modified.slice(5) }}</span>
            </button>
          </template>
          <div v-if="message && entries.length" class="empty">{{ message }}</div>
          <div v-else-if="!entries.length" class="empty">
            {{ message || "这个目录是空的" }}
          </div>
        </div>
      </div>

      <div class="preview" :style="previewStyle">
        <div class="preview-head">
          <span class="name">{{ selected ? selected.name : "预览" }}</span>
          <span class="spacer" />
          <span v-if="fullscreen" class="tiny">Esc 退出全屏</span>
          <SoftButton
            v-if="fullscreen"
            :height="26"
            :font-size="11.5"
            @click="toggleFullscreen"
          >
            退出全屏
          </SoftButton>
          <SoftButton
            v-if="selected"
            :height="26"
            :font-size="11.5"
            @click="revealPath(selected.path)"
          >
            在资源管理器里显示
          </SoftButton>
        </div>
        <div class="preview-body">
          <template v-if="selected">
            <PdfViewer
              v-if="isPdf"
              ref="viewer"
              class="pdf-host"
              :path="selected.path"
              mode="multi"
            />
            <pre v-else-if="preview" class="text">{{ preview }}</pre>
            <div v-else class="hint">
              <IconBadge glyph="DOCUMENT" :tone="badgeTone(selected.kind)" :size="40" />
              <div class="kind-big">{{ selected.kind }}</div>
              <div class="muted">
                {{ selected.name }} · {{ human(selected.size) }} ·
                {{ selected.modified }}
              </div>
              <div class="muted">
                {{ selected.kind === "PDF" ? "PDF 读取失败，可以交给系统默认程序打开" : "这类文件交给你自己的系统程序打开" }}
              </div>
              <SoftButton :height="30" :font-size="12.5" @click="openPath(selected.path)">
                用系统程序打开
              </SoftButton>
            </div>
          </template>
          <div v-else class="hint muted">选左边一个文件就能预览</div>
        </div>
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
/* 全屏阅读：目录树 / 标题行都藏了，预览区铺满整个窗口。
   用 position:fixed 盖住窗口，不再依赖父容器的宽度（实测在 flex 里只剩一个孩子时
   宽度会被算成内容宽度、阅读区被挤成 110px）。 */
.page.full {
  gap: 0;
  position: fixed;
  inset: 0;
  z-index: 55;
  padding: 14px 16px 12px;
  background: var(--card);
}
.page.full .body {
  display: block;
  min-height: 0;
}
.page.full .preview {
  width: auto;
  min-width: 0;
  height: 100%;
}
.body {
  flex: 1;
  display: flex;
  gap: 12px;
  min-height: 0;
}
.side {
  width: 232px;
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.side-title {
  font-size: 11.5px;
  font-weight: 600;
  color: var(--faint);
  padding: 2px 4px 4px;
}
.side-row {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 10px;
  border: 1px solid var(--border-soft);
  border-radius: 10px;
  background: var(--card);
  text-align: left;
  font-family: inherit;
  cursor: pointer;
}
.side-row:hover {
  background: var(--card-hover);
}
.side-row .name {
  font-size: 13px;
  color: var(--text);
}
.side-row .path,
.path-full {
  font-size: 11px;
  color: var(--muted);
  overflow-wrap: anywhere;
}
.side-foot {
  margin-top: auto;
  padding: 6px 4px;
}
.list {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.list-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.up {
  height: 24px;
  padding: 0 10px;
  border-radius: 8px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  font-family: inherit;
  font-size: 12px;
  color: var(--text-soft);
  cursor: pointer;
}
.up:disabled {
  opacity: 0.45;
  cursor: default;
}
.rows {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.row {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 7px 9px;
  border-radius: 9px;
  border: 1px solid transparent;
  background: transparent;
  font-family: inherit;
  text-align: left;
  cursor: pointer;
}
.row:hover {
  background: var(--card-hover);
}
.row.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 7%, transparent);
}
/* 展开箭头：文件行留同宽空白，保证图标列对齐 */
.twist {
  width: 12px;
  flex: none;
  text-align: center;
  font-size: 9.5px;
  line-height: 1;
  color: var(--faint);
  user-select: none;
}
.row .name {
  flex: 1;
  min-width: 0;
  font-size: 13px;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row.folder .name {
  color: var(--text-soft);
  font-weight: 560;
}
.row .kind,
.row .size,
.row .date {
  font-size: 11.5px;
  color: var(--muted);
  white-space: nowrap;
}
.preview {
  width: 40%;
  min-width: 320px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-card);
  padding: 12px 14px;
}
.preview-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border-soft);
}
.preview-head .name {
  font-size: 13.5px;
  font-weight: 600;
}
.preview-body {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.pdf-host {
  flex: 1;
  min-height: 0;
}
.text {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.65;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  color: var(--text);
  user-select: text;
}
.hint {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 28px 10px;
  text-align: center;
  font-size: 12.5px;
  line-height: 1.7;
}
.kind-big {
  font-size: 15px;
  font-weight: 640;
  color: var(--text-soft);
}
.empty {
  font-size: 12.5px;
  color: var(--muted);
  padding: 8px 4px;
}
.empty.placeholder {
  font-size: 12px;
  padding: 6px 4px;
  color: var(--faint);
}
.spacer {
  flex: 1;
}
/* 列表行一律左对齐（button 默认居中） */
.rows .row,
.rows .row > span {
  text-align: left;
}
</style>
