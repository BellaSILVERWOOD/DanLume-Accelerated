// AI 附件：把本地文件读成给模型看的文本 / 图片（对应旧版 `_lumina/lumina/ai_files.py`）。
//
// 分工：
// * 纯文本 / Word / PPT / Excel → Rust 的 `read_text_file`（`docs::preview_text` 抽正文）；
// * **PDF** → 前端用 pdf.js 逐页抽文字（管线里已经有 pdf.js，不引新依赖）；
// * 图片 → base64 data URL，按 OpenAI 兼容的 `image_url` 发给多模态模型。
//
// 上限沿用 Python 版：单文件 12 万字、一次 60 个、合计 40 万字、最多 6 张图、单图 4 MB。
import * as pdfjs from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { aiExpandPaths, readFileBase64, readTextFile, type AiFileCandidate } from "../api";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

export const MAX_CHARS = 120000;
export const MAX_FILES = 60;
export const MAX_TOTAL_CHARS = 400000;
export const MAX_IMAGES = 6;
export const MAX_IMAGE_BYTES = 4 * 1024 * 1024;
export const PDF_MAX_PAGES = 60;

export type Attachment = {
  path: string;
  name: string;
  kind: "text" | "office" | "pdf" | "image";
  /** 文字内容（图片为空） */
  text: string;
  /** 图片的 data URL（文字为空） */
  dataUrl: string;
  chars: number;
  size: number;
};

function decodeBase64(base64: string): Uint8Array {
  const raw = atob(base64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i += 1) out[i] = raw.charCodeAt(i);
  return out;
}

function imageMime(name: string): string {
  const ext = name.toLowerCase().split(".").pop() || "";
  if (ext === "jpg" || ext === "jpeg") return "image/jpeg";
  if (ext === "webp") return "image/webp";
  if (ext === "gif") return "image/gif";
  if (ext === "bmp") return "image/bmp";
  return "image/png";
}

/** PDF 逐页抽文字（扫描件抽不到就如实说明） */
async function pdfText(path: string): Promise<string> {
  const file = await readFileBase64(path, 96 * 1024 * 1024);
  const task = pdfjs.getDocument({ data: decodeBase64(file.data) });
  const doc = await task.promise;
  const chunks: string[] = [];
  let used = 0;
  try {
    const pages = Math.min(doc.numPages, PDF_MAX_PAGES);
    for (let index = 1; index <= pages; index += 1) {
      const page = await doc.getPage(index);
      const content = await page.getTextContent();
      const text = content.items
        .map((item) => ("str" in item ? item.str : ""))
        .join(" ")
        .replace(/[ \t]+/g, " ")
        .trim();
      if (!text) continue;
      chunks.push(`—— 第 ${index} 页 ——\n${text}`);
      used += text.length;
      if (used >= MAX_CHARS) break;
    }
    if (!chunks.length) {
      throw new Error("这个 PDF 里抽不到文字（大概是扫描件/图片版），可以截图当图片发");
    }
    let out = chunks.join("\n\n");
    if (doc.numPages > pages) out += `\n…（共 ${doc.numPages} 页，只读了前 ${pages} 页）`;
    return out;
  } finally {
    try {
      await doc.destroy();
    } catch {
      /* 忽略 */
    }
  }
}

async function loadOne(file: AiFileCandidate): Promise<Attachment> {
  const base = { path: file.path, name: file.name, size: file.size };
  if (file.kind === "image") {
    if (file.size > MAX_IMAGE_BYTES) {
      throw new Error(
        `图片 ${(file.size / 1024 / 1024).toFixed(1)}MB，超过 ${MAX_IMAGE_BYTES / 1024 / 1024}MB，先压一下再发`,
      );
    }
    const data = await readFileBase64(file.path, MAX_IMAGE_BYTES + 1024);
    return { ...base, kind: "image", text: "", dataUrl: `data:${imageMime(file.name)};base64,${data.data}`, chars: 0 };
  }
  let text = "";
  if (file.kind === "pdf") {
    text = await pdfText(file.path);
  } else {
    text = await readTextFile(file.path, MAX_CHARS + 1000);
  }
  text = (text || "").trim();
  if (!text) throw new Error("文件里没有可读的文字（可能是扫描件/图片）");
  if (text.length > MAX_CHARS) text = `${text.slice(0, MAX_CHARS)}\n…（已截断，原文件更长）`;
  const kind: Attachment["kind"] = file.kind === "office" ? "office" : file.kind === "pdf" ? "pdf" : "text";
  return { ...base, kind, text, dataUrl: "", chars: text.length };
}

/**
 * 把路径（文件 / 文件夹 / 混在一起）读成附件。
 * 返回 (成功的附件, 提示信息)；提示信息直接显示在状态行。
 */
export async function loadAttachments(
  paths: string[],
  existing: Attachment[] = [],
): Promise<{ files: Attachment[]; notes: string[] }> {
  const expanded = await aiExpandPaths(paths);
  const notes = [...(expanded.notes || [])];
  const files: Attachment[] = [];
  const known = new Set(existing.map((one) => one.path.toLowerCase()));
  let images = existing.filter((one) => one.kind === "image").length;
  let total = existing.reduce((sum, one) => sum + one.chars, 0);
  let count = existing.length;

  for (const candidate of expanded.files || []) {
    if (known.has(candidate.path.toLowerCase())) continue;
    if (!candidate.kind) continue;
    if (count >= MAX_FILES) {
      notes.push(`一次最多带 ${MAX_FILES} 个文件，后面的没加进来`);
      break;
    }
    if (candidate.kind === "image" && images >= MAX_IMAGES) {
      notes.push(`一次最多带 ${MAX_IMAGES} 张图片，后面的没加进来`);
      break;
    }
    try {
      const one = await loadOne(candidate);
      if (one.kind !== "image" && total > 0 && total + one.chars > MAX_TOTAL_CHARS) {
        notes.push(`附件合计到 ${MAX_TOTAL_CHARS} 字上限，后面的没加进来`);
        break;
      }
      total += one.chars;
      count += 1;
      if (one.kind === "image") images += 1;
      known.add(candidate.path.toLowerCase());
      files.push(one);
    } catch (err) {
      notes.push(`${candidate.name}：${err instanceof Error ? err.message : String(err)}`);
    }
  }
  return { files, notes };
}

/** 文字附件拼成给模型看的上下文块（图片不进这里） */
export function asContext(files: Attachment[]): string {
  const blocks = files
    .filter((one) => one.kind !== "image" && one.text.trim())
    .map((one) => `【附件：${one.name}】\n\`\`\`\n${one.text}\n\`\`\``);
  if (!blocks.length) return "";
  let text = blocks.join("\n\n");
  if (text.length > MAX_TOTAL_CHARS) {
    text = `${text.slice(0, MAX_TOTAL_CHARS)}\n…（附件合计超出上限，后面的内容已截断）`;
  }
  return text;
}

/** 图片附件 → data URL 列表（多模态 content 用） */
export function imageParts(files: Attachment[]): string[] {
  return files.filter((one) => one.kind === "image" && one.dataUrl.startsWith("data:")).map((one) => one.dataUrl);
}

/** 状态行文案：「已附加 2 个：a.pdf、b.docx（共 3.2 万字）」 */
export function describeAttachments(files: Attachment[]): string {
  if (!files.length) return "";
  const names = files.slice(0, 3).map((one) => one.name);
  let text = files.length > 3 ? `${names.join("、")} 等 ${files.length} 个` : names.join("、");
  const chars = files.reduce((sum, one) => sum + one.chars, 0);
  const images = files.filter((one) => one.kind === "image").length;
  const parts = [`已附加 ${files.length} 个：${text}`];
  if (chars) parts.push(`文字 ${chars.toLocaleString()} 字`);
  if (images) parts.push(`${images} 张图片`);
  return parts.join("　");
}
