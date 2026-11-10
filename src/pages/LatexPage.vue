<script setup lang="ts">
// 公式：两种视图（对应旧版 page_latex.py）
//   * 文档视图：像 Word 一样写（标题 / 粗斜体 / 下划线 / 颜色 / 对齐 / 列表 / 表格 / 公式块）
//   * LaTeX 源码：直接改源码
// 右边是 KaTeX 即时排版预览 + 真正的 PDF 预览（编译后）。导出仍走本机 XeLaTeX。
import { computed, nextTick, onMounted, ref } from "vue";
import katex from "katex";
import "katex/dist/katex.min.css";
import SoftButton from "../components/SoftButton.vue";
import PdfViewer from "../components/PdfViewer.vue";
import { latexPdf, readFileBase64, settingsAll, settingsSave } from "../api";

const source = ref(`\\section*{特征值与特征向量}

设 $A$ 为 $n$ 阶方阵。若存在数 $\\lambda$ 与非零向量 $x$，使
$$Ax = \\lambda x,$$
则称 $\\lambda$ 为 $A$ 的特征值，$x$ 为对应的特征向量。

特征多项式由下面的行列式给出：
$$\\det(A - \\lambda I) = 0$$

\\begin{enumerate}
  \\item 展开特征多项式得到特征方程
  \\item 解方程得到特征值，再代回求特征向量
\\end{enumerate}
`);

const mode = ref<"doc" | "source">("doc");
const rightTab = ref<"rich" | "pdf">("rich");
const preview = ref("");
const exporting = ref(false);
const status = ref("实时预览");
const pdfData = ref("");
const pdfPath = ref("");
const docEl = ref<HTMLElement | null>(null);
const mathInput = ref("");
const mathDisplay = ref(false);
const docReady = ref(false);
const restored = ref(false);
const restoredDoc = ref(false);
// 旧版 latex_auto（编辑即预览）/ latex_math_display（公式渲染成图还是显示源码）
const autoPreview = ref(true);
const mathSource = ref(false);

const macros: { label: string; latex: string }[] = [
  { label: "分式", latex: "\\frac{a}{b}" },
  { label: "根号", latex: "\\sqrt{x}" },
  { label: "上标", latex: "x^{2}" },
  { label: "下标", latex: "x_{i}" },
  { label: "求和", latex: "\\sum_{i=1}^{n}" },
  { label: "积分", latex: "\\int_{a}^{b} f(x)\\,dx" },
  { label: "极限", latex: "\\lim_{x \\to 0}" },
  { label: "矩阵", latex: "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}" },
  { label: "分段", latex: "\\begin{cases} x, & x > 0 \\\\ 0, & x \\le 0 \\end{cases}" },
];

const greek = ["alpha", "beta", "gamma", "delta", "theta", "lambda", "mu", "pi", "sigma", "omega", "Delta", "Sigma", "Omega"];
const styles = [
  { label: "正文", value: "p" },
  { label: "标题 1", value: "h1" },
  { label: "标题 2", value: "h2" },
  { label: "标题 3", value: "h3" },
];
const colors = [
  { label: "红", hex: "#c0392b", latex: "red" },
  { label: "蓝", hex: "#2d6cdf", latex: "blue" },
  { label: "绿", hex: "#1f8b4c", latex: "green" },
  { label: "橙", hex: "#d97706", latex: "orange" },
  { label: "紫", hex: "#7c4dff", latex: "violet" },
];

const lines = computed(() => source.value.split("\n").length);

function escapeHtml(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function escapeLatex(text: string): string {
  return text.replace(/([\\{}$&#%_^~])/g, (_all, ch) => {
    if (ch === "\\") return "\\textbackslash{}";
    if (ch === "~") return "\\textasciitilde{}";
    if (ch === "^") return "\\textasciicircum{}";
    return "\\" + ch;
  });
}

function katexHtml(latex: string, display: boolean): string {
  try {
    return katex.renderToString(latex, { displayMode: display, throwOnError: true, strict: false });
  } catch {
    return `<code class="math-bad">${escapeHtml(latex)}</code>`;
  }
}

// ---------------------------------------------------------------- KaTeX 预览
function renderInline(text: string): string {
  let out = text.replace(/\$\$([\s\S]+?)\$\$/g, (_m, body) => {
    return `<span class="math-display">${katexHtml(String(body).trim(), true)}</span>`;
  });
  out = out.replace(/\$([^$\n]+?)\$/g, (_m, body) => katexHtml(String(body).trim(), false));
  return out.replace(/\n/g, "<br>");
}

function renderDocument(text: string): string {
  let html = "";
  for (const block of text.split(/\n{2,}/)) {
    const trimmed = block.trim();
    if (!trimmed) continue;
    const section = /^\\(section|subsection|subsubsection)\*?\{(.*)\}$/.exec(trimmed);
    if (section) {
      const tag = section[1] === "section" ? "h2" : section[1] === "subsection" ? "h3" : "h4";
      html += `<${tag}>${escapeHtml(section[2])}</${tag}>`;
      continue;
    }
    if (/^\\begin\{(enumerate|itemize)\}/.test(trimmed)) {
      const ordered = trimmed.startsWith("\\begin{enumerate}");
      const items = trimmed
        .replace(/^\\begin\{(enumerate|itemize)\}/, "")
        .replace(/\\end\{(enumerate|itemize)\}$/, "")
        .split(/\\item/)
        .map((one) => one.trim())
        .filter(Boolean);
      const tag = ordered ? "ol" : "ul";
      html += `<${tag}>${items.map((one) => `<li>${renderInline(one)}</li>`).join("")}</${tag}>`;
      continue;
    }
    if (/^\\\[/.test(trimmed)) {
      const body = trimmed.replace(/^\\\[/, "").replace(/\\\]$/, "");
      html += `<div class="math-display">${katexHtml(body.trim(), true)}</div>`;
      continue;
    }
    html += `<p>${renderInline(trimmed)}</p>`;
  }
  return html;
}

/** LaTeX 源码 → 可编辑文档 HTML（公式变成不可编辑的小卡片，保留 data-latex） */
function docHtmlFromLatex(text: string): string {
  const chip = (latex: string, display: boolean) =>
    `<span class="fchip${display ? " block" : ""}" contenteditable="false" data-latex="${escapeHtml(latex)}" data-display="${display ? "1" : "0"}">${katexHtml(latex, display)}</span>`;

  const inline = (raw: string): string => {
    let out = "";
    let rest = raw;
    while (rest.length) {
      const next = rest.search(/\$[^$\n]+?\$|\$\$[\s\S]+?\$\$/);
      if (next < 0) {
        out += escapeHtml(rest);
        break;
      }
      out += escapeHtml(rest.slice(0, next));
      const match = /^\$\$([\s\S]+?)\$\$|^\$([^$\n]+?)\$/.exec(rest.slice(next));
      if (!match) {
        out += escapeHtml(rest.slice(next));
        break;
      }
      const latex = (match[1] ?? match[2] ?? "").trim();
      out += chip(latex, Boolean(match[1]));
      rest = rest.slice(next + match[0].length);
    }
    return out.replace(/\n/g, "<br>");
  };

  let html = "";
  for (const block of text.split(/\n{2,}/)) {
    const trimmed = block.trim();
    if (!trimmed) continue;
    const section = /^\\(section|subsection|subsubsection)\*?\{(.*)\}$/.exec(trimmed);
    if (section) {
      const tag = section[1] === "section" ? "h1" : section[1] === "subsection" ? "h2" : "h3";
      html += `<${tag}>${escapeHtml(section[2])}</${tag}>`;
      continue;
    }
    if (/^\\begin\{(enumerate|itemize)\}/.test(trimmed)) {
      const ordered = trimmed.startsWith("\\begin{enumerate}");
      const items = trimmed
        .replace(/^\\begin\{(enumerate|itemize)\}/, "")
        .replace(/\\end\{(enumerate|itemize)\}$/, "")
        .split(/\\item/)
        .map((one) => one.trim())
        .filter(Boolean);
      const tag = ordered ? "ol" : "ul";
      html += `<${tag}>${items.map((one) => `<li>${inline(one)}</li>`).join("")}</${tag}>`;
      continue;
    }
    if (/^\\\[/.test(trimmed)) {
      html += `<div>${chip(trimmed.replace(/^\\\[/, "").replace(/\\\]$/, "").trim(), true)}</div>`;
      continue;
    }
    if (/^\$\$[\s\S]+\$\$$/.test(trimmed)) {
      html += `<div>${chip(trimmed.replace(/^\$\$/, "").replace(/\$\$$/, "").trim(), true)}</div>`;
      continue;
    }
    html += `<p>${inline(trimmed)}</p>`;
  }
  return html || "<p><br></p>";
}

// ---------------------------------------------------------------- 编辑区 → LaTeX
function colorName(hex: string): string {
  const hit = colors.find((one) => one.hex.toLowerCase() === hex.toLowerCase());
  return hit ? hit.latex : "";
}

function inlineOf(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return escapeLatex(node.textContent || "");
  if (node.nodeType !== Node.ELEMENT_NODE) return "";
  const el = node as HTMLElement;
  const kids = () => Array.from(el.childNodes).map(inlineOf).join("");
  if (el.classList.contains("fchip")) {
    const latex = el.dataset.latex || "";
    if (!latex) return "";
    return el.dataset.display === "1" ? `\n$$ ${latex} $$\n` : `$${latex}$`;
  }
  const tag = el.tagName;
  if (tag === "BR") return "\n";
  if (tag === "B" || tag === "STRONG") return `\\textbf{${kids()}}`;
  if (tag === "I" || tag === "EM") return `\\textit{${kids()}}`;
  if (tag === "U") return `\\underline{${kids()}}`;
  if (tag === "CODE") return `\\texttt{${kids()}}`;
  if (tag === "SUP") return `\\textsuperscript{${kids()}}`;
  if (tag === "SUB") return `\\textsubscript{${kids()}}`;
  if (tag === "A") return `\\href{${el.getAttribute("href") || ""}}{${kids()}}`;
  const raw = el.style.color || el.getAttribute("color") || "";
  const color = colorName(raw);
  if (color) return `\\textcolor{${color}}{${kids()}}`;
  return kids();
}

function blockOf(el: HTMLElement): string {
  const tag = el.tagName;
  if (/^H[1-6]$/.test(tag)) {
    const level = Number(tag.slice(1));
    const command = level <= 1 ? "section" : level === 2 ? "subsection" : "subsubsection";
    return `\\${command}*{${Array.from(el.childNodes).map(inlineOf).join("").trim()}}`;
  }
  if (tag === "UL" || tag === "OL") {
    const env = tag === "OL" ? "enumerate" : "itemize";
    const items = Array.from(el.children)
      .filter((one) => one.tagName === "LI")
      .map((one) => `  \\item ${Array.from(one.childNodes).map(inlineOf).join("").trim()}`)
      .join("\n");
    return `\\begin{${env}}\n${items}\n\\end{${env}}`;
  }
  if (tag === "TABLE") {
    const rows = Array.from(el.querySelectorAll("tr")).map((row) =>
      Array.from(row.children)
        .filter((cell) => ["TD", "TH"].includes(cell.tagName))
        .map((cell) => Array.from(cell.childNodes).map(inlineOf).join("").trim()),
    );
    const cols = Math.max(1, rows[0]?.length || 1);
    const body = rows.map((cells) => cells.join(" & ") + " \\\\").join("\n");
    return `\\begin{center}\n\\begin{tabular}{|${"c|".repeat(cols)}}\n\\hline\n${body}\n\\hline\n\\end{tabular}\n\\end{center}`;
  }
  if (tag === "BLOCKQUOTE") {
    return `\\begin{quote}\n${Array.from(el.childNodes).map(inlineOf).join("").trim()}\n\\end{quote}`;
  }
  if (tag === "PRE") {
    return `\\begin{verbatim}\n${el.textContent || ""}\n\\end{verbatim}`;
  }
  if (el.classList.contains("fchip") && el.dataset.display === "1") {
    return `$$ ${el.dataset.latex || ""} $$`;
  }
  const text = Array.from(el.childNodes).map(inlineOf).join("").trim();
  return text;
}

function docToLatex(root: HTMLElement | null): string {
  if (!root) return "";
  const blocks: string[] = [];
  for (const node of Array.from(root.childNodes)) {
    if (node.nodeType === Node.TEXT_NODE) {
      const text = escapeLatex(node.textContent || "").trim();
      if (text) blocks.push(text);
      continue;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) continue;
    const el = node as HTMLElement;
    if (el.tagName === "BR") continue;
    const block = blockOf(el).trim();
    if (block) blocks.push(block);
  }
  return blocks.join("\n\n") + "\n";
}

function update(force = false) {
  if (!autoPreview.value && !force) {
    status.value = "预览已暂停（点「刷新预览」）";
    scheduleSave();
    return;
  }
  preview.value = renderDocument(source.value);
  status.value = source.value.trim() ? "即时预览" : "还没有内容";
  scheduleSave();
}

// ---------------------------------------------------------------- 持久化（旧版 latex_source / latex_doc_html）
let saveTimer: number | null = null;
function scheduleSave() {
  if (saveTimer !== null) window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    saveTimer = null;
    persist();
  }, 700);
}

async function persist() {
  try {
    await settingsSave({
      latex_source: source.value,
      latex_doc_html: docEl.value ? docEl.value.innerHTML : "",
    });
    savedAt.value = new Date().toLocaleTimeString();
  } catch {
    // 存不上不影响编辑
  }
}

const savedAt = ref("");

async function restore() {
  try {
    const all = await settingsAll();
    const values: Record<string, string> = all.values || {};
    const raw = values["latex_source"];
    if (raw && raw.trim()) {
      source.value = raw;
      restored.value = true;
    }
    if (values["latex_auto"] === "0") autoPreview.value = false;
    if (values["latex_math_display"] === "source") mathSource.value = true;
    const doc = values["latex_doc_html"] || "";
    await nextTick();
    if (doc.trim() && docEl.value) {
      docEl.value.innerHTML = doc;
      docReady.value = true;
      restoredDoc.value = true;
    }
  } catch {
    // 读不到就用默认样例
  }
}

function onDocInput() {
  source.value = docToLatex(docEl.value);
  update();
}

function paintDoc() {
  if (!docEl.value) return;
  docEl.value.innerHTML = docHtmlFromLatex(source.value);
  docReady.value = true;
}

function exec(command: string, value?: string) {
  docEl.value?.focus();
  document.execCommand(command, false, value);
  onDocInput();
}

function applyColor(latex: string, hex: string) {
  void latex;
  exec("foreColor", hex);
}

function insertHtml(html: string) {
  docEl.value?.focus();
  document.execCommand("insertHTML", false, html);
  onDocInput();
}

function insertFormula(latex: string, display: boolean) {
  const chip = `<span class="fchip${display ? " block" : ""}" contenteditable="false" data-latex="${escapeHtml(
    latex,
  )}" data-display="${display ? "1" : "0"}">${katexHtml(latex, display)}</span>`;
  insertHtml(display ? `<div>${chip}</div>` : chip);
}

function insertMath() {
  const latex = mathInput.value.trim();
  if (!latex) return;
  insertFormula(latex, mathDisplay.value);
  mathInput.value = "";
}

function addMacro(latex: string) {
  mathInput.value = mathInput.value ? `${mathInput.value} ${latex}` : latex;
}

function insertTable() {
  const rows = 3;
  const cols = 3;
  let html = "<table><tbody>";
  for (let r = 0; r < rows; r += 1) {
    html += "<tr>";
    for (let c = 0; c < cols; c += 1) {
      html += r === 0 ? "<th>&nbsp;</th>" : "<td>&nbsp;</td>";
    }
    html += "</tr>";
  }
  html += "</tbody></table><p><br></p>";
  insertHtml(html);
}

async function switchMode(next: "doc" | "source") {
  mode.value = next;
  if (next === "doc") {
    await nextTick();
    paintDoc();
  }
}

async function rebuildFromSource() {
  await nextTick();
  paintDoc();
  status.value = "已按源码重建文档视图";
}

async function exportPdf() {
  if (exporting.value) return;
  exporting.value = true;
  status.value = "正在用 XeLaTeX 编译 PDF…";
  try {
    const result = await latexPdf(source.value);
    if (result.ok) {
      pdfPath.value = result.path;
      status.value = "PDF 已生成：" + result.path.split("\\").pop();
      try {
        const file = await readFileBase64(result.path);
        pdfData.value = file.data;
        rightTab.value = "pdf";
      } catch (err) {
        status.value = "PDF 生成了，但没能读回来预览：" + String(err);
      }
    } else {
      status.value = result.error;
    }
  } catch (err) {
    status.value = String(err);
  } finally {
    exporting.value = false;
  }
}

onMounted(async () => {
  await restore();
  update();
  await nextTick();
  paintDoc();
});
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">公式</h1>
        <div class="muted">文档视图像 Word 一样写，LaTeX 源码随时可改</div>
      </div>
      <div class="modes">
        <button class="tab" :class="{ on: mode === 'doc' }" @click="switchMode('doc')">文档视图</button>
        <button class="tab" :class="{ on: mode === 'source' }" @click="switchMode('source')">
          LaTeX 源码
        </button>
      </div>
      <div class="spacer" />
      <SoftButton
        v-if="mode === 'source'"
        :height="30"
        :font-size="12"
        @click="rebuildFromSource"
      >
        按源码重建文档
      </SoftButton>
      <SoftButton
        :height="30"
        :font-size="12"
        glyph="DOWNLOAD"
        :disabled="exporting"
        @click="exportPdf"
      >
        {{ exporting ? "编译中…" : "导出 PDF" }}
      </SoftButton>
      <SoftButton variant="primary" :height="30" :font-size="12" @click="update(true)">刷新预览</SoftButton>
    </div>

    <div v-if="mode === 'doc'" class="toolbar">
      <div class="row">
        <select class="sel" @change="exec('formatBlock', ($event.target as HTMLSelectElement).value)">
          <option v-for="item in styles" :key="item.value" :value="item.value">
            {{ item.label }}
          </option>
        </select>
        <button class="tool" title="加粗" @click="exec('bold')"><b>B</b></button>
        <button class="tool" title="斜体" @click="exec('italic')"><i>I</i></button>
        <button class="tool" title="下划线" @click="exec('underline')"><u>U</u></button>
        <button
          v-for="item in colors"
          :key="item.latex"
          class="dot"
          :style="{ background: item.hex }"
          :title="`字体颜色：${item.label}`"
          @click="applyColor(item.latex, item.hex)"
        />
        <span class="gap" />
        <button class="tool" title="左对齐" @click="exec('justifyLeft')">左</button>
        <button class="tool" title="居中" @click="exec('justifyCenter')">中</button>
        <button class="tool" title="右对齐" @click="exec('justifyRight')">右</button>
        <span class="gap" />
        <button class="tool wide" title="无序列表" @click="exec('insertUnorderedList')">• 列表</button>
        <button class="tool wide" title="有序列表" @click="exec('insertOrderedList')">1. 列表</button>
        <button class="tool wide" title="插入表格" @click="insertTable">表格</button>
      </div>
      <div class="row">
        <input
          v-model="mathInput"
          class="math-input"
          placeholder="写 LaTeX，例如 \frac{a}{b}，回车插入"
          spellcheck="false"
          @keydown.enter.prevent="insertMath"
        />
        <button class="tool wide" @click="mathDisplay = false, insertMath()">插入行内公式</button>
        <button class="tool wide" @click="mathDisplay = true, insertMath()">插入行间公式</button>
        <span class="gap" />
        <button v-for="macro in macros" :key="macro.label" class="tool" @click="addMacro(macro.latex)">
          {{ macro.label }}
        </button>
        <span class="gap" />
        <button
          v-for="name in greek"
          :key="name"
          class="tool greek"
          :title="`\\${name}`"
          @click="addMacro(`\\${name}`)"
        >
          {{ name === name.toUpperCase() ? name : "\\" + name }}
        </button>
      </div>
    </div>

    <div class="body">
      <div class="pane">
        <div class="pane-head">
          <span class="label">{{ mode === "doc" ? "文档" : "源码" }}</span>
          <span class="spacer" />
          <span class="tiny">{{ lines }} 行 · {{ source.length }} 字</span>
        </div>
        <div
          v-show="mode === 'doc'"
          ref="docEl"
          class="doc"
          contenteditable="true"
          spellcheck="false"
          @input="onDocInput"
        />
        <textarea
          v-show="mode === 'source'"
          v-model="source"
          class="source"
          spellcheck="false"
          @input="update()"
        />
      </div>
      <div class="pane">
        <div class="pane-head">
          <button class="tab" :class="{ on: rightTab === 'rich' }" @click="rightTab = 'rich'">
            排版预览
          </button>
          <button class="tab" :class="{ on: rightTab === 'pdf' }" @click="rightTab = 'pdf'">
            PDF 预览
          </button>
          <span class="spacer" />
          <span class="tiny">{{ status }}</span>
        </div>
        <div v-show="rightTab === 'rich'" class="preview" v-html="preview" />
        <div v-show="rightTab === 'pdf'" class="pdf-wrap">
          <PdfViewer v-if="pdfData" :data="pdfData" mode="single" />
          <div v-else class="hint muted">
            点「导出 PDF」编译一次，这里就会显示真正的排版结果（和打印出来一致）
          </div>
        </div>
      </div>
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
.modes {
  display: flex;
  gap: 4px;
}
.tab {
  height: 26px;
  padding: 0 12px;
  border-radius: 8px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  color: var(--text-soft);
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
  text-align: center;
}
.tab.on {
  background: var(--accent-soft);
  color: var(--accent);
  border-color: var(--accent);
}
.toolbar {
  display: flex;
  flex-direction: column;
  gap: 5px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  border-radius: 10px;
  padding: 7px 9px;
}
.toolbar .row {
  display: flex;
  gap: 5px;
  align-items: center;
  flex-wrap: wrap;
}
.tool {
  height: 26px;
  min-width: 28px;
  padding: 0 8px;
  border-radius: 7px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text-soft);
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
  text-align: center;
}
.tool:hover {
  background: var(--card-hover);
  color: var(--text);
}
.tool.wide {
  padding: 0 10px;
}
.tool.greek {
  font-size: 11.5px;
  color: var(--muted);
}
.dot {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 1px solid var(--border-soft);
  cursor: pointer;
  padding: 0;
}
.sel {
  height: 26px;
  border-radius: 7px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text-soft);
  font-family: inherit;
  font-size: 12px;
}
.gap {
  width: 8px;
}
.math-input {
  flex: 1;
  min-width: 200px;
  height: 26px;
  border-radius: 7px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 12px;
  padding: 0 8px;
}
.body {
  flex: 1;
  display: flex;
  gap: 12px;
  min-height: 0;
}
.pane {
  text-align: left;
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: 12px;
  padding: 9px 11px;
  min-height: 0;
}
.pane-head {
  display: flex;
  align-items: center;
  gap: 6px;
}
.pane-head .label {
  font-size: 12px;
  color: var(--muted);
  letter-spacing: 0.4px;
}
.spacer {
  flex: 1;
}
.tiny {
  font-size: 11px;
  color: var(--muted);
}
.doc {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  outline: none;
  font-size: 13.5px;
  line-height: 1.85;
  color: var(--text);
  padding: 4px 2px;
}
.doc :deep(h1) {
  font-size: 19px;
  margin: 12px 0 8px;
}
.doc :deep(h2) {
  font-size: 16.5px;
  margin: 10px 0 6px;
}
.doc :deep(h3) {
  font-size: 14.5px;
  margin: 8px 0 5px;
}
.doc :deep(p) {
  margin: 6px 0;
}
.doc :deep(ul),
.doc :deep(ol) {
  margin: 6px 0;
  padding-left: 22px;
}
.doc :deep(table) {
  border-collapse: collapse;
  margin: 8px 0;
}
.doc :deep(th),
.doc :deep(td) {
  border: 1px solid var(--border-soft);
  padding: 4px 10px;
  min-width: 60px;
  font-size: 12.5px;
}
.doc :deep(.fchip) {
  display: inline-block;
  padding: 0 5px;
  border-radius: 6px;
  background: var(--accent-soft);
  border: 1px dashed var(--accent);
  cursor: default;
  user-select: none;
}
.doc :deep(.fchip.block) {
  display: block;
  padding: 6px 10px;
  margin: 6px 0;
  text-align: center;
}
.source {
  flex: 1;
  min-height: 0;
  resize: none;
  border: 0;
  background: transparent;
  color: var(--text);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.7;
  outline: none;
  text-align: left;
}
.preview {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  font-size: 13.5px;
  line-height: 1.85;
  color: var(--text);
}
.preview :deep(h2) {
  font-size: 17px;
  margin: 12px 0 6px;
}
.preview :deep(h3) {
  font-size: 15px;
  margin: 10px 0 5px;
}
.preview :deep(h4) {
  font-size: 14px;
  margin: 8px 0 4px;
}
.preview :deep(ul),
.preview :deep(ol) {
  padding-left: 22px;
  margin: 6px 0;
}
.preview :deep(p) {
  margin: 6px 0;
}
.preview :deep(.math-display) {
  display: block;
  text-align: center;
  margin: 8px 0;
}
.preview :deep(.math-bad) {
  color: var(--danger);
  font-family: var(--font-mono);
  font-size: 12px;
}
.pdf-wrap {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.hint {
  display: flex;
  align-items: center;
  justify-content: center;
  flex: 1;
  text-align: center;
  font-size: 12.5px;
  padding: 20px;
}
</style>
