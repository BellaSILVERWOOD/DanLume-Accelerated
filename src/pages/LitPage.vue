<script setup lang="ts">
// 文献库：读旧库 papers 表，项目 / 星级 / 搜索筛选，右侧详情 + 引用
// 另外接上了「方法流程图」（对齐旧版 lit_flow.py）：让模型把论文拆成步骤，
// 前端用 CSS + SVG 自己画节点图（不引第三方图库），也能降级成纯文本步骤列表。
import { computed, onMounted, ref, watch } from "vue";
import SoftButton from "../components/SoftButton.vue";
import Pill from "../components/Pill.vue";
import IconBadge from "../components/IconBadge.vue";
import EmbedWebview from "../components/EmbedWebview.vue";
import {
  aiState,
  endnoteDetect,
  endnoteOpen,
  libraryAccess,
  libraryAccessUrl,
  librarySetAccess,
  litFlowDelete,
  litFlowGenerate,
  litFlowLoad,
  openPath,
  paperCitation,
  paperUpdate,
  papersDatabases,
  papersDownload,
  papersLinks,
  papersList,
  papersSemanticIndex,
  papersSemanticSearch,
  papersSemanticStatus,
  pickFiles,
  papersAdd,
  papersSearch,
  refsAddDoi,
  refsDelete,
  refsExport,
  refsImport,
  refsList,
  revealPath,
  type LibraryAccess,
  type LibraryDatabase,
  type LitFlow,
  type Paper,
  type PaperLinks,
  type PaperSearchHitWithCoverage,
  type RefEntry,
} from "../api";

// ---- 在线检索（arXiv + Crossref），旧版 lit.search_all —— Rust 以前没有
//
// 两个用户反馈：
// * 「（搜 MRI）怎么可能只有 20 条啊」→ 以前固定 10 条/来源、没有翻页；现在每页 20 条/来源 + 「再看 20 条」+ 总量；
// * 「检索结果和 crossfield 疑似没有任何关系」→ 多个关键词现在**按 AND 搜**，结果里每个词都必须出现
//   （arXiv 检索式用 `all:"A" AND all:"B"`，Crossref 的结果再过滤一遍）。
const PAGE = 20;
const searchOpen = ref(false);
const searchText = ref("");
const searchHits = ref<PaperSearchHitWithCoverage[]>([]);
const searchBusy = ref(false);
const searchMoreBusy = ref(false);
const searchMsg = ref("");
const searchOffset = ref(0);
const searchMore = ref(false);
const searchTotals = ref<{ arxiv?: number; crossref?: number; semantic?: number }>({});
const searchTerms = ref<string[]>([]);
const searchRelaxed = ref(false);

const searchTotalText = computed(() => {
  const crossref = searchTotals.value.crossref || 0;
  const arxiv = searchTotals.value.arxiv || 0;
  const semantic = searchTotals.value.semantic || 0;
  const parts: string[] = [];
  // 多关键词时 Crossref 报的「总量」是**未过滤**的模糊命中数（几十万条跟关键词只有一半关系），
  // 摆出来只会误导，所以只在单词检索时显示它。
  if (arxiv > 0) parts.push(`arXiv 约 ${arxiv.toLocaleString()} 条`);
  if (crossref > 0 && searchTerms.value.length <= 1) {
    parts.push(`Crossref 约 ${crossref.toLocaleString()} 条`);
  }
  if (semantic > 0 && searchTerms.value.length <= 1) {
    parts.push(`Semantic Scholar 约 ${semantic.toLocaleString()} 条`);
  }
  return parts.join(" · ");
});

async function searchOnline() {
  const query = searchText.value.trim();
  if (!query) {
    searchMsg.value = "先写个关键词（比如 transformer reasoning）";
    return;
  }
  searchBusy.value = true;
  searchHits.value = [];
  searchOffset.value = 0;
  searchMore.value = false;
  searchTotals.value = {};
  searchTerms.value = [];
  searchRelaxed.value = false;
  searchMsg.value = "正在搜 arXiv / Crossref / Semantic Scholar…";
  try {
    const result = await papersSearch(query, PAGE, 0);
    searchHits.value = result.results || [];
    searchTotals.value = result.totals || {};
    searchTerms.value = result.terms || [];
    searchRelaxed.value = !!result.relaxed;
    searchMore.value = !!result.more;
    searchOffset.value = PAGE;
    searchMsg.value = result.ok
      ? describeSearch(result)
      : (result.errors || []).join("；") || "检索失败";
  } catch (err) {
    searchMsg.value = `检索失败：${String(err)}`;
  } finally {
    searchBusy.value = false;
  }
}

function describeSearch(result: {
  counts?: { arxiv: number; crossref: number; semantic?: number };
  totals?: { arxiv?: number; crossref?: number; semantic?: number };
  errors?: string[];
}): string {
  const counts = result.counts || { arxiv: 0, crossref: 0, semantic: 0 };
  const terms = searchTerms.value;
  const errors = (result.errors || []).length ? `（${(result.errors || []).join("；")}）` : "";
  if (!searchHits.value.length) {
    const why = terms.length > 1
      ? `没有同时包含「${terms.join("」「")}」的文献——可以少写一个关键词，或者换个更常见的说法`
      : "没搜到文献";
    return `${why}${errors}`;
  }
  if (searchRelaxed.value) {
    // 严格匹配为空、自动放宽：说清楚，并且每条会标「命中 2/3 个词」
    return `没有同时包含「${terms.join("」「")}」的文献；下面是含其中一部分的（按命中词数排序，每条标了「命中 N/${terms.length} 个词」）${errors}`;
  }
  const rule = terms.length > 1 ? `（要求「${terms.join("」「")}」都出现）` : "";
  const semantic = counts.semantic || 0;
  const head = `这一页 ${searchHits.value.length} 条（arXiv ${counts.arxiv} + Crossref ${counts.crossref}` +
    `${semantic ? ` + Semantic Scholar ${semantic}` : ""}，已去重）${rule}`;
  const totals = searchTotalText.value ? `；库里的总量：${searchTotalText.value}` : "";
  const more = searchMore.value ? "；点「再看 20 条」继续翻" : "";
  return `${head}${totals}${more}${errors}`;
}

/** 再看下一页（每个来源都往后翻一页，结果追加在后面） */
async function searchMoreResults() {
  const query = searchText.value.trim();
  if (!query || searchMoreBusy.value) return;
  searchMoreBusy.value = true;
  searchMsg.value = "正在读下一页…";
  try {
    const result = await papersSearch(query, PAGE, searchOffset.value);
    const known = new Set(
      searchHits.value.map((one) => (one.doi || one.title || "").toLowerCase()),
    );
    const fresh = (result.results || []).filter(
      (one) => !known.has((one.doi || one.title || "").toLowerCase()),
    );
    searchHits.value = [...searchHits.value, ...fresh];
    searchTotals.value = result.totals || searchTotals.value;
    searchMore.value = !!result.more && fresh.length > 0;
    searchOffset.value += PAGE;
    searchMsg.value = `又加了 ${fresh.length} 条，现在共 ${searchHits.value.length} 条` +
      (searchTotalText.value ? `；库里总量：${searchTotalText.value}` : "");
  } catch (err) {
    searchMsg.value = `读取下一页失败：${String(err)}`;
  } finally {
    searchMoreBusy.value = false;
  }
}

async function addHit(hit: PaperSearchHitWithCoverage) {
  const result = await papersAdd(hit, project.value || "");
  searchMsg.value = result.ok ? `已加入文献库：${hit.title.slice(0, 40)}` : `加入失败：${result.error}`;
  if (result.ok) await load();
}

// ---- 参考文献层（旧版 refs.py）
const refsOpen = ref(false);
const refsItems = ref<RefEntry[]>([]);
const refsBusy = ref(false);
const refsMsg = ref("");
const doiInput = ref("");

async function loadRefs() {
  const result = await refsList(300);
  refsItems.value = result.items || [];
}

async function openRefs() {
  refsOpen.value = !refsOpen.value;
  if (refsOpen.value) await loadRefs();
}

async function importRefs() {
  const picked = await pickFiles(
    [{ name: "BibTeX / RIS", extensions: ["bib", "bibtex", "ris", "txt"] }],
    false,
  );
  if (!picked.length) return;
  refsBusy.value = true;
  refsMsg.value = "正在导入…";
  try {
    const result = await refsImport({ path: picked[0] });
    refsMsg.value = result.ok
      ? `导入完成：解析 ${result.parsed} 条，新增 ${result.added} 条（重复 ${result.skipped} 条）`
      : `导入失败：${result.error}`;
    await loadRefs();
  } finally {
    refsBusy.value = false;
  }
}

async function addDoi() {
  const doi = doiInput.value.trim();
  if (!doi) {
    refsMsg.value = "先粘一个 DOI（如 10.1145/3531146）";
    return;
  }
  refsBusy.value = true;
  refsMsg.value = "正在问 Crossref…";
  try {
    const result = await refsAddDoi(doi);
    refsMsg.value = result.ok
      ? result.added
        ? `已加入：${result.entry?.title || doi}`
        : "这条已经在库里了"
      : `加失败：${result.error}`;
    if (result.ok) doiInput.value = "";
    await loadRefs();
  } finally {
    refsBusy.value = false;
  }
}

async function exportRefs(format: "ris" | "bibtex" | "enw") {
  refsBusy.value = true;
  try {
    const result = await refsExport(format);
    refsMsg.value = result.ok ? `已导出到 ${result.path}` : `导出失败：${result.error}`;
    if (result.ok && result.path) {
      lastExport.value = result.path;
      void revealPath(result.path);
    }
  } finally {
    refsBusy.value = false;
  }
}

// EndNote 检测（旧版 endnote.detect）：装了就直接拿它打开导出的文件，没装就给一句话说明
const endnote = ref<{ found: boolean; path: string; summary: string } | null>(null);
const lastExport = ref("");
const endnoteNote = ref("");

async function openInEndnote() {
  if (!lastExport.value) return;
  endnoteNote.value = "正在叫 EndNote…";
  try {
    const result = await endnoteOpen(lastExport.value);
    endnoteNote.value = result.ok ? "已经在 EndNote 里打开" : result.error;
  } catch (err) {
    endnoteNote.value = `打不开：${String(err)}`;
  }
}

async function removeRef(id: number) {
  await refsDelete(id);
  await loadRefs();
}

const items = ref<Paper[]>([]);
const projects = ref<{ name: string; total: number }[]>([]);
const project = ref("");
const starsMin = ref(0);
const keyword = ref("");
const selectedId = ref<number | null>(null);
const message = ref("");
const selected = computed(
  () => items.value.find((item) => item.id === selectedId.value) || items.value[0] || null,
);

// ---- 流程图 ----
const flow = ref<LitFlow | null>(null);
const flowOpen = ref(false);
const flowBusy = ref(false);
const flowMsg = ref("");
const flowMode = ref<"graph" | "list">("graph");
const aiReady = ref(true);
const flowNodes = computed(() => flow.value?.nodes || []);

async function load() {
  const result = await papersList(project.value, starsMin.value, keyword.value);
  items.value = result.items || [];
  projects.value = result.projects || [];
  if (!items.value.find((item) => item.id === selectedId.value)) {
    selectedId.value = items.value.length ? items.value[0].id : null;
  }
  message.value = items.value.length ? "" : "还没有文献（旧版里读过 / 下载过的会出现在这里）";
}

async function refreshAiReady() {
  try {
    const state = await aiState();
    aiReady.value = !!state?.has_key;
  } catch {
    aiReady.value = false;
  }
}

// ---- 按意思搜（语义检索，B 方案：服务器只算向量，本地比）
//
// 用户 2026-11-12 选了「模型跑服务器本机、不花 API 钱」这条路；
// 关键词搜不到的（比如标题写 `across field strengths`、你想搜「跨场强」）靠它补。
const semanticHits = ref<
  {
    id: number;
    title: string;
    authors: string;
    year: string;
    venue: string;
    score: number;
  }[]
>([]);
const semanticBusy = ref(false);
const semanticMsg = ref("");
const semanticStatus = ref<{ indexed: number; papers: number; need_index: boolean } | null>(null);

async function refreshSemanticStatus() {
  try {
    semanticStatus.value = await papersSemanticStatus();
  } catch {
    semanticStatus.value = null;
  }
}

/** 建索引（`force` = 全部重建）；建完自动刷新状态 */
async function buildIndex(force: boolean) {
  if (semanticBusy.value) return;
  semanticBusy.value = true;
  semanticMsg.value = force ? "正在重建全部向量…" : "正在给新文献补向量…";
  try {
    const result = await papersSemanticIndex(force);
    semanticMsg.value = result.ok ? result.message || "建好了" : result.error || "建索引失败";
    await refreshSemanticStatus();
  } catch (err) {
    semanticMsg.value = `建索引失败：${String(err)}`;
  } finally {
    semanticBusy.value = false;
  }
}

async function searchSemantic() {
  const query = searchText.value.trim();
  if (!query) {
    semanticMsg.value = "先写要搜的意思，比如「跨场强 MRI 图像翻译」";
    return;
  }
  if (semanticBusy.value) return;
  semanticBusy.value = true;
  semanticHits.value = [];
  semanticMsg.value = "正在按意思搜…";
  try {
    // 还没索引就先建（第一次用会多花一两秒）
    if (semanticStatus.value?.need_index) {
      await buildIndex(false);
    }
    const result = await papersSemanticSearch(query, 10);
    if (!result.ok) {
      semanticMsg.value = result.error || "没搜到";
      return;
    }
    semanticHits.value = result.hits || [];
    semanticMsg.value = result.hits?.length
      ? `按意思找到 ${result.hits.length} 篇（从 ${result.indexed} 篇里排的）`
      : "没有相近的文献";
    if (result.stale && result.stale > 0) {
      semanticMsg.value += `；另有 ${result.stale} 篇用的是旧模型的向量，点「重建索引」重来一次`;
    }
  } catch (err) {
    semanticMsg.value = `按意思搜失败：${String(err)}`;
  } finally {
    semanticBusy.value = false;
    await refreshSemanticStatus();
  }
}

/** 点「看这篇」：切到左边那篇（不在当前筛选里就先清掉筛选） */
async function selectSemantic(hit: { id: number }) {
  if (!items.value.some((one) => one.id === hit.id)) {
    project.value = "";
    starsMin.value = 0;
    keyword.value = "";
    await load();
  }
  selectedId.value = hit.id;
}

// ---- 下载 PDF（J1-2，对应旧版 lit.download_pdf）----
const downloadBusy = ref(false);
const downloadMsg = ref("");

/* ---- 下载链接 + 机构登录弹窗（M4，对应旧版 `lit.access_url` / `library_page.py`）----
 *
 * 用户 2026-11-14：「文献库能否增加下载链接，然后如果需要机构登录，可以弹窗一个
 * DanLume 内置浏览器，让用户操作（由于此处为一次性页面所以弹窗可以接受）」。
 * 弹的是 `EmbedWebview`（子 Webview），登录态留在 WebView2 的 profile 里，
 * 下回再点就不用重新登录。
 */
const links = ref<PaperLinks | null>(null);
const libraryOpen = ref(false);
const libraryUrl = ref("");
const accessState = ref<LibraryAccess | null>(null);
const prefixDraft = ref("");
const libraryMsg = ref("");
const databases = ref<LibraryDatabase[]>([]);

const libraryHomeUrl = computed(() => accessState.value?.dbnav || "https://libdbnav.fudan.edu.cn/");

/** 这篇文献有没有可点的链接 */
const hasLink = computed(() => !!(links.value?.download || links.value?.page));

async function loadLinks() {
  const paper = selected.value;
  if (!paper) {
    links.value = null;
    return;
  }
  try {
    links.value = await papersLinks(paper.id);
  } catch {
    links.value = null;
  }
}

async function loadDatabases() {
  if (databases.value.length) return;
  try {
    databases.value = (await papersDatabases()).items || [];
  } catch {
    databases.value = [];
  }
}

/** 打开内置浏览器（弹窗）；地址按当前访问方式套前缀 */
async function openDatabase(url: string, label = "") {
  const target = (url || "").trim();
  if (!target) {
    libraryMsg.value = "这篇没有可用的链接";
    return;
  }
  if (accessState.value === null) {
    try {
      accessState.value = await libraryAccess();
      prefixDraft.value = accessState.value.prefix || "";
    } catch {
      /* 读不到就用默认（校内直连） */
    }
  }
  await loadDatabases();
  let finalUrl = target;
  try {
    const rewritten = await libraryAccessUrl(target);
    if (rewritten.ok && rewritten.url) finalUrl = rewritten.url;
  } catch {
    /* 改写失败就按原地址开 */
  }
  libraryMsg.value =
    label || (finalUrl !== target ? "已套上校外访问前缀" : "需要机构登录就在这里登录，登录态会留着");
  libraryUrl.value = finalUrl;
  libraryOpen.value = true;
}

function openDownloadLink() {
  const target = links.value?.download || links.value?.page || "";
  void openDatabase(target, links.value?.needs_login ? "没有现成 PDF：登录后在这里自己下" : "");
}

function openPageLink() {
  void openDatabase(links.value?.page || "", "");
}

async function toggleAccessMode() {
  const next = accessState.value?.mode === "prefix" ? "campus" : "prefix";
  await saveAccess(next, prefixDraft.value);
}

async function saveAccess(mode: string, prefix: string) {
  try {
    const saved = await librarySetAccess(mode, prefix);
    accessState.value = saved;
    prefixDraft.value = saved.prefix || "";
    libraryMsg.value =
      saved.mode === "prefix"
        ? saved.prefix
          ? "已切到「校外访问前缀」：地址会先套上前缀再打开"
          : "已切到校外，但前缀还空着（填 WebVPN / libproxy 的前缀）"
        : "已切到「校内直连」";
    await loadLinks();
  } catch (err) {
    libraryMsg.value = `保存失败：${String(err)}`;
  }
}

/** 下载失败时，多半是要机构登录：直接把内置浏览器弹出来 */
const LOGIN_HINTS = ["机构", "登录", "HTTP 40", "HTTP 41", "不是 PDF", "验证"];

async function downloadPaper() {
  const paper = selected.value;
  if (!paper || downloadBusy.value) return;
  downloadBusy.value = true;
  downloadMsg.value = "正在下载…";
  try {
    const result = await papersDownload(paper.id);
    downloadMsg.value = result.summary || (result.ok ? "下载完成" : "下载失败");
    if (result.ok) {
      await load();
      await loadLinks();
      return;
    }
    const text = String(result.summary || "");
    if (LOGIN_HINTS.some((one) => text.includes(one)) && hasLink.value) {
      downloadMsg.value = `${text}（已打开内置浏览器：登录后在这里自己点下载）`;
      await openDatabase(links.value?.download || links.value?.page || "", "登录后在这里自己点下载");
    }
  } catch (err) {
    downloadMsg.value = `下载失败：${String(err)}`;
  } finally {
    downloadBusy.value = false;
  }
}

/** 切文献时看一下这篇之前有没有存过流程图（存在 data/lit_flows/flow-<id>.json） */
async function loadFlowForSelected() {
  const paper = selected.value;
  flowMsg.value = "";
  if (!paper) {
    flow.value = null;
    return;
  }
  try {
    const result = await litFlowLoad(paper.id);
    flow.value = result.ok ? result : null;
  } catch {
    flow.value = null;
  }
}

async function generateFlow(force = false) {
  const paper = selected.value;
  if (!paper || flowBusy.value) return;
  flowBusy.value = true;
  flowMsg.value = "";
  try {
    const result = await litFlowGenerate(paper.id, {
      force,
      title: paper.title,
      text: paper.abstract || "",
    });
    if (!result.ok) {
      flow.value = null;
      flowMsg.value = result.need_key
        ? "还没有可用的模型：去 设置 → AI（API 服务）里填好接口地址和 API Key，再回来点一次"
        : result.error || "生成失败，再试一次";
      return;
    }
    flow.value = result;
    flowMode.value = result.fallback ? "list" : "graph";
    flowOpen.value = true;
    flowMsg.value = result.fallback
      ? "模型这次没按 JSON 回，已降级成「纯文本步骤列表」（可以点「重新生成」再试一次）"
      : result.cached
        ? "这是上次存下来的流程图；想重做一次就点「重新生成」"
        : "";
  } catch (err) {
    flowMsg.value = `生成失败：${String(err)}`;
  } finally {
    flowBusy.value = false;
  }
}

async function removeFlow() {
  const paper = selected.value;
  if (!paper) return;
  try {
    const result = await litFlowDelete(paper.id);
    if (!result.ok) {
      flowMsg.value = result.error || "删除失败";
      return;
    }
    flow.value = null;
    flowOpen.value = false;
    flowMsg.value = "已删除这篇文献存下来的流程图";
  } catch (err) {
    flowMsg.value = `删除失败：${String(err)}`;
  }
}

/** 相邻两个节点之间有没有带说明的边（模型给的 label 画在箭头旁边） */
function edgeLabel(from: string, to: string): string {
  const found = (flow.value?.edges || []).find((edge) => edge.from === from && edge.to === to);
  return found?.label || "";
}

function flowText(): string {
  return flowNodes.value
    .map((node, index) => `${index + 1}. ${node.label}${node.detail ? `（${node.detail}）` : ""}`)
    .join("\n");
}

async function copyFlowSteps() {
  const text = flowText();
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    flowMsg.value = "已复制步骤";
  } catch {
    flowMsg.value = text;
  }
}

async function toggleStar(paper: Paper, star: number) {
  const next = paper.stars === star ? 0 : star;
  await paperUpdate(paper.id, { stars: next });
  paper.stars = next;
}

async function copyCitation() {
  if (!selected.value) return;
  const result = await paperCitation(selected.value.id);
  if (!result.ok) {
    message.value = "取引用失败";
    return;
  }
  try {
    await navigator.clipboard.writeText(result.text);
    message.value = "已复制引用（GB/T 7714）";
  } catch {
    message.value = result.text;
  }
}

const stars = [1, 2, 3, 4, 5];

watch(selectedId, () => {
  flowOpen.value = false;
  loadFlowForSelected();
  void loadLinks();
});

// 选中的那篇可能是列表第一项（`selected` 默认取 items[0]），所以盯 `selected` 本身
watch(selected, () => {
  void loadLinks();
});

onMounted(async () => {
  await load();
  await refreshAiReady();
  await loadFlowForSelected();
  await refreshSemanticStatus();
  try {
    endnote.value = await endnoteDetect();
  } catch {
    endnote.value = null;
  }
});
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">文献库</h1>
        <div class="muted">共 {{ items.length }} 篇 · 项目、星级、引用一条龙</div>
      </div>
      <div class="spacer" />
      <SoftButton :height="32" :font-size="12.5" glyph="REFRESH" @click="load">刷新</SoftButton>
      <SoftButton :height="32" :font-size="12.5" glyph="FOLDER" @click="openPath('')">
        打开文献库目录
      </SoftButton>
      <SoftButton :height="32" :font-size="12.5" glyph="ADD" @click="openRefs">
        {{ refsOpen ? "收起参考文献" : "参考文献" }}
      </SoftButton>
    </div>

    <!-- 在线检索：arXiv + Crossref（旧版 lit.search_all） -->
    <div class="refs-card">
      <div class="refs-head">
        <b>在线检索</b>
        <input
          v-model="searchText"
          class="search-input"
          type="search"
          placeholder="关键词，比如 transformer reasoning"
          @keydown.enter="searchOnline"
        />
        <SoftButton :height="28" :font-size="12" :disabled="searchBusy" @click="searchOnline">
          {{ searchBusy ? "搜索中…" : "搜 arXiv / Crossref / S2" }}
        </SoftButton>
        <SoftButton
          :height="28"
          :font-size="12"
          :disabled="semanticBusy"
          title="本地文献库「按意思搜」：问句由服务器算一次向量，其余在本地比"
          @click="searchSemantic"
        >
          {{ semanticBusy ? "思考中…" : "按意思搜" }}
        </SoftButton>
      </div>
      <div v-if="searchMsg" class="hint">{{ searchMsg }}</div>
      <div v-if="semanticStatus" class="hint">
        语义索引：{{ semanticStatus.indexed }}/{{ semanticStatus.papers }} 篇
        <template v-if="semanticStatus.need_index">
          <SoftButton :height="24" :font-size="11" :disabled="semanticBusy" @click="buildIndex(false)">
            建索引
          </SoftButton>
        </template>
        <template v-else>
          <SoftButton :height="24" :font-size="11" :disabled="semanticBusy" @click="buildIndex(true)">
            重建索引
          </SoftButton>
        </template>
      </div>
      <div v-if="semanticMsg" class="hint">{{ semanticMsg }}</div>
      <div v-for="hit in semanticHits" :key="`sem-${hit.id}`" class="ref-row">
        <div class="ref-main">
          <b>{{ hit.title }}</b>
          <span class="ref-meta">
            {{ hit.authors || "（无作者）" }}{{ hit.year ? ` · ${hit.year}` : "" }}{{
              hit.venue ? ` · ${hit.venue}` : ""
            }}
          </span>
        </div>
        <span class="coverage full">相似度 {{ (hit.score * 100).toFixed(0) }}%</span>
        <SoftButton :height="26" :font-size="11.5" @click="selectSemantic(hit)">看这篇</SoftButton>
      </div>
      <div v-for="hit in searchHits" :key="`${hit.source}-${hit.source_id}`" class="ref-row">
        <div class="ref-main">
          <b>{{ hit.title }}</b>
          <span class="ref-meta">
            {{ hit.authors || "（无作者）" }}{{ hit.year ? ` · ${hit.year}` : "" }}{{
              hit.venue ? ` · ${hit.venue}` : ""
            }}
          </span>
        </div>
        <span
          v-if="searchRelaxed && hit.total_terms"
          class="coverage"
          :class="{ full: (hit.matched_terms || 0) >= hit.total_terms }"
        >
          命中 {{ hit.matched_terms }}/{{ hit.total_terms }} 个词
        </span>
        <span class="ref-doi">{{ hit.source }}</span>
        <SoftButton :height="26" :font-size="11.5" @click="addHit(hit)">加入文献库</SoftButton>
      </div>
      <div v-if="searchHits.length" class="search-more">
        <span class="tiny">{{ searchTotalText ? `库里总量：${searchTotalText}` : "" }}</span>
        <span class="spacer" />
        <SoftButton
          :height="28"
          :font-size="12"
          :disabled="searchMoreBusy || !searchMore"
          @click="searchMoreResults"
        >
          {{ searchMoreBusy ? "读取中…" : searchMore ? "再看 20 条" : "没有更多了" }}
        </SoftButton>
      </div>
    </div>

    <!-- 参考文献层（旧版 refs.py）：BibTeX / RIS / DOI 导入，导出成 EndNote 能吃的格式 -->
    <div v-if="refsOpen" class="refs-card">
      <div class="refs-head">
        <b>参考文献（{{ refsItems.length }} 条）</b>
        <span class="spacer" />
        <SoftButton :height="28" :font-size="12" :disabled="refsBusy" @click="importRefs">
          导入 BibTeX / RIS
        </SoftButton>
        <SoftButton :height="28" :font-size="12" :disabled="refsBusy" @click="exportRefs('ris')">
          导出 RIS
        </SoftButton>
        <SoftButton :height="28" :font-size="12" :disabled="refsBusy" @click="exportRefs('bibtex')">
          导出 BibTeX
        </SoftButton>
        <SoftButton :height="28" :font-size="12" :disabled="refsBusy" @click="exportRefs('enw')">
          导出 EndNote
        </SoftButton>
      </div>
      <div class="refs-add">
        <input v-model="doiInput" type="text" placeholder="粘一个 DOI，回车就加进来（走 Crossref）" @keydown.enter="addDoi" />
        <SoftButton :height="28" :font-size="12" :disabled="refsBusy" @click="addDoi">加 DOI</SoftButton>
      </div>
      <div v-if="refsMsg" class="hint">{{ refsMsg }}</div>
      <!-- EndNote：装了就能一键打开刚导出的文件；没装就说清楚该去哪儿装 -->
      <div v-if="endnote" class="endnote-line">
        <template v-if="endnote.found">
          <Pill text="装了 EndNote" color="var(--success)" :font-size="11" />
          <span class="endnote-path" :title="endnote.path">{{ endnote.path }}</span>
          <SoftButton
            v-if="lastExport"
            :height="24"
            :font-size="11.5"
            @click="openInEndnote"
          >
            用 EndNote 打开刚导出的文件
          </SoftButton>
        </template>
        <span v-else class="endnote-hint">{{ endnote.summary }}</span>
        <span v-if="endnoteNote" class="tiny">{{ endnoteNote }}</span>
      </div>
      <div v-if="!refsItems.length" class="muted tiny">
        还没有参考文献。可以导入 .bib / .ris，或者粘一个 DOI。
      </div>
      <div v-for="ref in refsItems" :key="ref.id" class="ref-row">
        <div class="ref-main">
          <b>{{ ref.title || "（无标题）" }}</b>
          <span class="ref-meta">
            {{ ref.authors || "（无作者）" }}{{ ref.year ? ` · ${ref.year}` : "" }}{{
              ref.venue ? ` · ${ref.venue}` : ""
            }}
          </span>
        </div>
        <span v-if="ref.doi" class="ref-doi">{{ ref.doi }}</span>
        <button class="ref-del" title="删掉这条" @click="removeRef(ref.id)">×</button>
      </div>
    </div>

    <div class="bar">
      <input v-model="keyword" type="search" placeholder="过滤本机文献：标题 / 作者 / 期刊…" @change="load" />
      <div class="projects">
        <button :class="{ on: !project }" @click="(project = ''), load()">全部</button>
        <button
          v-for="one in projects"
          :key="one.name"
          :class="{ on: project === one.name }"
          @click="(project = one.name), load()"
        >
          {{ one.name || "未分类" }} ({{ one.total }})
        </button>
      </div>
      <select v-model.number="starsMin" @change="load">
        <option :value="0">全部星级</option>
        <option :value="1">1 星以上</option>
        <option :value="3">3 星以上</option>
        <option :value="5">只看 5 星</option>
      </select>
    </div>

    <div class="body">
      <div class="list">
        <button
          v-for="paper in items"
          :key="paper.id"
          class="row"
          :class="{ on: selected?.id === paper.id }"
          @click="selectedId = paper.id"
        >
          <span class="stars">
            <b v-for="star in stars" :key="star" :class="{ lit: paper.stars >= star }" @click.stop="toggleStar(paper, star)">★</b>
          </span>
          <span class="text">
            <span class="title">{{ paper.title }}</span>
            <span class="meta">
              {{ paper.authors.slice(0, 40) }} · {{ paper.venue }} · {{ paper.year || "" }}
            </span>
            <span v-if="paper.summary_zh" class="summary">{{ paper.summary_zh }}</span>
          </span>
          <span class="project">{{ paper.project || "未分类" }}</span>
        </button>
        <div v-if="!items.length" class="empty">{{ message }}</div>
      </div>

      <div class="detail">
        <template v-if="selected">
          <div class="detail-head">
            <IconBadge glyph="DOCUMENT" :tone="selected.stars >= 3 ? 'rose' : 'sky'" :size="30" />
            <div class="head-text">
              <div class="title">{{ selected.title }}</div>
              <div class="meta">{{ selected.authors }}</div>
              <div class="meta">{{ selected.venue }} · {{ selected.year }} · {{ selected.doi }}</div>
            </div>
          </div>
          <div class="scroll">
            <div class="field">
              <div class="label">摘要</div>
              <div class="text">{{ selected.abstract || "（没有摘要）" }}</div>
            </div>
            <div class="field">
              <div class="label">中文概述</div>
              <div class="text">{{ selected.summary_zh || "（还没有自动概述）" }}</div>
            </div>
            <div class="field">
              <div class="label">备注</div>
              <div class="text">{{ selected.note || "（空）" }}</div>
            </div>
            <!-- 下载链接（M4）：点了弹内置浏览器；需要机构登录就在里面登录 -->
            <div class="field">
              <div class="label">下载链接</div>
              <div class="text">
                <button
                  v-if="hasLink"
                  class="ext-link"
                  :title="links?.download_url || links?.download || ''"
                  @click="openDownloadLink"
                >
                  {{ links?.download || links?.page }}
                </button>
                <span v-else class="muted">这篇没有可下载的链接（可以去「图书馆数据库」里按标题搜）</span>
                <div v-if="links?.needs_login" class="hint">
                  没有现成的开放 PDF：多半要学校权限，点上面的链接会弹出内置浏览器让你登录。
                </div>
              </div>
            </div>
          </div>
          <div class="detail-foot">
            <Pill :text="`${selected.stars} 星`" :font-size="11.5" />
            <SoftButton :height="28" :font-size="12" @click="copyCitation">复制引用</SoftButton>
            <!-- 下载到文献库：library/<项目>/<年份>/<标题>.pdf（旧版 lit.download_pdf） -->
            <SoftButton
              :height="28"
              :font-size="12"
              :disabled="downloadBusy || !!selected.file_path"
              @click="downloadPaper"
            >
              {{ selected.file_path ? "已下载" : downloadBusy ? "下载中…" : "下载 PDF" }}
            </SoftButton>
            <SoftButton
              :height="28"
              :font-size="12"
              :disabled="!selected.file_path"
              @click="openPath(selected.file_path)"
            >
              打开 PDF
            </SoftButton>
            <SoftButton :height="28" :font-size="12" :disabled="!hasLink" @click="openPageLink">
              原文页面
            </SoftButton>
            <SoftButton :height="28" :font-size="12" @click="openDatabase(libraryHomeUrl)">
              图书馆数据库
            </SoftButton>
          </div>
          <div v-if="downloadMsg" class="detail-foot">
            <span class="tiny">{{ downloadMsg }}</span>
          </div>
          <div class="detail-foot flow-actions">
            <SoftButton
              variant="primary"
              :height="28"
              :font-size="12"
              :disabled="flowBusy || !aiReady"
              @click="generateFlow(!!flow)"
            >
              {{ flowBusy ? "生成中…" : flow ? "重新生成" : "生成流程图" }}
            </SoftButton>
            <SoftButton v-if="flow" :height="28" :font-size="12" @click="flowOpen = true">
              查看流程图
            </SoftButton>
            <span v-if="!aiReady" class="hint">
              没配 API Key：去 设置 → AI（API 服务）填好接口地址和 Key，再回来点这里
            </span>
            <span v-else-if="flowMsg" class="hint">{{ flowMsg }}</span>
          </div>
        </template>
        <div v-else class="empty">选左边一篇文献</div>
      </div>
    </div>

    <!-- 图书馆数据库（M4）：一次性弹窗，里面就是 DanLume 自己的浏览器 -->
    <div v-if="libraryOpen" class="lib-mask" @click.self="libraryOpen = false">
      <div class="lib-card">
        <div class="lib-head">
          <div class="lib-titles">
            <div class="h2">图书馆数据库（内置浏览器）</div>
            <div class="tiny">
              数据库的全文 / 下载由学校买的权限决定；校外先登录再套「校外访问前缀」
            </div>
          </div>
          <span class="spacer" />
          <SoftButton :height="28" :font-size="12" @click="openDatabase(libraryHomeUrl)">
            数据库导航
          </SoftButton>
          <SoftButton
            :height="28"
            :font-size="12"
            @click="openDatabase(accessState?.uis || 'https://id.fudan.edu.cn/')"
          >
            统一身份认证
          </SoftButton>
          <SoftButton
            :height="28"
            :font-size="12"
            @click="openDatabase(accessState?.webvpn || 'https://webvpn.fudan.edu.cn/')"
          >
            WebVPN
          </SoftButton>
          <SoftButton variant="primary" :height="28" :font-size="12" @click="libraryOpen = false">
            关闭
          </SoftButton>
        </div>
        <div class="lib-access">
          <SoftButton :height="28" :font-size="12" @click="toggleAccessMode">
            {{ accessState?.mode === "prefix" ? "校外访问（前缀）" : "校内直连" }}
          </SoftButton>
          <input
            v-model="prefixDraft"
            type="text"
            placeholder="校外访问前缀，例如 https://libproxy.fudan.edu.cn:8080/LibProxy/?url="
            @keyup.enter="saveAccess('prefix', prefixDraft)"
          />
          <SoftButton :height="28" :font-size="12" @click="saveAccess('prefix', prefixDraft)">
            保存前缀
          </SoftButton>
        </div>
        <div class="lib-chips">
          <span class="tiny">常用：</span>
          <button
            v-for="db in databases"
            :key="db.id"
            class="chip"
            :title="db.url"
            @click="openDatabase(db.url)"
          >
            {{ db.name }}
          </button>
        </div>
        <EmbedWebview
          v-if="libraryUrl"
          kind="library"
          :url="libraryUrl"
          home-label="数据库导航"
        />
        <div v-if="libraryMsg" class="tiny lib-msg">{{ libraryMsg }}</div>
      </div>
    </div>

    <!-- 流程图：节点卡片 + SVG 竖线箭头，纯 CSS/SVG，不引第三方图库 -->
    <div v-if="flowOpen && flow" class="flow-mask" @click.self="flowOpen = false">
      <div class="flow-card">
        <div class="flow-head">
          <div class="flow-titles">
            <div class="h2">{{ flow.title || "方法流程图" }}</div>
            <div class="tiny">
              共 {{ flowNodes.length }} 步 ·
              {{ flow.fallback ? "纯文本降级" : "结构化节点图" }}
              <template v-if="flow.generated_at"> · {{ flow.generated_at }}</template>
              <template v-if="flow.cached"> · 上次的结果</template>
            </div>
          </div>
          <div class="spacer" />
          <div class="mode-switch">
            <button :class="{ on: flowMode === 'graph' }" @click="flowMode = 'graph'">流程图</button>
            <button :class="{ on: flowMode === 'list' }" @click="flowMode = 'list'">步骤列表</button>
          </div>
          <SoftButton :height="28" :font-size="12" @click="copyFlowSteps">复制步骤</SoftButton>
          <SoftButton :height="28" :font-size="12" :disabled="flowBusy" @click="generateFlow(true)">
            {{ flowBusy ? "生成中…" : "重新生成" }}
          </SoftButton>
          <SoftButton variant="danger" :height="28" :font-size="12" @click="removeFlow">
            删除
          </SoftButton>
          <SoftButton :height="28" :font-size="12" @click="flowOpen = false">关闭</SoftButton>
        </div>
        <div v-if="flowMsg" class="flow-msg">{{ flowMsg }}</div>
        <div class="flow-body">
          <!-- 箭头形状只定义一次，下面每一段连线都用 marker-end 引用 -->
          <svg class="flow-defs" width="0" height="0" aria-hidden="true">
            <defs>
              <marker
                id="lit-flow-arrow"
                viewBox="0 0 8 8"
                refX="4"
                refY="4"
                markerWidth="7"
                markerHeight="7"
                orient="auto"
              >
                <path d="M0,0 L8,4 L0,8 z" fill="var(--accent)" />
              </marker>
            </defs>
          </svg>

          <template v-if="flowMode === 'graph'">
            <div class="flow-canvas">
              <template v-for="(node, index) in flowNodes" :key="node.id || index">
                <div class="flow-node" :class="{ head: index === 0 }">
                  <span class="flow-index">{{ index + 1 }}</span>
                  <span class="flow-text">
                    <b>{{ node.label }}</b>
                    <span v-if="node.detail" class="flow-detail">{{ node.detail }}</span>
                  </span>
                </div>
                <div v-if="index < flowNodes.length - 1" class="flow-gap">
                  <svg width="100%" height="26">
                    <line
                      x1="50%"
                      y1="0"
                      x2="50%"
                      y2="18"
                      stroke="var(--accent)"
                      stroke-width="1.6"
                      marker-end="url(#lit-flow-arrow)"
                    />
                  </svg>
                  <span v-if="edgeLabel(node.id, flowNodes[index + 1].id)" class="flow-edge-label">
                    {{ edgeLabel(node.id, flowNodes[index + 1].id) }}
                  </span>
                </div>
              </template>
            </div>
          </template>
          <template v-else>
            <ol class="flow-list">
              <li v-for="(node, index) in flowNodes" :key="node.id || index">
                <b>{{ node.label }}</b>
                <span v-if="node.detail" class="flow-detail">{{ node.detail }}</span>
              </li>
            </ol>
          </template>
          <div v-if="!flowNodes.length" class="empty">
            模型没给出能用的步骤，点「重新生成」再试一次
          </div>
          <div v-if="flow.fallback && flow.raw" class="flow-raw">
            <div class="label">模型原话（没能解析成 JSON，下面是原文）</div>
            <pre>{{ flow.raw }}</pre>
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
.bar {
  display: flex;
  align-items: center;
  gap: 10px;
}
.bar input {
  width: 300px;
}
.projects {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}
.projects button {
  height: 24px;
  padding: 0 10px;
  border-radius: var(--radius-pill);
  border: 1px solid var(--border-soft);
  background: var(--card);
  color: var(--muted);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.projects button.on {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, var(--card));
}
.body {
  flex: 1;
  display: flex;
  gap: 12px;
  min-height: 0;
}
.list {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.row {
  display: flex;
  align-items: flex-start;
  gap: 9px;
  padding: 9px 11px;
  border-radius: var(--radius-card);
  border: 1px solid var(--border-soft);
  background: var(--card);
  font-family: inherit;
  text-align: left;
  cursor: pointer;
}
.row.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 6%, var(--card));
}
.stars {
  display: flex;
  gap: 1px;
  font-size: 11px;
  color: var(--border);
  flex: none;
}
.stars b {
  font-weight: 400;
}
.stars b.lit {
  color: #f5a623;
}
.text {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.title {
  font-size: 13.5px;
  color: var(--text);
  overflow-wrap: anywhere;
}
.meta {
  font-size: 11.5px;
  color: var(--muted);
}
.summary {
  font-size: 11.5px;
  color: var(--text-soft);
  overflow: hidden;
  text-overflow: ellipsis;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}
.project {
  font-size: 11px;
  color: var(--faint);
  white-space: nowrap;
}
.detail {
  width: 42%;
  min-width: 340px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: var(--radius-card);
  padding: 12px 14px;
  min-height: 0;
}
.detail-head {
  display: flex;
  gap: 10px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-soft);
}
.head-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.head-text .title {
  font-size: 14.5px;
  font-weight: 640;
}
.scroll {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.label {
  font-size: 11.5px;
  color: var(--faint);
}
.text {
  font-size: 12.5px;
  line-height: 1.7;
  color: var(--text-soft);
  user-select: text;
}
.detail-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--border-soft);
}
.empty {
  font-size: 12.5px;
  color: var(--muted);
  padding: 8px 2px;
}
/* 列表行一律左对齐（button 默认居中） */
.list .row,
.list .row > span {
  text-align: left;
}

/* ------------------------------------------------------------ 流程图 */
.flow-actions {
  flex-wrap: wrap;
  row-gap: 4px;
  /* 上一行 detail-foot 已经有分隔线了，这里不重复画 */
  border-top: none;
  padding-top: 2px;
}
.flow-actions .hint {
  font-size: 11px;
  color: var(--muted);
  line-height: 1.5;
}
/* 下载链接（M4）：一行地址，点开就是内置浏览器 */
.ext-link {
  border: none;
  background: none;
  padding: 0;
  font-size: 12px;
  color: var(--accent);
  text-align: left;
  cursor: pointer;
  word-break: break-all;
  text-decoration: underline dotted;
}
.ext-link:hover {
  text-decoration: underline;
}
.hint {
  font-size: 11px;
  color: var(--muted);
  line-height: 1.6;
  margin-top: 3px;
}
/* 图书馆数据库弹窗：里面嵌的是子 Webview，弹窗自己只留页头和工具条 */
.lib-mask {
  position: fixed;
  inset: 0;
  z-index: 70;
  background: rgba(0, 0, 0, 0.34);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 20px;
}
.lib-card {
  width: min(1180px, 96vw);
  height: min(880px, 92vh);
  display: flex;
  flex-direction: column;
  gap: 7px;
  padding: 12px 14px 14px;
  border-radius: var(--radius-card);
  border: 1px solid var(--hairline, var(--border-soft));
  background: var(--card);
  box-shadow: var(--shadow-pop);
}
.lib-head,
.lib-access,
.lib-chips {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
}
.lib-titles {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.lib-access input {
  flex: 1;
  height: 28px;
  font-size: 12px;
}
.lib-chips {
  flex-wrap: wrap;
}
.chip {
  height: 24px;
  padding: 0 9px;
  border-radius: 12px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt, transparent);
  color: var(--text-soft);
  font-size: 11.5px;
  cursor: pointer;
}
.chip:hover {
  border-color: var(--accent);
  color: var(--text);
}
.lib-msg {
  color: var(--muted);
}
.flow-mask {
  position: fixed;
  inset: 0;
  z-index: 60;
  background: rgba(0, 0, 0, 0.32);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 28px;
}
.flow-card {
  width: min(720px, 100%);
  max-height: 100%;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 16px 18px 18px;
  border-radius: var(--radius-card);
  border: 1px solid var(--hairline, var(--border-soft));
  background: var(--card);
  box-shadow: var(--shadow-pop);
}
.flow-head {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-soft);
}
.flow-titles {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.mode-switch {
  display: flex;
  gap: 3px;
  padding: 2px;
  border-radius: var(--radius-pill);
  background: var(--card-alt);
}
.mode-switch button {
  height: 22px;
  padding: 0 10px;
  border: none;
  border-radius: var(--radius-pill);
  background: transparent;
  color: var(--muted);
  font-family: inherit;
  font-size: 11.5px;
  cursor: pointer;
}
.mode-switch button.on {
  background: var(--card);
  color: var(--text);
  font-weight: 600;
}
.flow-msg {
  font-size: 11.5px;
  color: var(--muted);
  background: var(--card-alt);
  border-radius: 8px;
  padding: 6px 9px;
  line-height: 1.6;
}
.flow-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 2px;
}
.flow-defs {
  position: absolute;
  width: 0;
  height: 0;
  overflow: hidden;
}
/* 节点卡片竖排：浅色边框 + 第一颗用强调色淡底（对应旧版第一颗高亮） */
.flow-canvas {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 0;
}
.flow-node {
  display: flex;
  align-items: flex-start;
  gap: 9px;
  padding: 9px 12px;
  border-radius: var(--radius-card);
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text);
}
.flow-node.head {
  border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
  background: color-mix(in srgb, var(--accent) 9%, var(--card));
}
.flow-index {
  flex: none;
  width: 20px;
  height: 20px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 14%, var(--card));
  color: var(--accent);
  font-size: 11px;
  font-weight: 640;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.flow-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.flow-text b {
  font-size: 13px;
  font-weight: 600;
  overflow-wrap: anywhere;
}
.flow-detail {
  font-size: 11.5px;
  color: var(--muted);
  line-height: 1.6;
  overflow-wrap: anywhere;
  user-select: text;
}
/* 连线：一段 26px 高的槽，里面是 SVG 竖线 + 箭头（marker-end） */
.flow-gap {
  position: relative;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.flow-gap svg {
  display: block;
  width: 100%;
  height: 26px;
}
.flow-edge-label {
  position: absolute;
  left: calc(50% + 14px);
  max-width: 46%;
  font-size: 10.5px;
  color: var(--muted);
  background: var(--card);
  border: 1px solid var(--border-soft);
  border-radius: 999px;
  padding: 0 7px;
  line-height: 16px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
/* 降级展示：纯文本步骤列表 */
.flow-list {
  margin: 0;
  padding-left: 20px;
  display: flex;
  flex-direction: column;
  gap: 7px;
  font-size: 13px;
  color: var(--text);
  user-select: text;
}
.flow-list li {
  line-height: 1.65;
}
.flow-list b {
  font-weight: 600;
}
.flow-list .flow-detail {
  display: block;
}
.flow-raw .label {
  font-size: 11px;
  color: var(--faint);
  margin: 10px 0 4px;
}
.flow-raw pre {
  margin: 0;
  padding: 8px 10px;
  border-radius: 8px;
  background: var(--card-alt);
  color: var(--text-soft);
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.6;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
}
/* 参考文献层（BibTeX / RIS / DOI） */
.refs-card {
  margin: 0 0 10px;
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 320px;
  overflow: auto;
}
.refs-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
}
.refs-add {
  display: flex;
  gap: 6px;
}
/* EndNote 那一行：装了显示路径 + 打开按钮，没装就一句话说明 */
.endnote-line {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  font-size: 11.5px;
  color: var(--muted);
}
.endnote-path {
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-soft);
}
.endnote-hint {
  line-height: 1.6;
}
.search-more {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-top: 6px;
  border-top: 1px solid var(--border-soft);
}
.coverage {
  font-size: 11px;
  color: var(--muted);
  white-space: nowrap;
  padding: 1px 6px;
  border-radius: 999px;
  border: 1px solid var(--border-soft);
}
.coverage.full {
  color: var(--accent);
  border-color: var(--accent);
}
.refs-head .search-input {
  flex: 1;
  min-width: 160px;
  font-size: 12px;
  padding: 5px 9px;
}
.refs-add input {
  flex: 1;
  font-size: 12px;
  padding: 5px 9px;
}
.ref-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 2px;
  border-top: 1px solid var(--border-soft);
  font-size: 12px;
}
.ref-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  line-height: 1.3;
}
.ref-main b {
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ref-meta {
  font-size: 10.5px;
  color: var(--muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ref-doi {
  flex: none;
  font-size: 10.5px;
  color: var(--muted);
  font-family: var(--font-mono);
}
.ref-del {
  flex: none;
  border: 0;
  background: transparent;
  color: var(--muted);
  font-size: 14px;
  line-height: 1;
  cursor: pointer;
}
.ref-del:hover {
  color: var(--danger);
}
</style>
