// 极简 Markdown → HTML：代码块 / 行内代码 / 粗体 / 标题 / 列表 / 公式（KaTeX）
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
      !/^\s*(```|#{1,4}\s|([-*+]|\d+\.)\s|\$\$.+?\$\$)/.test(lines[index])
    ) {
      paragraph.push(lines[index]);
      index += 1;
    }
    out.push(`<p>${inline(paragraph.join(" "))}</p>`);
  }
  return out.join("");
}
