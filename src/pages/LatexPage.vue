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
import { latexFormulaFromText, latexPdf, readFileBase64, settingsAll, settingsSave } from "../api";

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
/** 编辑即编译（旧版 `latex_auto`，默认开）：停手 400ms 就编一次 PDF */
const autoCompile = ref(true);
let compileTimer = 0;
/** 编译期间又有改动 → 编完再补一次 */
let compileAgain = false;

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

function katexHtml(latex: string, display: boolean): string {
  try {
    return katex.renderToString(latex, { displayMode: display, throwOnError: true, strict: false });
  } catch {
    return `<code class="math-bad">${escapeHtml(latex)}</code>`;
  }
}

// ---------------------------------------------------------------- KaTeX 预览
/** 行内：`\textbf` 之类的排版命令也吃掉，预览里不该出现 LaTeX 语法 */
function renderInline(text: string): string {
  let out = text;
  // 先处理行内命令（可以嵌套一层）
  for (let round = 0; round < 4; round += 1) {
    const before = out;
    out = out.replace(/\\(textbf|textit|emph|underline|texttt|textsuperscript|textsubscript)\{([^{}]*)\}/g,
      (_m, cmd: string, body: string) => {
        const tag: Record<string, string> = {
          textbf: "strong",
          textit: "em",
          emph: "em",
          underline: "u",
          texttt: "code",
          textsuperscript: "sup",
          textsubscript: "sub",
        };
        return `<${tag[cmd]}>${body}</${tag[cmd]}>`;
      });
    out = out.replace(/\\textcolor\{[^}]*\}\{([^{}]*)\}/g, (_m, body: string) => body);
    if (out === before) break;
  }
  // 公式（HTML 标签已经插好了，这里只替换 $…$，注意别把标签属性里的 $ 换掉）
  const parts: string[] = [];
  let rest = out;
  const pattern = /\$\$([\s\S]+?)\$\$|\$([^$\n]+?)\$/;
  let guard = 0;
  while (guard < 500) {
    guard += 1;
    const match = pattern.exec(rest);
    if (!match) break;
    // 找出公式前面残留的未转义 LaTeX 命令（认不出的）
    parts.push(match.index > 0 ? rest.slice(0, match.index) : "");
    parts.push(
      match[1] !== undefined
        ? `<span class="math-display">${katexHtml(String(match[1]).trim(), true)}</span>`
        : katexHtml(String(match[2]).trim(), false),
    );
    rest = rest.slice(match.index + match[0].length);
  }
  parts.push(rest);
  const joined = parts.join("");
  // 认不出来的命令（`\begin{...}` / `\end{...}` 之类）不要留在预览里
  const cleaned = joined
    .replace(/\\(begin|end)\{[^}]*\}/g, "")
    .replace(/\\(left|right|big|Big|bigg|Bigg)\b/g, "");
  return cleaned.replace(/\n/g, "<br>");
}

function renderDocument(text: string): string {
  let html = "";
  for (const block of text.split(/\n{2,}/)) {
    const trimmed = block.trim();
    if (!trimmed) continue;
    const section = /^\\(section|subsection|subsubsection)\*?\{(.*)\}$/.exec(trimmed);
    if (section) {
      const tag = section[1] === "section" ? "h2" : section[1] === "subsection" ? "h3" : "h4";
      // 标题内容也要过一遍行内命令：以前直接 escapeHtml，标题里一旦有 `\textbf{…}`
      // 就在预览里原样显示（用户 2026-11-11 圈出来的那行）
      html += `<${tag}>${renderInline(section[2])}</${tag}>`;
      continue;
    }
    // \begin{center} … \end{center} → 居中块
    const center = /^\\begin\{center\}([\s\S]*?)(?:\\end\{center\})?$/.exec(trimmed);
    if (center) {
      const inner = renderInline(center[1].trim());
      if (inner.trim()) html += `<div class="center-block">${inner}</div>`;
      continue;
    }
    if (/^\\begin\{(enumerate|itemize)\}/.test(trimmed)) {
      const ordered = trimmed.startsWith("\\begin{enumerate}");
      // `\end{...}` 后面可能还跟着一段文字（源码里没空行分隔），
      // 以前只去掉「结尾处」的 \end，于是预览里会把 `\end{enumerate}` 当正文显示出来。
      const endTag = ordered ? "\\end{enumerate}" : "\\end{itemize}";
      const endAt = trimmed.lastIndexOf(endTag);
      const listPart = endAt >= 0 ? trimmed.slice(0, endAt) : trimmed;
      const rest = endAt >= 0 ? trimmed.slice(endAt + endTag.length).trim() : "";
      const items = listPart
        .replace(/^\\begin\{(enumerate|itemize)\}/, "")
        .split(/\\item/)
        .map((one) => one.trim())
        .filter(Boolean);
      const tag = ordered ? "ol" : "ul";
      html += `<${tag}>${items.map((one) => `<li>${renderInline(one)}</li>`).join("")}</${tag}>`;
      if (rest) html += `<p>${renderInline(rest)}</p>`;
      continue;
    }
    if (/^\\\[/.test(trimmed)) {
      const body = trimmed.replace(/^\\\[/, "").replace(/\\\]$/, "");
      html += `<div class="math-display">${katexHtml(body.trim(), true)}</div>`;
      continue;
    }
    // 落单的 \end{...} / \begin{...}
    if (/^\\(end|begin)\{[^}]*\}$/.test(trimmed)) continue;
    html += `<p>${renderInline(trimmed)}</p>`;
  }
  return html;
}

/** LaTeX 源码 → 可编辑文档 HTML（公式变成不可编辑的小卡片，保留 data-latex）
 *
 * 目标是「Word 那种体验」：**编辑区里不该出现 LaTeX 语法**。
 * 所以这里要把常见的行内命令也吃掉：`\textbf{}` → 加粗、`\begin{center}` → 居中块、
 * `\item` → 编号/项目符号列表，`$…$` → 渲染好的小卡片；认不出来的命令才原样显示。
 */
function docHtmlFromLatex(text: string): string {
  const chip = (latex: string, display: boolean) =>
    `<span class="fchip${display ? " block" : ""}" contenteditable="false" data-latex="${escapeHtml(latex)}" data-display="${display ? "1" : "0"}">${katexHtml(latex, display)}</span>`;

  /** 从 `\cmd{...}` 里取出花括号内的内容（支持嵌套） */
  const takeBraced = (raw: string, start: number): { body: string; end: number } | null => {
    if (raw[start] !== "{") return null;
    let depth = 0;
    for (let index = start; index < raw.length; index += 1) {
      const ch = raw[index];
      if (ch === "{") depth += 1;
      else if (ch === "}") {
        depth -= 1;
        if (depth === 0) return { body: raw.slice(start + 1, index), end: index + 1 };
      }
    }
    return null;
  };

  /** 行内：公式 → 卡片；常见排版命令 → 对应标签；其余转义后原样 */
  const inline = (raw: string): string => {
    let out = "";
    let index = 0;
    while (index < raw.length) {
      const rest = raw.slice(index);
      // 1) 数学：$$…$$ 或 $…$
      const math = /^\$\$([\s\S]+?)\$\$|^\$([^$\n]+?)\$/.exec(rest);
      if (math) {
        out += chip((math[1] ?? math[2] ?? "").trim(), Boolean(math[1]));
        index += math[0].length;
        continue;
      }
      // 2) 行内排版命令
      const command = /^\\(textbf|textit|underline|texttt|textsuperscript|textsubscript|emph)\{/.exec(rest);
      if (command) {
        const braced = takeBraced(raw, index + command[0].length - 1);
        if (braced) {
          const tag: Record<string, string> = {
            textbf: "b",
            textit: "i",
            emph: "i",
            underline: "u",
            texttt: "code",
            textsuperscript: "sup",
            textsubscript: "sub",
          };
          out += `<${tag[command[1]]}>${inline(braced.body)}</${tag[command[1]]}>`;
          index = braced.end;
          continue;
        }
      }
      // 3) \textcolor{颜色}{内容}
      const colored = /^\\textcolor\{[^}]*\}\{/.exec(rest);
      if (colored) {
        const braced = takeBraced(raw, index + colored[0].length - 1);
        if (braced) {
          out += inline(braced.body);
          index = braced.end;
          continue;
        }
      }
      // 4) 普通字符（逐字转义，遇到下一个特殊符号再进循环）
      const nextSpecial = raw.slice(index + 1).search(/[$\\]/);
      if (nextSpecial < 0) {
        out += escapeHtml(raw.slice(index));
        break;
      }
      out += escapeHtml(raw.slice(index, index + 1 + nextSpecial));
      index += 1 + nextSpecial;
    }
    return out;
  };

  let html = "";
  for (const block of text.split(/\n{2,}/)) {
    const trimmed = block.trim();
    if (!trimmed) continue;
    const section = /^\\(section|subsection|subsubsection)\*?\{(.*)\}$/.exec(trimmed);
    if (section) {
      const tag = section[1] === "section" ? "h1" : section[1] === "subsection" ? "h2" : "h3";
      html += `<${tag}>${inline(section[2])}</${tag}>`;
      continue;
    }
    // \begin{center} … \end{center} → 居中块（里面的公式照样变成卡片）
    const center = /^\\begin\{center\}([\s\S]*?)(?:\\end\{center\})?$/.exec(trimmed);
    if (center) {
      // 空 center 直接跳过：转一次会留下一个空环境，编译出来是一条压扁的空行
      const inner = inline(center[1].trim());
      if (inner.trim()) html += `<p style="text-align: center;">${inner}</p>`;
      continue;
    }
    if (/^\\begin\{(enumerate|itemize)\}/.test(trimmed)) {
      const ordered = trimmed.startsWith("\\begin{enumerate}");
      const endTag = ordered ? "\\end{enumerate}" : "\\end{itemize}";
      const endAt = trimmed.lastIndexOf(endTag);
      const listPart = endAt >= 0 ? trimmed.slice(0, endAt) : trimmed;
      const rest = endAt >= 0 ? trimmed.slice(endAt + endTag.length).trim() : "";
      const items = listPart
        .replace(/^\\begin\{(enumerate|itemize)\}/, "")
        .split(/\\item/)
        .map((one) => one.trim())
        .filter(Boolean);
      const tag = ordered ? "ol" : "ul";
      html += `<${tag}>${items.map((one) => `<li>${inline(one)}</li>`).join("")}</${tag}>`;
      if (rest) html += `<p>${inline(rest)}</p>`;
      continue;
    }
    if (/^\\\[/.test(trimmed)) {
      html += `<div style="text-align: center;">${chip(trimmed.replace(/^\\\[/, "").replace(/\\\]$/, "").trim(), true)}</div>`;
      continue;
    }
    if (/^\$\$[\s\S]+\$\$$/.test(trimmed)) {
      html += `<div style="text-align: center;">${chip(trimmed.replace(/^\$\$/, "").replace(/\$\$$/, "").trim(), true)}</div>`;
      continue;
    }
    // 落单的 \end{...} / \begin{...}（源码里不配对时）直接丢掉，别让它变成正文
    if (/^\\(end|begin)\{[^}]*\}$/.test(trimmed)) continue;
    // 一个空行一段：这里**每块单独成段**，块内换行用 <br>（以前把几块拼在一起，
    // 标题就和正文粘成一行了）
    html += `<p>${inline(trimmed).replace(/\n/g, "<br>")}</p>`;
  }
  return html || "<p><br></p>";
}

// ---------------------------------------------------------------- 编辑区 → LaTeX
function colorName(hex: string): string {
  const hit = colors.find((one) => one.hex.toLowerCase() === hex.toLowerCase());
  return hit ? hit.latex : "";
}

function inlineOf(node: Node): string {
  // 纯文本原样保留：以前这里会把 `\ { } $ …` 都转义成 `\textbackslash{}`，
  // 于是一段手写的 LaTeX（比如 `\begin{bmatrix}…`）只要被编辑器存过一次，
  // 源码就变成 `\textbackslash{}textbackslash\{\}…` 这种乱码（用户报的「公式页全是反斜杠」就是这个）。
  if (node.nodeType === Node.TEXT_NODE) return node.textContent || "";
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

/** 去掉 `\textbf{…}` 外壳（保留里面内容）：标题里不该再套加粗 */
function unwrapBold(text: string): string {
  let out = "";
  let index = 0;
  while (index < text.length) {
    const at = text.indexOf("\\textbf{", index);
    if (at < 0) {
      out += text.slice(index);
      break;
    }
    out += text.slice(index, at);
    let depth = 0;
    let end = at + "\\textbf".length;
    for (; end < text.length; end += 1) {
      if (text[end] === "{") depth += 1;
      else if (text[end] === "}") {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    out += text.slice(at + "\\textbf{".length, end);
    index = end + 1;
  }
  return out;
}

function blockOf(el: HTMLElement): string {
  const tag = el.tagName;
  if (/^H[1-6]$/.test(tag)) {
    const level = Number(tag.slice(1));
    const command = level <= 1 ? "section" : level === 2 ? "subsection" : "subsubsection";
    // 标题本来就是粗体，**不要再套一层 `\textbf{…}`**：套了之后预览里会原样显示
    // `\textbf{特征值与特征向量}`（用户 2026-11-11 圈出来的那行）。
    const text = unwrapBold(Array.from(el.childNodes).map(inlineOf).join("")).trim();
    return `\\${command}*{${text}}`;
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
  // 居中块（\begin{center}…\end{center}）：编辑区里就是「居中的一段」，
  // 转回源码要还原成 center 环境，否则再打开就不居中了
  const align = (el.style.textAlign || el.getAttribute("align") || "").toLowerCase();
  if (align === "center") {
    const inner = Array.from(el.childNodes).map(inlineOf).join("").trim();
    // 空的居中块不要写成 `\begin{center}\n\n\end{center}`（编译出来是一条压扁的空行）
    return inner ? `\\begin{center}\n${inner}\n\\end{center}` : "";
  }
  const text = Array.from(el.childNodes).map(inlineOf).join("").trim();
  return text;
}

function docToLatex(root: HTMLElement | null): string {
  if (!root) return "";
  const blocks: string[] = [];
  for (const node of Array.from(root.childNodes)) {
    if (node.nodeType === Node.TEXT_NODE) {
      // 同上：纯文本不再做 LaTeX 转义，否则源码会被写成 `\textbackslash{}…`
      const text = (node.textContent || "").trim();
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
    scheduleCompile();
    return;
  }
  preview.value = renderDocument(source.value);
  status.value = source.value.trim() ? "即时预览" : "还没有内容";
  scheduleSave();
  scheduleCompile();
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
      latex_source: sanitizeLatex(source.value),
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
    if (values["latex_auto_compile"] === "0") autoCompile.value = false;
    if (values["latex_math_display"] === "source") mathSource.value = true;
    // **文档视图一律按源码重建**：以前优先用 `latex_doc_html`，那里面存的是旧编辑器留下的
    // 半截 HTML（`\textbf{…}`、`\begin{center}`、落单的 `\end{enumerate}` 全是裸文本），
    // 打开就是一堆 LaTeX 语法（用户 2026-11-11：「word 编辑处不应该出现 LaTeX 语法」）。
    // 只有源码为空时才退回去用存下的 HTML。
    const doc = values["latex_doc_html"] || "";
    await nextTick();
    if (!source.value.trim() && doc.trim() && docEl.value) {
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
  // 撤销栈：每次编辑都记一份源码（连续打字会合并成一步）
  pushUndo(source.value);
  update();
}

function paintDoc() {
  if (!docEl.value) return;
  docEl.value.innerHTML = docHtmlFromLatex(source.value);
  docReady.value = true;
  // 换了一份内容（打开文档 / 撤销恢复）也记一笔，撤销栈才连得上
  pushUndo(source.value);
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

// ---- 图形化矩阵输入（旧版 latex_matrix.py）：填格子 → 生成 LaTeX
const matrixOpen = ref(false);
const matrixRows = ref(3);
const matrixCols = ref(3);
const matrixEnv = ref("pmatrix");
const matrixCells = ref<string[][]>([]);

function resizeMatrix(rows: number, cols: number) {
  const next: string[][] = [];
  for (let r = 0; r < rows; r += 1) {
    const row: string[] = [];
    for (let c = 0; c < cols; c += 1) row.push(matrixCells.value[r]?.[c] || "");
    next.push(row);
  }
  matrixCells.value = next;
  matrixRows.value = rows;
  matrixCols.value = cols;
}

function openMatrix() {
  resizeMatrix(matrixRows.value, matrixCols.value);
  matrixOpen.value = true;
}

function buildMatrix(): string {
  const body = matrixCells.value
    .map((row) => row.map((cell) => (cell.trim() || "0")).join(" & "))
    .join(" \\\\ ");
  return `\\begin{${matrixEnv.value}} ${body} \\end{${matrixEnv.value}}`;
}

function insertMatrix() {
  const latex = buildMatrix();
  insertFormula(latex, true);
  matrixOpen.value = false;
}

function useMatrixSource() {
  addMacro(buildMatrix());
  matrixOpen.value = false;
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

// ------------------------------------------------------------------ 快捷键 / 剪贴板
//
// 用户 2026-11-11：文档相关的快捷键尽量都支持（参照 Word），并且要能选中公式剪切粘贴、
// 选中内容一键转公式。

/** 撤销/重做用的历史：直接存源码快照（文档视图 ↔ 源码来回转也不会乱） */
const undoStack = ref<string[]>([]);
let undoAt = -1;
let lastPushAt = 0;

/** 记一份快照；连续打字时只更新最上面那份（不然撤销一次退一个字） */
function pushUndo(text: string) {
  const now = Date.now();
  const last = undoAt >= 0 ? undoStack.value[undoAt] : null;
  if (last === text) return;
  const merging = last !== null && text.startsWith(last) && now - lastPushAt < 800;
  if (merging) {
    undoStack.value[undoAt] = text;
  } else {
    undoStack.value = undoStack.value.slice(0, undoAt + 1);
    undoStack.value.push(text);
    if (undoStack.value.length > 200) undoStack.value.shift();
    undoAt = undoStack.value.length - 1;
  }
  lastPushAt = now;
}

function applySnapshot(text: string) {
  source.value = text;
  paintDoc();
  update();
}

function undoDoc(): boolean {
  if (undoAt <= 0) {
    status.value = "已经撤到底了";
    return false;
  }
  undoAt -= 1;
  applySnapshot(undoStack.value[undoAt]);
  return true;
}

function redoDoc(): boolean {
  if (undoAt >= undoStack.value.length - 1) {
    status.value = "没有更多可以重做了";
    return false;
  }
  undoAt += 1;
  applySnapshot(undoStack.value[undoAt]);
  return true;
}

/** 排版类命令（加粗/对齐…）：只对文档视图有意义，源码视图直接忽略 */
function formatDoc(command: string) {
  if (mode.value !== "doc") return;
  docEl.value?.focus();
  document.execCommand(command, false);
  onDocInput();
}

/** 键盘：照 Word 那一套；源码视图只接管保存和公式（撤销/剪切交给浏览器原生） */
function onEditorKey(event: KeyboardEvent) {
  const mod = event.ctrlKey || event.metaKey;
  const key = event.key.toLowerCase();
  const inSource = (event.target as HTMLElement | null)?.tagName === "TEXTAREA";
  // Word 里 Alt+= 是「插入公式」
  if (event.altKey && key === "=" && !inSource) {
    event.preventDefault();
    mathInputFocus();
    return;
  }
  if (!mod) return;
  // 保存（Ctrl+S）
  if (key === "s") {
    event.preventDefault();
    void persist().then(() => {
      status.value = `已保存（${savedAt.value}）`;
    });
    return;
  }
  // 公式相关：Ctrl+Shift+M 选中转公式、Ctrl+Shift+E 让 AI 转
  if (event.shiftKey && key === "m") {
    event.preventDefault();
    convertSelection(false);
    return;
  }
  if (event.shiftKey && key === "e") {
    event.preventDefault();
    convertSelection(true);
    return;
  }
  if (inSource) return; // 源码视图剩下的交给浏览器（原生撤销、选择都正常）
  if (key === "z") {
    event.preventDefault();
    if (event.shiftKey) redoDoc();
    else undoDoc();
    return;
  }
  if (key === "y") {
    event.preventDefault();
    redoDoc();
    return;
  }
  if (key === "b") {
    event.preventDefault();
    formatDoc("bold");
    return;
  }
  if (key === "i") {
    event.preventDefault();
    formatDoc("italic");
    return;
  }
  if (key === "u") {
    event.preventDefault();
    formatDoc("underline");
    return;
  }
  if (event.shiftKey && key === "x") {
    event.preventDefault();
    formatDoc("strikeThrough");
    return;
  }
  if (key === "e") {
    event.preventDefault();
    formatDoc("justifyCenter");
    return;
  }
  if (key === "l") {
    event.preventDefault();
    formatDoc("justifyLeft");
    return;
  }
  if (key === "r") {
    event.preventDefault();
    formatDoc("justifyRight");
    return;
  }
  if (key === "j") {
    event.preventDefault();
    formatDoc("justifyFull");
    return;
  }
}

function mathInputFocus() {
  const el = document.querySelector<HTMLInputElement>(".math-input");
  el?.focus();
}

/** 点一下公式块：整块选中（这样剪切 / 删除 / 转公式都好操作） */
function onDocClick(event: MouseEvent) {
  const target = event.target as HTMLElement | null;
  // 点到公式里的 KaTeX 子元素也算（target 往往是里面那层 span，不是 .fchip 本身）
  const chip = target?.closest?.(".fchip") as HTMLElement | null;
  if (!chip) return;
  const range = document.createRange();
  range.selectNode(chip);
  const selection = window.getSelection();
  if (!selection) return;
  selection.removeAllRanges();
  selection.addRange(range);
}

/** 把一段节点转成 LaTeX（专门给复制用，不做 docToLatex 那套嵌套展开，避免同一个公式被写好几遍） */
function nodeToLatex(node: Node): string {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent || "";
  if (node.nodeType !== Node.ELEMENT_NODE) return "";
  const el = node as HTMLElement;
  if (el.classList.contains("fchip")) {
    const latex = el.dataset.latex || "";
    if (!latex) return "";
    return el.dataset.display === "1" ? `$$ ${latex} $$` : `$${latex}$`;
  }
  const kids = Array.from(el.childNodes).map(nodeToLatex).join("");
  switch (el.tagName) {
    case "BR":
      return "\n";
    case "B":
    case "STRONG":
      return `\\textbf{${kids}}`;
    case "I":
    case "EM":
      return `\\textit{${kids}}`;
    case "U":
      return `\\underline{${kids}}`;
    case "CODE":
      return `\\texttt{${kids}}`;
    case "SUP":
      return `\\textsuperscript{${kids}}`;
    case "SUB":
      return `\\textsubscript{${kids}}`;
    case "DIV":
    case "P":
    case "LI":
    case "H1":
    case "H2":
    case "H3":
    case "TR":
      return kids ? `${kids}\n` : "";
    default:
      return kids;
  }
}

/** 复制 / 剪切：选中的公式要变成 `$…$`，不能把渲染后的排版字复制出去 */
function onDocCopy(event: ClipboardEvent, cut: boolean) {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed) return;
  const range = selection.getRangeAt(0);
  if (!docEl.value || !docEl.value.contains(range.commonAncestorContainer)) return;
  const holder = document.createElement("div");
  holder.appendChild(range.cloneContents());
  const plain = holder.textContent?.trim()
    ? Array.from(holder.childNodes).map(nodeToLatex).join("").trim()
    : "";
  event.clipboardData?.setData("text/plain", plain || selection.toString());
  // text/html 留个记号：在本文档里粘贴时直接还原成公式块（不走文本解析，不会被拆坏）
  event.clipboardData?.setData(
    "text/html",
    `<!--danlume-doc--><div>${holder.innerHTML}</div>`,
  );
  event.preventDefault();
  if (cut) {
    // 用 range.deleteContents() 精确删掉选区。以前用 `execCommand("delete")`，
    // 选区是「整块不可编辑的公式」时会连旁边的字一起吃掉（2026-11-11 实测把段落里的
    // 公式和标点都删了）。这里只删选区本身。
    range.deleteContents();
    selection.removeAllRanges();
    onDocInput();
  }
}

/** 粘贴：优先还原本文档自己的 HTML（公式块原样回来），否则按文本里的 `$…$` 转 */
function onDocPaste(event: ClipboardEvent) {
  // 原生粘贴（Ctrl+Shift+V）：保留纯文本
  if ((event as ClipboardEvent & { shiftKey?: boolean }).shiftKey) return;
  const html = event.clipboardData?.getData("text/html") || "";
  const text = event.clipboardData?.getData("text/plain") || "";
  if (html.includes("<!--danlume-doc-->") && html.includes("data-latex")) {
    event.preventDefault();
    insertHtml(sanitizeDocHtml(html));
    return;
  }
  if (!text.trim()) return; // 图片之类交给浏览器
  event.preventDefault();
  insertHtml(textToDocHtml(text));
}

/** 只保留我们认识的标签和公式块（剪贴板里的东西不能直接塞进文档） */
function sanitizeDocHtml(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const out: string[] = [];
  const walk = (node: Node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      out.push(escapeHtml(node.textContent || ""));
      return;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) return;
    const el = node as HTMLElement;
    if (el.classList.contains("fchip")) {
      const latex = el.dataset.latex || "";
      if (latex) out.push(chipHtml(latex, el.dataset.display === "1"));
      return;
    }
    const tag = el.tagName.toLowerCase();
    const allowed = ["b", "strong", "i", "em", "u", "code", "sup", "sub", "p", "div", "br"];
    if (tag === "br") {
      out.push("<br>");
      return;
    }
    if (!allowed.includes(tag)) {
      Array.from(el.childNodes).forEach(walk);
      return;
    }
    const before = out.length;
    out.push(`<${tag}>`);
    Array.from(el.childNodes).forEach(walk);
    out.push(`</${tag}>`);
    if (out.length === before + 2) out.splice(before, 2); // 空标签不要
  };
  Array.from(doc.body.childNodes).forEach(walk);
  return out.join("") || "<p><br></p>";
}

/** 纯文本 → 文档 HTML：`$$…$$` 变行间公式、`$…$` 变行内公式，其余按段落 */
function textToDocHtml(text: string): string {
  const blocks: string[] = [];
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  let buffer: string[] = [];
  const flush = () => {
    if (!buffer.length) return;
    const body = buffer
      .map((line) => inlineTextToHtml(line))
      .join("<br>");
    blocks.push(`<p>${body || "<br>"}</p>`);
    buffer = [];
  };
  for (const line of lines) {
    const display = line.match(/^\s*\$\$(.+?)\$\$\s*$/);
    if (display) {
      flush();
      blocks.push(chipHtml(display[1].trim(), true));
      continue;
    }
    buffer.push(line);
  }
  flush();
  return blocks.join("") || "<p><br></p>";
}

/** 一行文字 → HTML：把 `$…$` 抠成公式块，其余转义 */
function inlineTextToHtml(line: string): string {
  let out = "";
  let rest = line;
  while (rest.length) {
    const start = rest.indexOf("$");
    if (start < 0) {
      out += escapeHtml(rest);
      break;
    }
    const end = rest.indexOf("$", start + 1);
    if (end < 0) {
      out += escapeHtml(rest);
      break;
    }
    out += escapeHtml(rest.slice(0, start));
    out += chipHtml(rest.slice(start + 1, end).trim(), false);
    rest = rest.slice(end + 1);
  }
  return out;
}

function chipHtml(latex: string, display: boolean): string {
  return `<span class="fchip${display ? " block" : ""}" contenteditable="false" data-latex="${escapeHtml(
    latex,
  )}" data-display="${display ? "1" : "0"}">${katexHtml(latex, display)}</span>`;
}

/** 当前选中的文字（文档视图或源码视图都支持） */
function selectionText(): string {
  const active = document.activeElement as HTMLTextAreaElement | null;
  if (active && active.tagName === "TEXTAREA") {
    const start = active.selectionStart ?? 0;
    const end = active.selectionEnd ?? 0;
    return active.value.slice(start, end);
  }
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed) return "";
  return selection.toString();
}

/** 把常见 Unicode 数学符号换成 LaTeX（AI 那条路是补充，这里先兜住大部分情况） */
function unicodeToLatex(text: string): string {
  const map: Record<string, string> = {
    "²": "^2",
    "³": "^3",
    "¹": "^1",
    "×": " \\times ",
    "÷": " \\div ",
    "±": " \\pm ",
    "≤": " \\le ",
    "≥": " \\ge ",
    "≠": " \\ne ",
    "≈": " \\approx ",
    "∞": " \\infty ",
    "√": "\\sqrt",
    "∑": "\\sum",
    "∏": "\\prod",
    "∫": "\\int",
    "α": "\\alpha ",
    "β": "\\beta ",
    "γ": "\\gamma ",
    "δ": "\\delta ",
    "ε": "\\varepsilon ",
    "θ": "\\theta ",
    "λ": "\\lambda ",
    "μ": "\\mu ",
    "π": "\\pi ",
    "σ": "\\sigma ",
    "φ": "\\varphi ",
    "ω": "\\omega ",
    "Δ": "\\Delta ",
    "Σ": "\\Sigma ",
    "Ω": "\\Omega ",
    "→": " \\to ",
    "·": " \\cdot ",
  };
  return Array.from(text)
    .map((ch) => map[ch] ?? ch)
    .join("");
}

/** 选中内容一键转公式：`useAi = true` 时让 AI 改写并给建议 */
const formulaOpen = ref(false);
const formulaBusy = ref(false);
const formulaNote = ref("");
const formulaLatex = ref("");
const formulaAdvice = ref("");
const formulaSource = ref("");
let savedRange: Range | null = null;

/** 预览用的纯公式体：去掉外层 `$…$` / `$$…$$`，KaTeX 不认这两个符号 */
const formulaPreview = computed(() => {
  const text = formulaLatex.value.trim();
  const display = text.match(/^\$\$([\s\S]+?)\$\$$/);
  if (display) return display[1].trim();
  const inline = text.match(/^\$([\s\S]+?)\$$/);
  if (inline) return inline[1].trim();
  return text;
});

function rememberRange() {
  const selection = window.getSelection();
  if (selection && selection.rangeCount > 0 && !selection.isCollapsed) {
    savedRange = selection.getRangeAt(0).cloneRange();
  }
}

function restoreRange(): boolean {
  if (!savedRange) return false;
  const selection = window.getSelection();
  if (!selection) return false;
  selection.removeAllRanges();
  selection.addRange(savedRange);
  return true;
}

async function convertSelection(useAi = false) {
  const text = selectionText().trim();
  if (!text) {
    status.value = "先选中要变成公式的内容（比如 x、A、x²+y²）";
    return;
  }
  rememberRange();
  if (!useAi) {
    // 本地转换：把选中的内容整体包成公式，顺手把 ² × √ 这类符号换掉
    const latex = unicodeToLatex(text).replace(/\s+/g, " ").trim();
    const block = text.includes("\n") || text.length > 40;
    const html = block
      ? chipHtml(latex, true)
      : chipHtml(latex, false);
    if (mode.value === "source") {
      const active = document.activeElement as HTMLTextAreaElement | null;
      if (active && active.tagName === "TEXTAREA") {
        const start = active.selectionStart ?? 0;
        const end = active.selectionEnd ?? 0;
        const wrapped = block ? `\n$$\n${latex}\n$$\n` : `$${latex}$`;
        source.value = `${active.value.slice(0, start)}${wrapped}${active.value.slice(end)}`;
        update(true);
        status.value = "已把选中的内容包成公式";
        return;
      }
    }
    insertHtml(html);
    status.value = "已把选中的内容包成公式（想改写法可以再选中后按 Ctrl+Shift+E 让 AI 处理）";
    return;
  }
  // AI 转换
  formulaOpen.value = true;
  formulaBusy.value = true;
  formulaNote.value = "正在让 AI 看这段文字…";
  formulaLatex.value = "";
  formulaAdvice.value = "";
  formulaSource.value = text;
  try {
    const result = await latexFormulaFromText(text, false);
    if (result.ok) {
      formulaLatex.value = result.latex;
      formulaAdvice.value = result.advice || "";
      formulaNote.value = "";
    } else {
      formulaNote.value = result.error || "AI 没给出结果";
    }
  } catch (err) {
    formulaNote.value = `调用失败：${String(err)}`;
  } finally {
    formulaBusy.value = false;
  }
}

/** 采纳 AI 的结果：换掉当初选中的那段 */
function applyFormula() {
  const text = formulaLatex.value.trim();
  if (!text) return;
  formulaOpen.value = false;
  if (mode.value === "source") {
    const active = document.activeElement as HTMLTextAreaElement | null;
    if (active && active.tagName === "TEXTAREA") {
      const start = active.selectionStart ?? 0;
      const end = active.selectionEnd ?? 0;
      source.value = `${active.value.slice(0, start)}${text}${active.value.slice(end)}`;
      update(true);
      status.value = "已应用 AI 的公式写法";
      return;
    }
  }
  restoreRange();
  insertHtml(textToDocHtml(text));
  status.value = "已应用 AI 的公式写法";
}

/** 把源码里「空环境 / 落单的 \end」收拾掉，再交给 xelatex。
 *
 * 为什么需要：文档视图和源码之间来回转一次，浏览器偶尔会把居中的内容挪出去，
 * 留下一个空 `\begin{center}\end{center}`——编译出来就是一条被压扁的空行
 * （用户 2026-11-11 圈出来的那块）。这里在编译前统一清掉，保证不出现这种碎行。
 */
function sanitizeLatex(text: string): string {
  let out = text || "";
  // 标题里的 `\textbf{…}` 去掉（标题本身就是粗体；留着预览里会露出裸语法）
  out = out.replace(
    /^(\\(?:sub)*section\*?\{)(.*)(\})[ \t]*$/gm,
    (_all, head: string, body: string, tail: string) => `${head}${unwrapBold(body)}${tail}`,
  );
  // 空 center / itemize / enumerate
  out = out.replace(/\\begin\{center\}\s*\\end\{center\}/g, "");
  out = out.replace(/\\begin\{(itemize|enumerate)\}\s*\\end\{\1\}/g, "");
  // 落单的 \end{...}（配对不上的）
  for (const env of ["center", "itemize", "enumerate"]) {
    const begins = out.split(`\\begin{${env}}`).length - 1;
    let ends = out.split(`\\end{${env}}`).length - 1;
    while (ends > begins) {
      const at = out.lastIndexOf(`\\end{${env}}`);
      out = out.slice(0, at) + out.slice(at + `\\end{${env}}`.length);
      ends -= 1;
    }
  }
  // 三行以上空行压成一行
  out = out.replace(/\n{3,}/g, "\n\n");
  return out.replace(/[ \t]+\n/g, "\n").trim() + "\n";
}

async function exportPdf() {
  if (exporting.value) return;
  exporting.value = true;
  status.value = "正在用 XeLaTeX 编译 PDF…";
  try {
    const result = await latexPdf(sanitizeLatex(source.value));
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

/**
 * 编辑即编译（旧版 `latex_auto` + `_auto_timer` 400ms 防抖）：
 * 停手 400 毫秒就编一次，编译期间又有改动就等这次编完再补一次。
 * 编译在 Rust 的 spawn_blocking 里跑（界面不卡），xelatex 用 CREATE_NO_WINDOW 起（不弹黑窗口）。
 */
function scheduleCompile() {
  if (!autoCompile.value) return;
  window.clearTimeout(compileTimer);
  compileTimer = window.setTimeout(() => {
    void compileQuietly();
  }, 400);
}

async function compileQuietly() {
  if (!source.value.trim()) return;
  if (exporting.value) {
    compileAgain = true;
    return;
  }
  exporting.value = true;
  status.value = "正在编译…";
  try {
    const result = await latexPdf(sanitizeLatex(source.value));
    if (result.ok) {
      pdfPath.value = result.path;
      const file = await readFileBase64(result.path);
      pdfData.value = file.data;
      status.value = `已编译 · ${new Date().toLocaleTimeString()}`;
    } else {
      status.value = result.error;
    }
  } catch (err) {
    status.value = String(err);
  } finally {
    exporting.value = false;
    if (compileAgain) {
      compileAgain = false;
      scheduleCompile();
    }
  }
}

async function toggleAutoCompile() {
  autoCompile.value = !autoCompile.value;
  await settingsSave({ latex_auto_compile: autoCompile.value ? "1" : "0" });
  if (autoCompile.value) scheduleCompile();
}

onMounted(async () => {
  await restore();
  update();
  await nextTick();
  paintDoc();
  scheduleCompile();
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
      <button
        class="auto-toggle"
        :class="{ on: autoCompile }"
        :title="autoCompile ? '编辑即编译：改动停手 0.4 秒就编一次 PDF（点击关闭）' : '编辑即编译已关闭（点击打开）'"
        @click="toggleAutoCompile"
      >
        {{ autoCompile ? "编辑即编译 · 开" : "编辑即编译 · 关" }}
      </button>
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
        <button class="tool" title="填格子生成矩阵 / 行列式" @click="openMatrix">矩阵…</button>
        <span class="gap" />
        <!-- 选中文字一键变公式（Ctrl+Shift+M）／交给 AI 转（Ctrl+Shift+E） -->
        <button class="tool wide" title="把选中的内容变成行内公式（Ctrl+Shift+M）" @click="convertSelection()">
          转公式
        </button>
        <button
          class="tool wide"
          title="选中一段文字，让 AI 把里面的公式转成 LaTeX 并给建议（Ctrl+Shift+E）"
          @click="convertSelection(true)"
        >
          AI 转公式
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
          @keydown="onEditorKey"
          @click="onDocClick"
          @copy="onDocCopy($event, false)"
          @cut="onDocCopy($event, true)"
          @paste="onDocPaste"
        />
        <textarea
          v-show="mode === 'source'"
          v-model="source"
          class="source"
          spellcheck="false"
          @input="update()"
          @keydown="onEditorKey"
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

    <!-- AI 转公式：给改写结果 + 建议，一键应用 -->
    <div v-if="formulaOpen" class="matrix-modal">
      <div class="matrix-card formula-card">
        <div class="matrix-head">
          <b>AI 转公式</b>
          <span class="spacer" />
          <button class="tool" @click="formulaOpen = false">关闭</button>
        </div>
        <div class="formula-body">
          <div class="formula-block">
            <span class="formula-label">原文</span>
            <div class="formula-text">{{ formulaSource }}</div>
          </div>
          <div v-if="formulaBusy" class="hint">正在让 AI 看这段文字…</div>
          <div v-else-if="formulaNote" class="hint">{{ formulaNote }}</div>
          <template v-else>
            <div class="formula-block">
              <span class="formula-label">改成</span>
              <div class="formula-text mono">{{ formulaLatex }}</div>
            </div>
            <div v-if="formulaPreview" class="formula-preview" v-html="katexHtml(formulaPreview, false)" />
            <div v-if="formulaAdvice" class="hint">建议：{{ formulaAdvice }}</div>
          </template>
        </div>
        <div class="formula-foot">
          <span class="tiny">替换掉你刚才选中的那段；不满意可以关掉自己改</span>
          <span class="spacer" />
          <button class="tool" @click="formulaOpen = false">算了</button>
          <button class="tool primary" :disabled="formulaBusy || !formulaLatex" @click="applyFormula">
            一键应用
          </button>
        </div>
      </div>
    </div>

    <!-- 矩阵 / 行列式：填格子生成 LaTeX（旧版 latex_matrix.py） -->
    <div v-if="matrixOpen" class="matrix-modal">
      <div class="matrix-card">
        <div class="matrix-head">
          <b>矩阵 / 行列式</b>
          <span class="spacer" />
          <button class="tool" @click="matrixOpen = false">关闭</button>
        </div>
        <div class="matrix-tools">
          <label>
            行
            <select :value="matrixRows" @change="resizeMatrix(Number(($event.target as HTMLSelectElement).value), matrixCols)">
              <option v-for="n in 8" :key="`r${n}`" :value="n">{{ n }}</option>
            </select>
          </label>
          <label>
            列
            <select :value="matrixCols" @change="resizeMatrix(matrixRows, Number(($event.target as HTMLSelectElement).value))">
              <option v-for="n in 8" :key="`c${n}`" :value="n">{{ n }}</option>
            </select>
          </label>
          <label>
            外形
            <select v-model="matrixEnv">
              <option value="pmatrix">圆括号（矩阵）</option>
              <option value="bmatrix">方括号</option>
              <option value="vmatrix">竖线（行列式）</option>
            </select>
          </label>
        </div>
        <div class="matrix-grid" :style="{ gridTemplateColumns: `repeat(${matrixCols}, minmax(0, 1fr))` }">
          <input
            v-for="at in matrixRows * matrixCols"
            :key="`cell-${at}`"
            :value="matrixCells[Math.floor((at - 1) / matrixCols)]?.[(at - 1) % matrixCols] || ''"
            :placeholder="(at - 1) % matrixCols === 0 ? `${Math.floor((at - 1) / matrixCols) + 1}` : ''"
            @input="
              matrixCells[Math.floor((at - 1) / matrixCols)][(at - 1) % matrixCols] = (
                $event.target as HTMLInputElement
              ).value
            "
          />
        </div>
        <div class="matrix-foot">
          <code class="matrix-preview">{{ buildMatrix() }}</code>
          <span class="spacer" />
          <button class="tool" @click="useMatrixSource">放进输入框</button>
          <button class="tool primary" @click="insertMatrix">插到文档</button>
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
/* 「编辑即编译」开关：像小胶囊，开的时候用强调色 */
.auto-toggle {
  height: 26px;
  padding: 0 10px;
  border-radius: 999px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  color: var(--muted);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
  white-space: nowrap;
}
.auto-toggle.on {
  color: var(--accent);
  border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  background: color-mix(in srgb, var(--accent) 8%, var(--card));
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
  cursor: text;
  /* 以前是 user-select: none —— 光标根本选不中公式（用户 2026-11-11 报的就是这个）。
     `all` 让点一下就把整块公式选中，剪切/复制/删除都按一整块处理。 */
  user-select: all;
}
.doc :deep(.fchip.block) {
  display: block;
  padding: 6px 10px;
  margin: 6px 0;
  text-align: center;
}
/* 选中公式时给个明显的底色，不然看不出到底选中了没 */
.doc :deep(.fchip)::selection,
.doc :deep(.fchip) *::selection {
  background: color-mix(in srgb, var(--accent) 38%, transparent);
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

/* 矩阵输入弹层（旧版 latex_matrix.py 的图形化矩阵） */
/* 矩阵弹层：不浮在整个窗口上（WebView 里 fixed 会被外壳盖住），直接排在页面里 */
.matrix-modal {
  display: flex;
  justify-content: center;
  margin: 8px 0;
}
.matrix-card {
  width: min(560px, 92vw);
  max-height: 86vh;
  overflow: auto;
  background: var(--card);
  border-radius: 14px;
  border: 1px solid var(--border);
  box-shadow: 0 18px 48px rgba(0, 0, 0, 0.18);
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.matrix-head,
.matrix-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
/* AI 转公式的弹窗 */
.formula-card {
  width: min(620px, 92vw);
}
.formula-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.formula-block {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.formula-label {
  font-size: 11px;
  color: var(--muted);
}
.formula-text {
  font-size: 12.5px;
  line-height: 1.7;
  color: var(--text);
  background: var(--card-alt);
  border-radius: 8px;
  padding: 7px 9px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  max-height: 160px;
  overflow: auto;
  user-select: text;
}
.formula-text.mono {
  font-family: var(--font-mono);
  font-size: 12px;
}
.formula-preview {
  padding: 8px 10px;
  border-radius: 8px;
  border: 1px dashed var(--border);
  background: var(--card);
  overflow-x: auto;
}
.formula-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.matrix-tools {
  display: flex;
  gap: 12px;
  font-size: 12px;
  color: var(--muted);
  align-items: center;
}
.matrix-tools select {
  margin-left: 4px;
  font-size: 12px;
  padding: 3px 6px;
}
.matrix-grid {
  display: grid;
  gap: 4px;
}
.matrix-grid input {
  font-size: 12px;
  padding: 5px 6px;
  text-align: center;
}
.matrix-preview {
  flex: 1;
  min-width: 0;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.matrix-foot .tool.primary {
  border-color: var(--accent);
  color: var(--accent);
}
