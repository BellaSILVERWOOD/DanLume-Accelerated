// 极简 Markdown → HTML：代码块 / 行内代码 / 粗体 / 标题 / 列表 / 表格 / 公式（KaTeX）
import katex from "katex";
import "katex/dist/katex.min.css";

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

function renderMath(body: string, display: boolean): string {
  try {
    return katex.renderToString(body.trim(), {
      displayMode: display,
      throwOnError: false,
      strict: false,
    });
  } catch {
    return `<code>${escapeHtml(body)}</code>`;
  }
}

/** 行内：公式、行内代码、粗体、斜体 */
function inline(text: string): string {
  let out = text;
  // 先把代码段摘出来，避免里面的 $ 被当公式
  const codes: string[] = [];
  out = out.replace(/`([^`]+)`/g, (_m, body) => {
    codes.push(`<code class="md-code">${escapeHtml(String(body))}</code>`);
    return `\u0000${codes.length - 1}\u0000`;
  });
  out = out.replace(/\$([^$\n]+?)\$/g, (_m, body) => renderMath(String(body), false));
  out = out.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  out = out.replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<em>$2</em>");
  out = out.replace(/\u0000(\d+)\u0000/g, (_m, index) => codes[Number(index)] || "");
  return out;
}

/** 把一个表格行切成单元格（去掉首尾竖线，单元格去空白） */
function splitRow(line: string): string[] {
  let body = String(line ?? "").trim();
  if (body.startsWith("|")) body = body.slice(1);
  if (body.endsWith("|")) body = body.slice(0, -1);
  return body.split("|").map((cell) => cell.trim());
}

/** 对齐行里的 `---` / `:---` / `---:` / `:---:` */
function isAlignCell(cell: string): boolean {
  return /^:?-+:?$/.test(cell.replace(/\s+/g, ""));
}

function alignOf(cell: string): string {
  const text = cell.replace(/\s+/g, "");
  if (text.startsWith(":") && text.endsWith(":")) return "center";
  if (text.endsWith(":")) return "right";
  return "left";
}

/** 表格 → HTML。默认左对齐（用户要求表格内容左对齐），`:---:` / `---:` 才覆盖 */
function tableHtml(header: string[], aligns: string[], rows: string[][]): string {
  const head = header
    .map((cell, column) => `<th class="md-${aligns[column] || "left"}">${inline(cell)}</th>`)
    .join("");
  const body = rows
    .map(
      (row) =>
        `<tr>${header
          .map(
            (_cell, column) =>
              `<td class="md-${aligns[column] || "left"}">${inline(row[column] ?? "")}</td>`,
          )
          .join("")}</tr>`,
    )
    .join("");
  return (
    `<div class="md-table"><table>` +
    `<thead><tr>${head}</tr></thead><tbody>${body}</tbody></table></div>`
  );
}

/**
 * 把一串单元格按列数切成若干行。
 * 空单元格当「行分隔」处理：模型图省事时常写成 `| a | b | | c | d |`（行与行之间多一根竖线），
 * 2026-11-15 那张日程表就是这种写法。
 */
function chunkCells(cells: string[], columns: number): string[][] {
  const rows: string[][] = [];
  let current: string[] = [];
  const flush = () => {
    for (let i = 0; i < current.length; i += columns) {
      rows.push(Array.from({ length: columns }, (_v, k) => current[i + k] ?? ""));
    }
    current = [];
  };
  for (const cell of cells) {
    if (cell) current.push(cell);
    else flush();
  }
  flush();
  return rows;
}

/**
 * 认领一张表格，返回 HTML 和下一行下标；不是表格就返回 null。
 *
 * 兼容两种写法：
 *  1. 标准 GFM 多行表格（表头一行、`| --- |` 对齐行一行、数据每行一条）；
 *  2. 模型图省事把**整张表压成一行**的写法 —— `| 时间 | 安排 | |---|---| | 08:55 | 微积分 |`
 *     （2026-11-15 用户截图里就是这种，以前只能显示成一串竖线）。
 */
function takeTable(lines: string[], start: number): { html: string; next: number } | null {
  const first = lines[start] ?? "";
  if (!first.includes("|")) return null;
  const cells = splitRow(first);
  if (cells.length < 2) return null;

  // ① 标准写法：下一行整行都是 `---` 这种对齐格
  if (start + 1 < lines.length && lines[start + 1].includes("|")) {
    const aligns = splitRow(lines[start + 1]);
    if (aligns.length >= 1 && aligns.every(isAlignCell)) {
      const columns = aligns.length;
      const header = Array.from({ length: columns }, (_v, i) => cells[i] ?? "");
      const rows: string[][] = [];
      let index = start + 2;
      while (index < lines.length) {
        const line = lines[index];
        if (!line.trim() || !line.includes("|")) break;
        const row = splitRow(line);
        if (row.every(isAlignCell)) break;
        rows.push(...chunkCells(row, columns));
        index += 1;
      }
      return { html: tableHtml(header, aligns.map(alignOf), rows), next: index };
    }
  }

  // ② 压成一行的表：找到连续的对齐格 —— 前面是表头，后面按列数切成数据行
  const firstAlign = cells.findIndex(isAlignCell);
  if (firstAlign > 0) {
    let columns = 0;
    while (firstAlign + columns < cells.length && isAlignCell(cells[firstAlign + columns])) {
      columns += 1;
    }
    // 表头末尾可能被写过一根多余的竖线（`| 备注 | |---|---|---|`），跳过这些空格子
    let headerEnd = firstAlign;
    while (headerEnd > 0 && !cells[headerEnd - 1]) headerEnd -= 1;
    const rest = cells.slice(firstAlign + columns);
    if (columns >= 1 && rest.some((cell) => cell)) {
      const headerStart = Math.max(0, headerEnd - columns);
      const header = Array.from(
        { length: columns },
        (_v, i) => cells[headerStart + i] ?? "",
      );
      const aligns = cells.slice(firstAlign, firstAlign + columns).map(alignOf);
      return {
        html: tableHtml(header, aligns, chunkCells(rest, columns)),
        next: start + 1,
      };
    }
  }
  return null;
}

/** 整段 Markdown → HTML（够聊天用） */
export function renderMarkdown(source: string): string {
  const lines = String(source ?? "").replace(/\r\n/g, "\n").split("\n");
  const out: string[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];

    // 围栏代码块
    const fence = line.match(/^\s*```(\w*)\s*$/);
    if (fence) {
      const lang = fence[1] || "";
      const body: string[] = [];
      index += 1;
      while (index < lines.length && !/^\s*```\s*$/.test(lines[index])) {
        body.push(lines[index]);
        index += 1;
      }
      index += 1;
      out.push(
        `<div class="md-pre"><div class="md-pre-head">${escapeHtml(lang || "代码")}</div>` +
          `<pre>${escapeHtml(body.join("\n"))}</pre></div>`,
      );
      continue;
    }

    // 行间公式 $$…$$（同一行）
    if (/^\s*\$\$(.+?)\$\$\s*$/.test(line)) {
      const body = line.trim().replace(/^\$\$/, "").replace(/\$\$$/, "");
      out.push(`<div class="md-display">${renderMath(body, true)}</div>`);
      index += 1;
      continue;
    }

    // 标题
    const heading = line.match(/^(#{1,4})\s+(.*)$/);
    if (heading) {
      const level = Math.min(6, heading[1].length + 2);
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      index += 1;
      continue;
    }

    // 表格（要多行 / 一行压成一张都认）
    if (line.includes("|")) {
      const table = takeTable(lines, index);
      if (table) {
        out.push(table.html);
        index = table.next;
        continue;
      }
    }

    // 无序 / 有序列表
    if (/^\s*([-*+]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\.\s+/.test(line);
      const items: string[] = [];
      while (index < lines.length && /^\s*([-*+]|\d+\.)\s+/.test(lines[index])) {
        items.push(lines[index].replace(/^\s*([-*+]|\d+\.)\s+/, ""));
        index += 1;
      }
      const tag = ordered ? "ol" : "ul";
      out.push(`<${tag}>${items.map((one) => `<li>${inline(one)}</li>`).join("")}</${tag}>`);
      continue;
    }

    if (!line.trim()) {
      index += 1;
      continue;
    }

    // 普通段落（连续行合并）
    const paragraph: string[] = [line];
    index += 1;
    while (
      index < lines.length &&
      lines[index].trim() &&
      !/^\s*(```|#{1,4}\s|([-*+]|\d+\.)\s|\$\$.+?\$\$|\|)/.test(lines[index])
    ) {
      paragraph.push(lines[index]);
      index += 1;
    }
    out.push(`<p>${inline(paragraph.join(" "))}</p>`);
  }
  return out.join("");
}
