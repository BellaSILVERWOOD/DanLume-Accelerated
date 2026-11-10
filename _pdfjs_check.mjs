// 用 pdf.js 在 Node 里读这份 PDF，看中文到底能不能出来（验证 cMap 是否必要）
import * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const pdfPath = "C:/Users/Lena/Downloads/8-linear transformation.pdf";
const cmapDir = join(here, "node_modules/pdfjs-dist/cmaps/");
const stdDir = join(here, "node_modules/pdfjs-dist/standard_fonts/");

const data = new Uint8Array(readFileSync(pdfPath));

async function run(label, options) {
  try {
    const doc = await pdfjs.getDocument({ data: data.slice(), ...options }).promise;
    const page = await doc.getPage(2);
    const content = await page.getTextContent();
    const text = content.items.map((one) => one.str).join("");
    const chinese = (text.match(/[\u4e00-\u9fa5]/g) || []).length;
    console.log(`--- ${label}: 页数=${doc.numPages} 字符=${text.length} 中文字符=${chinese}`);
    console.log("    片段:", JSON.stringify(text.slice(0, 120)));
    const fonts = [];
    const ops = await page.getOperatorList();
    for (const item of content.items.slice(0, 6)) {
      fonts.push(item.fontName);
    }
    console.log("    用到的字体:", [...new Set(fonts)].join(", "));
    printFonts(doc, page, ops);
  } catch (err) {
    console.log(`--- ${label}: 失败 ${err && err.message}`);
  }
}

async function printFonts(doc) {
  try {
    const names = Object.keys(doc.fontLoader ? {} : {});
    void names;
  } catch {
    /* 忽略 */
  }
}

const common = { useSystemFonts: false, verbosity: 0 };
await run("不加 cMap（现在的写法）", common);
await run("加 cMapUrl + standardFontDataUrl", {
  ...common,
  cMapUrl: cmapDir,
  cMapPacked: true,
  standardFontDataUrl: stdDir,
});
