<script setup lang="ts">
// 设置：左侧分类 + 搜索 + 20 张设置卡；字段与旧库的设置键一一对应，
// 保存直接写回 data/lumina.db（密码走 DPAPI，只写不读）。
import { computed, onMounted, reactive, ref } from "vue";
import Card from "../components/Card.vue";
import SoftButton from "../components/SoftButton.vue";
import IconBadge from "../components/IconBadge.vue";
import AdminPanel from "../components/AdminPanel.vue";
import AccountPanel from "../components/AccountPanel.vue";
import ContactsPanel from "../components/ContactsPanel.vue";
import Pill from "../components/Pill.vue";
import {
  ACCENTS,
  BG_TONES,
  setAccent,
  setAlpha,
  setBg,
  setBlur,
  setBrowserStyle,
  setBrowserTabs,
  setGlass,
  setMode,
  setTranslucent,
  state as themeState,
} from "../theme";
import {
  AI_TOOLS,
  RISK_LABELS,
  TOOL_GROUPS,
  TOOLS_ENABLED_KEY,
  toolEnabled,
  toolKey,
  toolsGloballyOn,
} from "../aiTools";
import {
  aiProfileDelete,
  aiProfileSave,
  accountState,
  appRelaunch,
  backupDb,
  dataExport,
  dataImport,
  dataInfo,
  openPath,
  pickFiles,
  pickSavePath,
  revealPath,
  aiProfiles,
  aiState,
  imapTest,
  mailTest,
  saveSecret,
  memoryAdd,
  memoryClear,
  memoryDelete,
  memoryList,
  memorySearch,
  memoryUpdate,
  settingsAll,
  settingsSave,
  uisLogin,
  proxySummary,
  mascotOpen,
  mascotClose,
  mascotResize,
  mailWatchState,
  mailWatchCheck,
  mailWatchResetSeen,
  proxyProbe,
  uisLogout,
  canvasDownload,
  canvasMigrate,
  canvasProbe,
  jwglProbe,
  jwglImport,
  uisCapture,
  uisOpenLogin,
  uisStatus,
  setSetting,
  type AiProfile,
  type AiState,
  type DataImportResult,
  type MemoryRow,
} from "../api";

type Kind = "text" | "password" | "select" | "switch" | "note" | "textarea";
type Field = {
  label: string;
  kind: Kind;
  key?: string;
  secret?: string;
  options?: string[];
  hint?: string;
  placeholder?: string;
};
type CardDef = {
  key: string;
  title: string;
  subtitle: string;
  category: string;
  fields: Field[];
  actions?: string[];
};

/**
 * 开关类字段的当前状态。
 * 约定是「值不是 '0' 就算开」，但代理（proxy_enabled）默认必须是**关**，
 * 否则设置页会显示「已启用」而后台其实没走代理（2026-11-12 修）。
 */
const OFF_BY_DEFAULT = new Set(["proxy_enabled"]);
function switchOn(key: string): boolean {
  const value = values[key] ?? "";
  if (OFF_BY_DEFAULT.has(key) && value === "") return false;
  return value !== "0";
}

/**
 * 浏览器多标签页（M5）：界面上的开关存在 localStorage（`theme.ts`），
 * 后端的内嵌网页读的是设置库里的 `browser_tabs`，两边都要写，
 * 不然开关一拨，网页那边还是老样子（旧版 `web_panel.set_tabs_enabled` 就是同一个坑）。
 */
function toggleBrowserTabs(on: boolean) {
  setBrowserTabs(on);
  persistTheme();
  values.browser_tabs = on ? "1" : "0";
  void settingsSave({ browser_tabs: on ? "1" : "0" });
}

/* ---------------- 看板娘（梅子） ---------------- */
const mascotMsg = ref("");

async function applyMascotScale() {
  try {
    const result = await mascotResize();
    mascotMsg.value = result.ok
      ? result.opened === false
        ? `大小已记下（${Math.round(result.width || 0)}×${Math.round(result.height || 0)}），下次打开梅子生效`
        : `已缩放到 ${Math.round(result.width || 0)}×${Math.round(result.height || 0)}`
      : `应用失败：${result.error || "未知原因"}`;
  } catch (err) {
    mascotMsg.value = `应用失败：${String(err)}`;
  }
}

async function openMascot() {
  try {
    await mascotOpen();
    await applyMascotScale();
  } catch (err) {
    mascotMsg.value = `打开失败：${String(err)}`;
  }
}

async function closeMascot() {
  try {
    await mascotClose();
    mascotMsg.value = "已关掉梅子";
  } catch (err) {
    mascotMsg.value = `关掉失败：${String(err)}`;
  }
}

/* ---------------- 盯邮箱（J2-6） ---------------- */
const watchBusy = ref(false);
const watchMsg = ref("");
const watchSummary = ref("");
const watchItems = ref<{ date: string; start_time?: string; title: string }[]>([]);

async function loadWatchSummary() {
  try {
    const result = await mailWatchState();
    watchSummary.value = result.summary || "";
  } catch {
    watchSummary.value = "";
  }
}

async function watchCheckNow() {
  if (watchBusy.value) return;
  watchBusy.value = true;
  watchMsg.value = "正在看邮箱…（拉命中发件人的新邮件并抽待办）";
  watchItems.value = [];
  try {
    const result = await mailWatchCheck();
    watchItems.value = result.items || [];
    const errors = (result.errors || []).join("；");
    watchMsg.value = result.items?.length
      ? `抽到 ${result.items.length} 条候选待办，已放到右上角卡片里让你确认`
      : errors || "这次没有需要加日程的事";
    await loadWatchSummary();
  } catch (err) {
    watchMsg.value = `看邮箱失败：${String(err)}`;
  } finally {
    watchBusy.value = false;
  }
}

async function watchResetSeen() {
  try {
    const result = await mailWatchResetSeen();
    watchSummary.value = result.summary || "";
    watchMsg.value = "已清空「已处理」记录，下次会重新看一遍旧邮件";
  } catch (err) {
    watchMsg.value = `清空失败：${String(err)}`;
  }
}

/* ---------------- 代理：测一下能不能连校内服务 ---------------- */
const proxyBusy = ref(false);
const proxyMsg = ref("");
const proxySummaryText = ref("");
const proxyResults = ref<{ name: string; ok: boolean; status: number; ms: number; error?: string }[]>([]);

async function loadProxySummary() {
  try {
    const result = await proxySummary();
    proxySummaryText.value = result.summary || "";
  } catch {
    proxySummaryText.value = "";
  }
}

async function testProxy() {
  if (proxyBusy.value) return;
  proxyBusy.value = true;
  proxyMsg.value = "正在测…（按当前设置连 eLearning 和教务处）";
  proxyResults.value = [];
  try {
    const result = await proxyProbe();
    proxyResults.value = (result.results || []) as typeof proxyResults.value;
    proxySummaryText.value = result.summary || proxySummaryText.value;
    proxyMsg.value = result.ok
      ? `能连上（${result.mode}）`
      : `连不上（${result.mode}）：${(result.results || [])
          .map((one) => one.error || `${one.name} ${one.status}`)
          .join("；")}`;
  } catch (err) {
    proxyMsg.value = `测试失败：${String(err)}`;
  } finally {
    proxyBusy.value = false;
  }
}

const CATEGORIES = ["常规", "仪表板", "邮件", "日程", "AI", "其他"];
const active = ref("常规");
const keyword = ref("");
const values = reactive<Record<string, string>>({});
const secrets = ref<string[]>([]);
const drafts = reactive<Record<string, string>>({});
const message = ref("");
const aiList = ref<AiProfile[]>([]);
const aiNow = ref<AiState | null>(null);
const info = ref<{
  data_dir: string;
  db_path: string;
  db_size: number;
  counts: Record<string, number>;
  version: string;
  kernel: string;
} | null>(null);
const aiEdit = reactive({ name: "", base_url: "", model: "" });
// 模型名候选（用户 2026-11-11：「DeepSeek 的 API 请提供更多的模型名让用户选择」）。
// 只做**下拉建议**，不限制手打：官方长期有效的就两个别名，其余看账号/平台支持。
const aiModelOptions: { value: string; label: string }[] = [
  { value: "deepseek-chat", label: "deepseek-chat · 官方通用对话（默认，长期有效）" },
  { value: "deepseek-reasoner", label: "deepseek-reasoner · 官方带思考过程（长期有效）" },
  { value: "deepseek-coder", label: "deepseek-coder · 早期代码模型（部分账号仍可用）" },
  { value: "deepseek-v3", label: "deepseek-v3 · 版本快照名" },
  { value: "deepseek-v3.1", label: "deepseek-v3.1 · 版本快照名" },
  { value: "deepseek-v3.2", label: "deepseek-v3.2 · 版本快照名（较新）" },
  { value: "deepseek-r1", label: "deepseek-r1 · 推理模型直连名（部分平台）" },
  { value: "deepseek-ai/DeepSeek-V3", label: "deepseek-ai/DeepSeek-V3 · 硅基流动等聚合平台" },
  { value: "deepseek-ai/DeepSeek-R1", label: "deepseek-ai/DeepSeek-R1 · 聚合平台的 R1" },
  { value: "Pro/deepseek-ai/DeepSeek-R1", label: "Pro/deepseek-ai/DeepSeek-R1 · 聚合平台付费版" },
];
// 工具清单与旧版 ai_tools.py 的 _SPECS 一一对应（41 个工具，7 组）
const toolsOn = ref<Record<string, boolean>>({});
const toolsGlobal = ref(true);
const adminOpen = ref(false);
// UIS 登录（旧版 uis_login.py）：学号读设置，密码只在本次登录时用
const uisPwd = ref("");
const uisBusy = ref(false);
const uisMsg = ref("");
const uisState = ref<{
  username: string;
  has_session: boolean;
  cookies: number;
  has_castgc: boolean;
  has_service_session?: boolean;
  has_saved_password: boolean;
  saved_at?: string;
  remember_password?: boolean;
} | null>(null);

// AI 记忆库（Rust 侧 memory.rs，旧版是 data/memory.md）：
// 列表 / 搜索 / 新增 / 编辑 / 删除 / 清空，全部走 memory_* 命令。
const memories = ref<MemoryRow[]>([]);
const memoryTotal = ref(0);
const memoryKeyword = ref("");
const memoryDraft = ref("");
const memoryMsg = ref("");
const memoryError = ref("");
const memoryEditing = reactive({ id: 0, content: "", tags: "" });

// 数据迁移（.dlpk 数据包）：导出 / 导入
const migrate = reactive({
  busy: false,
  exportPath: "",
  exportBytes: 0,
  exportTables: {} as Record<string, number>,
  importPath: "",
  confirmOpen: false,
  result: null as DataImportResult | null,
  relaunchMsg: "",
  error: "",
});

/** 字节数 → 人类看得懂的大小 */
function formatBytes(bytes: number): string {
  if (!bytes) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** 行数摘要：「日程 12 · 备忘 3 …」只挑常用的几张表 */
function tableSummary(tables: Record<string, number>): string {
  const labels: Record<string, string> = {
    events: "日程",
    memos: "备忘",
    notices: "通知",
    mails: "邮件",
    contacts: "联系人",
    papers: "文献",
    settings: "设置",
  };
  const parts = Object.entries(labels)
    .filter(([key]) => tables[key] !== undefined)
    .map(([key, label]) => `${label} ${tables[key]}`);
  return parts.length ? parts.join(" · ") : "（没有可显示的表）";
}

/** 导出数据包：选个保存位置 → 后端 VACUUM INTO 打快照 → 压成 zip */
async function doExportPackage() {
  if (migrate.busy) return;
  migrate.error = "";
  const stamp = new Date()
    .toISOString()
    .slice(0, 19)
    .replace(/[:T]/g, "-");
  let target = await pickSavePath(`DanLume-${stamp}.dlpk`, [
    { name: "DanLume 数据包", extensions: ["dlpk"] },
  ]);
  if (!target) return;
  if (!target.toLowerCase().endsWith(".dlpk")) target += ".dlpk";
  migrate.busy = true;
  try {
    const result = await dataExport(target);
    if (result.ok) {
      migrate.exportPath = result.path;
      migrate.exportBytes = result.bytes;
      migrate.exportTables = result.tables;
      message.value = `已导出数据包：${result.path}`;
    } else {
      migrate.error = `导出失败：${result.error}`;
    }
  } catch (err) {
    migrate.error = `导出失败：${String(err)}`;
  } finally {
    migrate.busy = false;
  }
}

/** 第一步：挑包，弹确认（不点确认不会动数据） */
async function chooseImportPackage() {
  if (migrate.busy) return;
  migrate.error = "";
  const picked = await pickFiles([{ name: "DanLume 数据包", extensions: ["dlpk"] }]);
  if (!picked.length) return;
  migrate.importPath = picked[0];
  migrate.result = null;
  migrate.relaunchMsg = "";
  migrate.confirmOpen = true;
}

/** 第二步：确认后才真的导入（后端会先自动备份当前库） */
async function confirmImportPackage() {
  if (migrate.busy || !migrate.importPath) return;
  migrate.busy = true;
  migrate.error = "";
  try {
    const result = await dataImport(migrate.importPath);
    if (result.ok) {
      migrate.result = result;
      migrate.confirmOpen = false;
      message.value = "数据包已校验并展开：重新打开应用后生效";
    } else {
      migrate.error = `导入失败：${result.error}`;
    }
  } catch (err) {
    migrate.error = `导入失败：${String(err)}`;
  } finally {
    migrate.busy = false;
  }
}

/** 立即重启（不装 plugin-process，Rust 侧自己 spawn 自己） */
async function doRelaunch() {
  migrate.relaunchMsg = "正在重启…";
  try {
    const result = await appRelaunch();
    if (!result.ok) migrate.relaunchMsg = result.error;
  } catch (err) {
    migrate.relaunchMsg = `自动重启失败：${String(err)}（请手动关闭后重新打开）`;
  }
}

async function refreshUis() {
  try {
    uisState.value = await uisStatus();
  } catch {
    uisState.value = null;
  }
}

async function doUisLogin() {
  if (uisBusy.value) return;
  uisBusy.value = true;
  uisMsg.value = "正在登录…";
  try {
    const result = await uisLogin(values["uis_username"] || "", uisPwd.value);
    if (result.ok) {
      uisMsg.value = `登录成功：${result.cookies} 个 cookie${
        result.has_castgc ? "（含 CASTGC）" : "（没有 CASTGC，可能只是部分服务可用）"
      }`;
      uisPwd.value = "";
    } else {
      uisMsg.value = result.error || "登录失败";
    }
  } catch (err) {
    uisMsg.value = String(err);
  } finally {
    uisBusy.value = false;
    await refreshUis();
  }
}

async function doUisLogout() {
  await uisLogout();
  uisMsg.value = "已退出 UIS";
  await refreshUis();
}

async function doCanvasProbe() {
  uisBusy.value = true;
  uisMsg.value = "正在诊断 Canvas…";
  try {
    const result = await canvasProbe();
    const current = result.current;
    uisMsg.value = result.ok
      ? `Canvas 可用：${result.courses} 门课程${
          current ? `，默认选「${current.name}」（${current.files} 个文件）` : ""
        }（${(result.names || []).join("、")}）`
      : String(result.error);
    if (result.ok) {
      // 诊断会把课程清单和默认课程写进设置，这里补回本地值，
      // 免得「下载课程文件」读到旧值 / 下拉框是空的
      const fresh = await settingsAll();
      for (const key of ["canvas_last_course", "canvas_course_list"]) {
        if (fresh.values[key] !== undefined) values[key] = fresh.values[key];
      }
    }
  } catch (err) {
    uisMsg.value = String(err);
  } finally {
    uisBusy.value = false;
  }
}

/** 诊断记下的课程清单（JSON），给下拉框用 */
const canvasCourses = computed<{ id: string; name: string }[]>(() => {
  try {
    const raw = values["canvas_course_list"] || "";
    if (!raw.trim()) return [];
    const rows = JSON.parse(raw);
    return Array.isArray(rows) ? rows.filter((one) => one && one.id) : [];
  } catch {
    return [];
  }
});

/** 换课程：立刻存进设置，「下载课程文件」用这个键 */
function pickCanvasCourse(event: Event) {
  const id = (event.target as HTMLSelectElement).value;
  values["canvas_last_course"] = id;
  settingsSave({ canvas_last_course: id });
}

async function doCanvasDownload() {
  const cid = values["canvas_last_course"] || "";
  if (!cid) {
    uisMsg.value = "先点「诊断 Canvas」，拿到课程后再下载";
    return;
  }
  uisBusy.value = true;
  uisMsg.value = "正在下载课程文件…";
  try {
    const result = await canvasDownload(cid, 40);
    uisMsg.value = result.ok
      ? `下载完成${result.course ? `（课程：${result.course}）` : ""}：新增 ${result.saved}、重名跳过 ${result.skipped}、失败 ${result.failed} → ${result.target}`
      : String(result.error);
  } catch (err) {
    uisMsg.value = String(err);
  } finally {
    uisBusy.value = false;
  }
}

// 整理旧下载文件：把以前平铺在下载目录里的文件搬进对应课程文件夹
async function doCanvasMigrate() {
  if (uisBusy.value) return;
  uisBusy.value = true;
  uisMsg.value = "正在整理旧下载文件…";
  try {
    const result = await canvasMigrate();
    if (!result.ok) {
      uisMsg.value = result.error || "整理失败";
      return;
    }
    const moved = result.moved || [];
    const deduped = result.deduped || [];
    const unresolved = result.unresolved || [];
    const failed = result.failed || [];
    let text = `迁移 ${moved.length} 个 · 去重 ${deduped.length} 个 · 未识别 ${unresolved.length} 个`;
    if (failed.length) text += ` · 失败 ${failed.length} 个`;
    if (unresolved.length) text += "（没有动这些文件，还留在下载目录根下）";
    // 一个都没搬的时候要说清为什么，否则用户只看到「未识别 N 个」
    if (!moved.length && !deduped.length && unresolved.length) {
      text += uisState.value?.has_session
        ? "。原因：Canvas 官方文件清单里对不上这些文件名（多半不是用本程序下载的）"
        : "。原因：既没有「文件名 → 课程」的索引，也没有 Canvas 会话（拉不到官方文件清单）——先点上面的「抓取会话」，再点这里";
    }
    if (failed.length) {
      text += `；失败：${failed.map(([name, why]) => `${name}（${why}）`).join("、")}`;
    }
    if (result.root) text += ` → ${result.root}`;
    uisMsg.value = text;
  } catch (err) {
    uisMsg.value = String(err);
  } finally {
    uisBusy.value = false;
  }
}

async function doJwglProbe() {
  uisBusy.value = true;
  uisMsg.value = "正在诊断教务接口…";
  try {
    // 学期号不再让用户填：留空交给后端从课表页里自动认
    const result = await jwglProbe("");
    if (!result.ok) {
      uisMsg.value = String(result.error);
      return;
    }
    const sem = result.semester
      ? `学期 ${result.semester}${result.semester_sniffed ? "（自动识别）" : ""} · `
      : "";
    const slots = typeof result.slots === "number" ? result.slots : 0;
    const sample = (result.sample_titles || []).join("、");
    const monday = result.first_monday
      ? `第一周周一 ${result.first_monday}${result.term_start ? `（教务写的学年起始：${result.term_start}）` : ""} · `
      : "";
    uisMsg.value = slots
      ? `教务接口可用：${sem}${monday}能解析出 ${slots} 个课表格子（${sample}…），点「导入课表」写进日历`
      : `教务接口可用：${sem}${monday}shape=${result.shape} bytes=${result.bytes} keys=${
          (result.top_keys || []).join(",") || "(无)"
        }，但没解析出课表格子`;
    // 自动识别出来的学期号 / 第一周周一记进设置（学期号界面上不显示，只给后台复用）
    if (result.semester) {
      values["jwgl_semester"] = result.semester;
      await setSetting("jwgl_semester", result.semester);
    }
    if (result.first_monday) {
      values["term_start"] = result.first_monday;
      await setSetting("term_start", result.first_monday);
    }
  } catch (err) {
    uisMsg.value = String(err);
  } finally {
    uisBusy.value = false;
  }
}

async function doJwglImport() {
  const semester = (values["jwgl_semester"] || "").trim();
  // 「学期第一周周一」不再是必填：留空时后端会用教务页面上的「学年起始日期」自动换算
  const termStart = (values["term_start"] || "").trim();
  uisBusy.value = true;
  uisMsg.value = termStart
    ? "正在导入课表…（先抓教务 print-data，再解析成日程）"
    : "正在导入课表…（学期起始日期会自动从教务读取）";
  try {
    const result = await jwglImport(termStart, semester);
    if (!result.ok) {
      uisMsg.value = `导入失败：${result.error}`;
      return;
    }
    if (result.term_start) {
      values["term_start"] = result.term_start;
      await setSetting("term_start", result.term_start);
    }
    const sem = result.semester
      ? `学期 ${result.semester}${result.semester_sniffed ? "（自动识别）" : ""}`
      : "";
    uisMsg.value = `导入完成：新增 ${result.added} 条，跳过 ${result.skipped} 条（${
      result.slots ?? 0
    } 个课表格子${sem ? " · " + sem : ""}）。一条都没新增的话，先点「教务诊断」看返回体结构`;
    if (result.semester) {
      values["jwgl_semester"] = result.semester;
      await setSetting("jwgl_semester", result.semester);
    }
  } catch (err) {
    uisMsg.value = String(err);
  } finally {
    uisBusy.value = false;
  }
}

async function doUisOpen() {
  const result = await uisOpenLogin();
  if (!result.ok) {
    uisMsg.value = String(result.error);
    return;
  }
  uisMsg.value = "已打开登录页：学号密码会自动填好并点「登录」；登录成功后会话会自动保存…";
  // 登录成功后是后端自动抓会话的，这里轮询一下状态把结果告诉用户
  for (let i = 0; i < 60; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 2000));
    await refreshUis();
    const state = uisState.value;
    if (state && state.has_session) {
      uisMsg.value = `会话已保存：${state.cookies} 个 cookie${
        state.has_service_session ? "（含业务站会话）" : "（还没拿到业务站会话，可点「重新抓取会话」）"
      }`;
      return;
    }
  }
  uisMsg.value =
    "还没拿到会话：确认已在窗口里完成登录（有二次验证要过一次）；" +
    "实在不行点「重新抓取会话」手动兜底";
}

/** 「检查自动填充」：只报告账号、密码、开关与开关状态，不联网（Python 版 `uis_login.test` 的等价） */
async function doUisCheck() {
  await refreshUis();
  const state = uisState.value;
  if (!state) {
    uisMsg.value = "读不到统一身份认证的状态";
    return;
  }
  const username = state.username || "未填";
  const password = state.has_saved_password ? "已保存" : "未保存";
  const autofill = state.remember_password ? "开" : "关";
  const session = state.has_session ? `已就绪（${state.cookies} 个 cookie）` : "未取得";
  uisMsg.value = `学号 ${username} · 密码 ${password} · 自动填表 ${autofill} · 会话 ${session}` +
    (autofill === "关" ? "；打开「网页自动填表并点登录」才会自动登录" : "");
}

async function doUisCapture() {
  uisBusy.value = true;
  try {
    const result = await uisCapture();
    uisMsg.value = result.ok
      ? `已抓取 ${result.cookies} 个 cookie${result.verdict ? ` · ${result.verdict}` : ""}：${(result.names || []).join(", ")}`
      : String(result.error);
  } catch (err) {
    uisMsg.value = String(err);
  } finally {
    uisBusy.value = false;
    await refreshUis();
  }
}

function loadTools() {
  const map: Record<string, boolean> = {};
  for (const tool of AI_TOOLS) map[tool.name] = toolEnabled(values, tool.name);
  toolsOn.value = map;
  toolsGlobal.value = toolsGloballyOn(values);
}

function toggleTool(name: string) {
  const next = !toolsOn.value[name];
  toolsOn.value = { ...toolsOn.value, [name]: next };
  values[toolKey(name)] = next ? "1" : "0";
  settingsSave({ [toolKey(name)]: next ? "1" : "0" });
}

function setToolsGlobal(event: Event) {
  const on = (event.target as HTMLInputElement).checked;
  toolsGlobal.value = on;
  values[TOOLS_ENABLED_KEY] = on ? "1" : "0";
  settingsSave({ [TOOLS_ENABLED_KEY]: on ? "1" : "0" });
}

function setGroup(group: string, on: boolean) {
  const patch: Record<string, string> = {};
  for (const tool of AI_TOOLS.filter((one) => one.group === group)) {
    toolsOn.value = { ...toolsOn.value, [tool.name]: on };
    values[toolKey(tool.name)] = on ? "1" : "0";
    patch[toolKey(tool.name)] = on ? "1" : "0";
  }
  settingsSave(patch);
}

function selectProfile(profile: AiProfile) {
  values["ai_profile"] = String(profile.id);
  aiEdit.name = profile.name;
  aiEdit.base_url = profile.base_url;
  aiEdit.model = profile.model;
  refreshAi();
}

function newProfile() {
  aiEdit.name = "新配置";
  aiEdit.base_url = "https://api.deepseek.com/v1";
  aiEdit.model = "deepseek-chat";
}

async function saveProfile() {
  const id = Number(values["ai_profile"] || 0);
  const saved = await aiProfileSave(id, aiEdit.name, aiEdit.base_url, aiEdit.model);
  values["ai_profile"] = String(saved);
  await refreshAi();
  message.value = "AI 配置已保存";
}

async function removeProfile() {
  const id = Number(values["ai_profile"] || 0);
  if (!id) return;
  await aiProfileDelete(id);
  values["ai_profile"] = "";
  aiEdit.name = "";
  aiEdit.base_url = "";
  aiEdit.model = "";
  await refreshAi();
  message.value = "这套配置已删除";
}

const cards: CardDef[] = [
  {
    key: "appearance",
    title: "外观",
    subtitle: "浅色 / 深色、强调色与窗口通透度 —— 改一下这里就立刻生效，并且自动保存",
    category: "常规",
    fields: [],
  },
  {
    key: "dirs",
    title: "数据目录",
    subtitle: "数据库、作业下载、LaTeX 编译与公式缓存放哪儿",
    category: "常规",
    fields: [
      {
        label: "下载目录",
        kind: "text",
        key: "dir_downloads",
        hint: "留空 = 数据目录下的 elearning；下载时每门课会再建一个以课程名命名的子文件夹",
      },
      { label: "LaTeX 编译目录", kind: "text", key: "dir_latex_build" },
      { label: "公式缓存目录", kind: "text", key: "dir_latex_math" },
    ],
    actions: ["save"],
  },
  {
    key: "data",
    title: "数据与关于",
    subtitle: "备份数据库、看数据都在哪、当前版本",
    category: "其他",
    fields: [],
    actions: [],
  },
  {
    key: "migration",
    title: "数据迁移",
    subtitle:
      "导出 .dlpk 数据包（换电脑 / 重装系统 / 与 Python 版互相搬家），或从数据包导入",
    category: "其他",
    fields: [],
    actions: [],
  },  {
    key: "tray",
    title: "托盘与启动",
    subtitle: "关闭窗口时收进系统托盘",
    category: "常规",
    fields: [
      { label: "关闭窗口时收进托盘", kind: "switch", key: "tray_close" },
      { label: "启动时最小化到托盘", kind: "switch", key: "tray_start_min" },
      { label: "双击托盘图标", kind: "select", key: "tray_double", options: ["显示主窗口", "新建日程", "不响应"] },
      { label: "开机自动启动", kind: "switch", key: "autostart" },
    ],
    actions: ["save"],
  },
  {
    key: "dashsrc",
    title: "eHall 数据来源",
    subtitle: "每张卡的数据地址填一次（在 eHall 里打开对应服务，复制地址栏）",
    category: "仪表板",
    fields: [
      { label: "校园卡余额", kind: "text", key: "ehall_json_card", placeholder: "https://ecard.fudan.edu.cn/…" },
      { label: "校车安排", kind: "text", key: "ehall_json_bus" },
      { label: "体育刷锻", kind: "text", key: "ehall_json_sports" },
      { label: "图书馆人数", kind: "text", key: "ehall_json_library_people" },
      { label: "我的 ehall 通知", kind: "text", key: "ehall_json_ehall_todo" },
      { label: "空教室查询", kind: "text", key: "ehall_json_empty_room" },
      { label: "教务通知（通知页抓取来源）", kind: "text", key: "dashsrc:notice" },
    ],
    actions: ["save"],
  },
  {
    key: "smtp",
    title: "发件邮箱（SMTP）",
    subtitle: "汇报邮件与普通邮件都从这里发",
    category: "邮件",
    fields: [
      { label: "服务器", kind: "text", key: "smtp_host", placeholder: "smtp.qq.com" },
      { label: "端口", kind: "text", key: "smtp_port", placeholder: "465" },
      { label: "加密方式", kind: "select", key: "smtp_enc", options: ["SSL/TLS", "STARTTLS", "不加密"] },
      { label: "账号", kind: "text", key: "smtp_user" },
      { label: "密码 / 授权码", kind: "password", secret: "smtp_password", placeholder: "已保存（不可查看），留空表示不改" },
      { label: "发件邮箱", kind: "text", key: "smtp_from" },
      { label: "发件人显示名", kind: "text", key: "smtp_name", placeholder: "Helena Lee" },
      { label: "单封最大附件（MB）", kind: "text", key: "smtp_max_mb", placeholder: "20" },
      { label: "发送失败自动重试", kind: "switch", key: "smtp_retry" },
    ],
    actions: ["save", "smtp-test"],
  },
  {
    key: "imap",
    title: "收件箱（IMAP）",
    subtitle: "支持多个账户；邮件正文图片会缓存到本地",
    category: "邮件",
    fields: [
      { label: "服务器", kind: "text", key: "imap_host", placeholder: "imap.qq.com" },
      { label: "端口", kind: "text", key: "imap_port", placeholder: "993" },
      { label: "账号", kind: "text", key: "imap_user" },
      { label: "授权码", kind: "password", secret: "imap_password", placeholder: "已保存（不可查看）" },
      { label: "每个账户最多收几封", kind: "text", key: "imap_max", placeholder: "50" },
      { label: "自动收取间隔（分钟，0=不自动）", kind: "text", key: "imap_interval", placeholder: "5" },
      { label: "启动时自动收一次", kind: "switch", key: "imap_on_start" },
      { label: "只收未读", kind: "switch", key: "imap_unread_only" },
    ],
    actions: ["save", "imap-test"],
  },
  {
    key: "mascot",
    title: "看板娘（梅子）",
    subtitle: "桌面上那个人物：大小可以调；文件拖到她身上就发给 AI",
    category: "其他",
    fields: [
      {
        label: "大小（百分比）",
        kind: "select",
        key: "mascot_scale",
        options: ["70", "85", "100", "115", "130", "150", "180"],
        hint: "默认 100（约 250×430）；改完点下面的「立即应用大小」，开着的小窗会马上缩放",
      },
    ],
    actions: ["save"],
  },
  {
    key: "mailwatch",
    title: "盯邮箱（只盯指定发件人）",
    subtitle:
      "只处理这几个发件人的新邮件：把里面的待办抽出来，等你在右上角卡片里确认后再加进日程（对应旧版 mail_watch）",
    category: "邮件",
    fields: [
      {
        label: "开启盯邮箱",
        kind: "switch",
        key: "mail_watch_enabled",
        hint: "开着才会每隔几分钟看一次；发现候选会弹系统通知",
      },
      {
        label: "发件人关键词",
        kind: "text",
        key: "mail_watch_senders",
        placeholder: "教务处, 辅导员, noreply@fudan.edu.cn（逗号分隔，匹配名字或地址）",
      },
      { label: "多久看一次（分钟，2–180）", kind: "text", key: "mail_watch_minutes", placeholder: "10" },
    ],
    actions: ["save"],
  },
  {
    key: "recipient",
    title: "收件人",
    subtitle: "汇报邮件的默认收件人",
    category: "邮件",
    fields: [
      { label: "默认收件人", kind: "text", key: "smtp_default_to" },
      { label: "默认抄送", kind: "text", key: "smtp_cc" },
      { label: "默认密送", kind: "text", key: "smtp_bcc" },
      { label: "汇报邮件主题模板", kind: "text", key: "report_subject_tpl", placeholder: "{name} 的工作汇报（{date}）" },
      { label: "汇报末尾加上签名", kind: "switch", key: "report_sign" },
    ],
    actions: ["save"],
  },
  {
    key: "groups",
    title: "联系人分组",
    subtitle: "常用收件人分组（在发件箱里选）",
    category: "邮件",
    fields: [
      { label: "分组名", kind: "text", key: "contact_group" },
      { label: "新建日程默认分组", kind: "text", key: "default_event_group" },
      { label: "分组颜色", kind: "select", key: "group_color", options: ["violet", "teal", "amber", "rose", "sky"] },
    ],
    actions: ["save"],
  },
  {
    key: "timetable",
    title: "课表",
    subtitle: "从教务直接导入课表：登录一次后，学期、第一周周一、节次时间都自动读出来",
    category: "日程",
    fields: [
      // 与 Python 版一致：这一项只是「显示 / 兜底」，正常情况下由教务页面上的「学年起始日期」自动换算
      { label: "学期第一周周一（自动识别，可改）", kind: "text", key: "term_start", placeholder: "留空即可，导入时自动读" },
    ],
    actions: ["save"],
  },
  {
    key: "reminder",
    title: "提醒",
    subtitle: "桌面提醒开关、提示音、过期自动完成",
    category: "日程",
    fields: [
      { label: "桌面提醒", kind: "switch", key: "reminder_on" },
      { label: "提示音", kind: "switch", key: "reminder_sound" },
      { label: "过期自动完成", kind: "switch", key: "reminder_auto_done" },
      { label: "默认提前提醒（分钟）", kind: "text", key: "remind_default", placeholder: "15" },
      { label: "免打扰开始", kind: "text", key: "remind_quiet_from", placeholder: "23:30" },
      { label: "免打扰结束", kind: "text", key: "remind_quiet_to", placeholder: "07:30" },
      { label: "开机后补提醒（小时）", kind: "text", key: "remind_catchup", placeholder: "12" },
      { label: "提醒卡片停在右下角", kind: "switch", key: "remind_pin_corner" },
    ],
    actions: ["save"],
  },
  {
    key: "location",
    title: "定位与出发提醒",
    subtitle: "按地点算步行 / 骑行时间，上课前提示该出发了",
    category: "日程",
    fields: [
      { label: "主要出行方式", kind: "select", key: "travel_mode", options: ["都看", "步行", "骑行"] },
      { label: "步行速度（米/分）", kind: "text", key: "walk_speed", placeholder: "80" },
      { label: "骑行速度（米/分）", kind: "text", key: "bike_speed", placeholder: "250" },
      { label: "提前缓冲（分钟）", kind: "text", key: "travel_buffer", placeholder: "10" },
      { label: "常用地点：宿舍", kind: "text", key: "place_dorm" },
      { label: "常用地点：食堂", kind: "text", key: "place_canteen" },
      { label: "常用地点：图书馆", kind: "text", key: "place_library" },
      { label: "定位来源", kind: "select", key: "loc_source", options: ["常用地点优先", "IP 定位", "只用常用地点"] },
    ],
    actions: ["save"],
  },
  {
    key: "focus",
    title: "专注",
    subtitle: "桌面悬浮计时器与离开提醒",
    category: "日程",
    fields: [
      { label: "专注时长（分钟）", kind: "text", key: "focus_minutes", placeholder: "25" },
      { label: "休息时长（分钟）", kind: "text", key: "focus_break", placeholder: "5" },
      { label: "离开多久提醒（分钟）", kind: "text", key: "focus_away", placeholder: "10" },
      { label: "悬浮计时器位置", kind: "select", key: "focus_pos", options: ["右下", "右上", "左下", "左上"] },
      { label: "结束提示音", kind: "switch", key: "focus_sound" },
    ],
    actions: ["save"],
  },
  {
    key: "llm",
    title: "AI（API 服务）",
    subtitle: "多套配置；Key 加密保存，设置里只能修改、不能查看",
    category: "AI",
    fields: [
      { label: "这套配置的 API Key", kind: "password", secret: "__profile_key__", placeholder: "已保存（不可查看）" },
    ],
    actions: ["save", "ai-refresh"],
  },
  {
    key: "webui",
    title: "网页版面板",
    subtitle: "在右栏 AI 标签里嵌一个模型网页版（和网页版对话，不新开窗口）；地址留空就不显示",
    category: "AI",
    fields: [
      {
        label: "网页版地址",
        kind: "text",
        key: "ai_web_url",
        placeholder: "https://chat.deepseek.com/",
        hint:
          "留空 = 右栏不出现「网页版」子标签。登录态由浏览器缓存决定（应用不保存、也不上传你的账号）。" +
          "很多站点会用 X-Frame-Options / CSP 禁止被别的页面嵌入，那种只能点面板上的「在系统浏览器打开」。",
      },
    ],
    actions: ["save"],
  },
  {
    key: "prompts",
    title: "系统提示词",
    subtitle: "AI 的人设与规则",
    category: "AI",
    fields: [{ label: "系统提示词", kind: "textarea", key: "system_prompt" }],
    actions: ["save"],
  },
  {
    key: "memory",
    title: "AI 记忆",
    subtitle: "让 AI 长期记住的事：每次对话自动带上（最多 4000 字）；跟它说「记住……」它自己会写",
    category: "AI",
    fields: [],
  },
  {
    key: "contacts",
    title: "通讯录",
    subtitle: "学生工作用：姓名 / 职位 / 邮箱 / 手机号 / 微信备注名 / QQ / 其它联系方式（AI 读取需单独授权）",
    category: "其他",
    fields: [],
  },
  {
    key: "account",
    title: "账户与登录",
    subtitle: "DanLume 账号（自家服务器：登录 / 注册 / 设备管理）与本机账户、管理员权限",
    category: "其他",
    fields: [
      { label: "当前账户", kind: "note" },
      { label: "记住这台设备", kind: "switch", key: "remember_device" },
      { label: "登录有效期（天）", kind: "text", key: "session_days", placeholder: "30" },
      { label: "到期后要重新输密码", kind: "switch", key: "relogin_on_expire" },
      { label: "修改密码：旧密码", kind: "password", secret: "__old_password__" },
      { label: "修改密码：新密码", kind: "password", secret: "__new_password__" },
    ],
    actions: ["save"],
  },
  {
    key: "uis",
    title: "统一身份认证",
    subtitle: "内嵌网页遇到登录页时自动填好学号密码，你点一次「登录」即可",
    category: "其他",
    fields: [
      { label: "学号 / 工号", kind: "text", key: "uis_username", placeholder: "如 26303050178" },
      { label: "密码（DPAPI 加密，只能改不能看）", kind: "password", secret: "uis_password" },
      {
        label: "网页自动填表并点登录",
        kind: "switch",
        key: "uis_autofill",
        hint: "打开 eHall / 作业页 / 教务时，如果跳到统一身份认证会自动填好",
      },
    ],
    actions: ["save"],
  },
  {
    key: "elearn",
    title: "eLearning / Canvas",
    subtitle: "诊断课程接口、按课程下载文件，把以前平铺在下载目录里的旧文件整理进课程文件夹",
    category: "其他",
    fields: [],
  },
  {
    key: "vpn",
    title: "VPN / 代理",
    subtitle: "校外访问走代理时填这里",
    category: "其他",
    fields: [
      { label: "启用代理", kind: "switch", key: "proxy_enabled", hint: "开启后后台请求立刻生效；内嵌网页要重启一次" },
      { label: "代理类型（http / socks5）", kind: "select", key: "proxy_kind", options: ["http", "socks5"] },
      { label: "代理地址", kind: "text", key: "proxy_host", placeholder: "127.0.0.1" },
      { label: "代理端口", kind: "text", key: "proxy_port", placeholder: "7890" },
      { label: "认证方式", kind: "select", key: "proxy_auth", options: ["账号密码", "免认证"] },
      { label: "账号", kind: "text", key: "proxy_user" },
      { label: "密码", kind: "password", secret: "proxy_password" },
      { label: "不走代理的域名", kind: "textarea", key: "proxy_bypass", placeholder: "fudan.edu.cn, localhost（本机永远直连）" },
    ],
    actions: ["save"],
  },
  {
    key: "refs",
    title: "文献库引用",
    subtitle: "引用格式与自动概述",
    category: "其他",
    fields: [
      { label: "引用格式", kind: "select", key: "ref_style", options: ["GB/T 7714", "APA", "IEEE"] },
      { label: "自动生成中文概述", kind: "switch", key: "lit_auto_summary" },
      { label: "文献库目录", kind: "text", key: "lit_dir" },
      { label: "EndNote 导出编码", kind: "select", key: "lit_endnote_enc", options: ["UTF-8", "GBK"] },
      { label: "打开 PDF 用", kind: "select", key: "lit_pdf_open", options: ["内置预览", "系统默认程序"] },
      { label: "记录 DOI", kind: "switch", key: "lit_use_doi" },
    ],
    actions: ["save"],
  },
  {
    key: "notice",
    title: "通知",
    subtitle: "抓取教务 / eHall 通知的频率",
    category: "其他",
    fields: [
      { label: "开启通知监控", kind: "switch", key: "notice_watch" },
      { label: "多久抓一次（分钟）", kind: "text", key: "notice_interval", placeholder: "60" },
      { label: "只看教务通知", kind: "switch", key: "notice_only_edu" },
      { label: "静默时段开始", kind: "text", key: "notice_quiet_from", placeholder: "23:00" },
      { label: "静默时段结束", kind: "text", key: "notice_quiet_to", placeholder: "07:00" },
      { label: "新通知弹提示", kind: "switch", key: "notice_toast" },
    ],
    actions: ["save"],
  },
];

const visible = computed(() => {
  const text = keyword.value.trim().toLowerCase();
  if (!text) return cards.filter((card) => card.category === active.value);
  return cards.filter((card) => {
    const haystack = [card.title, card.subtitle, ...card.fields.map((f) => `${f.label} ${f.hint || ""}`)]
      .join(" ")
      .toLowerCase();
    return haystack.includes(text);
  });
});

const modeLabel: Record<string, string> = { light: "浅色", dark: "深色", auto: "自动" };

function setValue(field: Field, next: string) {
  if (field.key) values[field.key] = next;
  // 用户自己改了「学期第一周周一」就不再算自动值：下次导入让教务页面的日期说了算
  if (field.key === "term_start") {
    values["term_start_auto"] = "0";
    settingsSave({ term_start_auto: "0" });
  }
}

async function load() {
  const result = await settingsAll();
  for (const [key, value] of Object.entries(result.values)) values[key] = value;
  secrets.value = result.secrets;
  await refreshAi();
}

async function refreshAi() {
  aiList.value = await aiProfiles();
  aiNow.value = await aiState();
  if (!values["ai_profile"] && aiNow.value?.id) values["ai_profile"] = String(aiNow.value.id);
  loadTools();
  await refreshUis();
  const current = aiList.value.find((one) => String(one.id) === values["ai_profile"]);
  if (current && !aiEdit.name) {
    aiEdit.name = current.name;
    aiEdit.base_url = current.base_url;
    aiEdit.model = current.model;
  }
}

async function save(card: CardDef) {
  const patch: Record<string, string> = {};
  for (const field of card.fields) {
    if (field.key) patch[field.key] = values[field.key] ?? "";
  }
  if (card.key === "llm" && values["ai_profile"]) patch["ai_profile"] = values["ai_profile"];
  try {
    const count = Object.keys(patch).length ? await settingsSave(patch) : 0;
    let secretNote = "";
    for (const field of card.fields) {
      if (!field.secret) continue;
      const draftValue = (drafts[field.secret] || "").trim();
      if (!draftValue) continue;
      const key =
        field.secret === "__profile_key__"
          ? `llm_profile_key_${values["ai_profile"] || aiNow.value?.id || 1}`
          : field.secret;
      await saveSecret(key, draftValue);
      drafts[field.secret] = "";
      secretNote = "（含密码）";
    }
    message.value = `${card.title}：已保存 ${count} 项${secretNote}`;
    await refreshAi();
  } catch (err) {
    message.value = `保存失败：${String(err)}`;
  }
}

async function testSmtp() {
  const result = await mailTest();
  message.value = result.ok ? "发件配置齐全，可以发信" : result.error;
}

async function testImap() {
  const result = await imapTest();
  message.value = result.ok ? `收件配置齐全：${result.server} · ${result.user}` : result.error;
}

/** 服务器用不了时的兜底：跳到老的「邮件申请 + 管理员签发」流程（AdminPanel 一个字没删） */
function openMailApply() {
  adminOpen.value = true;
  message.value = "已切到邮件申请：用管理员公钥加密后发给管理员，批准后会回验证码 + 回执";
}

function chooseProfile(id: number) {
  values["ai_profile"] = String(id);
  refreshAi();
}

async function persistTheme() {
  try {
    await settingsSave({
      theme_mode: themeState.mode,
      theme_accent: themeState.accent,
      theme_bg: themeState.bg,
      theme_alpha: String(themeState.alpha / 100),
      theme_blur: String(themeState.blur),
      theme_glass: themeState.glass ? "1" : "0",
      theme_browser: themeState.browser,
    });
  } catch {
    // 写不进设置也不影响界面
  }
}

// ------------------------------------------------------------ AI 记忆库
async function refreshMemories() {
  memoryError.value = "";
  try {
    const keyword = memoryKeyword.value.trim();
    const result = keyword ? await memorySearch(keyword) : await memoryList(200);
    memories.value = result.items;
    memoryTotal.value = result.total;
  } catch (err) {
    memoryError.value = `读取记忆失败：${String(err)}`;
  }
}

async function addMemory() {
  const text = memoryDraft.value.trim();
  if (!text) return;
  memoryError.value = "";
  try {
    const result = await memoryAdd(text, "手动", "");
    if (!result.ok) {
      memoryError.value = result.error || "写不进去";
      return;
    }
    memoryDraft.value = "";
    memoryMsg.value = result.message || "已记下";
    await refreshMemories();
  } catch (err) {
    memoryError.value = `写入失败：${String(err)}`;
  }
}

function startEditMemory(row: MemoryRow) {
  memoryEditing.id = row.id;
  memoryEditing.content = row.content;
  memoryEditing.tags = row.tags;
  memoryMsg.value = "";
}

async function saveMemoryEdit() {
  const text = memoryEditing.content.trim();
  if (!text) {
    memoryError.value = "内容不能为空";
    return;
  }
  memoryError.value = "";
  try {
    const result = await memoryUpdate(memoryEditing.id, text, memoryEditing.tags);
    if (!result.ok) {
      memoryError.value = result.error || "改不动";
      return;
    }
    memoryEditing.id = 0;
    memoryMsg.value = "已保存";
    await refreshMemories();
  } catch (err) {
    memoryError.value = `保存失败：${String(err)}`;
  }
}

async function removeMemory(row: MemoryRow) {
  memoryError.value = "";
  try {
    const result = await memoryDelete(row.id);
    if (!result.ok) {
      memoryError.value = result.error || "删不掉";
      return;
    }
    if (memoryEditing.id === row.id) memoryEditing.id = 0;
    memoryMsg.value = "已删除";
    await refreshMemories();
  } catch (err) {
    memoryError.value = `删除失败：${String(err)}`;
  }
}

async function clearMemories() {
  memoryError.value = "";
  try {
    const result = await memoryClear();
    if (!result.ok) {
      memoryError.value = result.error || "清不空";
      return;
    }
    memoryEditing.id = 0;
    memoryMsg.value = `已清空 ${result.removed ?? 0} 条记忆`;
    await refreshMemories();
  } catch (err) {
    memoryError.value = `清空失败：${String(err)}`;
  }
}

async function doBackup() {
  const result = await backupDb();
  message.value = result.ok
    ? `已备份：${result.path}（${Math.round(result.bytes / 1024)} KB）`
    : result.error;
}

onMounted(async () => {
  await load();
  await refreshMemories();
  info.value = await dataInfo();
  void refreshAdminFlag();
});

/**
 * 是不是管理员：只看**服务器账号**的角色（旧版那套本机「证书签发」早就名存实亡）。
 * 普通用户不该看到「账户审核（管理员）」这个入口（用户 2026-11-11 提的）。
 */
const isServerAdmin = ref(false);

async function refreshAdminFlag() {
  try {
    const state = await accountState();
    isServerAdmin.value = !!state.is_admin;
  } catch {
    isServerAdmin.value = false;
  }
}
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">设置</h1>
        <div class="muted">邮件服务器、常用收件人与提醒偏好，全部保存在本机</div>
      </div>
      <div class="spacer" />
      <span v-if="message" class="tiny">{{ message }}</span>
      <SoftButton :height="32" :font-size="12.5" glyph="REFRESH" @click="load">重新读取</SoftButton>
    </div>

    <div class="body">
      <aside class="cats">
        <input v-model="keyword" type="search" placeholder="搜索设置…" />
        <button
          v-for="cat in CATEGORIES"
          :key="cat"
          class="cat"
          :class="{ on: active === cat && !keyword }"
          @click="(active = cat), (keyword = '')"
        >
          {{ cat }}
        </button>
      </aside>

      <div class="cards">
        <Card v-for="card in visible" :key="card.key" :padding="16" :spacing="10" class="setting-card">
          <div class="card-head">
            <IconBadge :glyph="card.key === 'appearance' ? 'PICTURE' : 'SETTINGS'" tone="teal" :size="28" />
            <div class="head-text">
              <div class="title">{{ card.title }}</div>
              <div class="subtitle">{{ card.subtitle }}</div>
            </div>
          </div>

          <template v-if="card.key === 'account'">
            <AccountPanel @mail-apply="openMailApply" />
          </template>

          <template v-if="card.key === 'contacts'">
            <ContactsPanel />
          </template>

          <template v-if="card.key === 'data'">
            <div class="field">
              <div class="label">数据目录</div>
              <div class="path-box" @click="info && openPath(info.data_dir)">
                {{ info?.data_dir || "读取中…" }}
              </div>
            </div>
            <div class="field">
              <div class="label">数据库</div>
              <div class="hint">
                {{ (info?.db_size || 0) / 1024 / 1024 | 0 }} MB ·
                日程 {{ info?.counts.events ?? 0 }} · 备忘 {{ info?.counts.memos ?? 0 }} ·
                通知 {{ info?.counts.notices ?? 0 }} · 文献 {{ info?.counts.papers ?? 0 }} ·
                会话 {{ info?.counts.chat_sessions ?? 0 }}
              </div>
            </div>
            <div class="field">
              <div class="label">备份</div>
              <div class="row-actions">
                <SoftButton :height="30" :font-size="12.5" @click="doBackup">
                  备份数据库
                </SoftButton>
                <SoftButton :height="30" :font-size="12.5" @click="info && openPath(info.data_dir)">
                  打开数据目录
                </SoftButton>
              </div>
              <div class="hint">备份会存到 data/backups/lumina-时间.db（不改动原库）</div>
            </div>
            <div class="field">
              <div class="label">关于</div>
              <div class="hint">
                DanLume {{ info?.version || "" }} · {{ info?.kernel || "" }} ·
                界面与功能对齐 PySide6 版，数据共用同一份 lumina.db
              </div>
            </div>
          </template>

          <template v-if="card.key === 'migration'">
            <div class="field">
              <div class="label">导出数据包</div>
              <div class="row-actions">
                <SoftButton
                  :height="30"
                  :font-size="12.5"
                  :disabled="migrate.busy"
                  @click="doExportPackage"
                >
                  导出数据包…
                </SoftButton>
              </div>
              <div
                v-if="migrate.exportPath"
                class="path-box"
                @click="revealPath(migrate.exportPath)"
              >
                {{ migrate.exportPath }}（{{ formatBytes(migrate.exportBytes) }}）
              </div>
              <div v-if="migrate.exportPath" class="hint">
                包里：{{ tableSummary(migrate.exportTables) }}
              </div>
              <div class="hint">
                一个 .dlpk 文件（其实是 zip）= manifest.json + lumina.db + README.txt。
                库里是用 VACUUM INTO 打出来的一致性快照，导出时正在写也不怕；
                口令类设置（DPAPI 密文）不会导出，换机器后要重新填。
              </div>
            </div>

            <div class="field">
              <div class="label">导入数据包</div>
              <div class="row-actions">
                <SoftButton
                  :height="30"
                  :font-size="12.5"
                  :disabled="migrate.busy"
                  @click="chooseImportPackage"
                >
                  从数据包导入…
                </SoftButton>
                <SoftButton
                  v-if="migrate.result"
                  variant="primary"
                  :height="30"
                  :font-size="12.5"
                  @click="doRelaunch"
                >
                  立即重启
                </SoftButton>
              </div>
              <div v-if="migrate.result" class="hint">
                已备份到 {{ migrate.result.backup }}，重新打开应用后生效。
              </div>
              <div v-if="migrate.result" class="hint">
                包里：{{ tableSummary(migrate.result.tables) }} · 来自
                {{ migrate.result.app || "?" }} 版 {{ migrate.result.app_version || "" }}
                <template v-if="migrate.result.stripped_keys.length">
                  · 未导出的密钥：{{ migrate.result.stripped_keys.join("、") }}
                </template>
              </div>
              <div v-if="migrate.error" class="err">{{ migrate.error }}</div>
              <div v-if="migrate.relaunchMsg" class="hint">{{ migrate.relaunchMsg }}</div>
              <div v-if="values['pending_import'] === '1' && !migrate.result" class="hint">
                有数据包正在等着换库（来源：{{ values["pending_import_from"] || "未知" }}）：
                重新打开应用后才会生效。
              </div>
              <div class="hint">
                导入不会直接覆盖正在用的库：先自动备份到 data/backups/pre-import-时间.db，
                再把包里的库解成 data/lumina.db.imported，重新打开应用时才换库。
                口令类设置（SMTP / IMAP 授权码、AI Key、UIS 口令、服务器登录）导入后要重新填。
              </div>
            </div>
          </template>

          <template v-if="card.key === 'appearance'">
            <div class="field">
              <div class="label">显示模式</div>
              <div class="switch-row">
                <button
                  v-for="mode in ['light', 'dark', 'auto'] as const"
                  :key="mode"
                  class="mode-btn"
                  :class="{ on: themeState.mode === mode }"
                  @click="setMode(mode), persistTheme()"
                >
                  {{ modeLabel[mode] }}
                </button>
              </div>
              <div class="hint">「自动」会跟着 Windows 的浅色 / 深色设置走</div>
            </div>

            <div class="field">
              <div class="label">强调色</div>
              <div class="swatches">
                <button
                  v-for="accent in ACCENTS"
                  :key="accent.value"
                  class="swatch"
                  :class="{ on: themeState.accent.toLowerCase() === accent.value.toLowerCase() }"
                  :style="{ background: accent.value }"
                  :title="accent.name"
                  @click="setAccent(accent.value), persistTheme()"
                />
                <label class="swatch-pick" title="自定义颜色">
                  <input
                    type="color"
                    :value="themeState.accent"
                    @input="setAccent(($event.target as HTMLInputElement).value), persistTheme()"
                  />
                </label>
              </div>
            </div>

            <div class="field">
              <div class="label">浏览器风格</div>
              <div class="switch-row">
                <button
                  v-for="one in [
                    { key: 'follow', name: '跟随应用' },
                    { key: 'light', name: '浅色' },
                    { key: 'dark', name: '深色' },
                  ]"
                  :key="one.key"
                  class="mode-btn"
                  :class="{ on: themeState.browser === one.key }"
                  @click="setBrowserStyle(one.key as 'follow' | 'light' | 'dark'), persistTheme()"
                >
                  {{ one.name }}
                </button>
              </div>
              <div class="hint">内嵌网页（作业 / eHall 等）用浅色还是深色</div>
            </div>

            <div class="field">
              <div class="label">背景色调</div>
              <div class="tones">
                <button
                  v-for="tone in BG_TONES"
                  :key="tone.key"
                  class="tone"
                  :class="{ on: themeState.bg === tone.key }"
                  @click="setBg(tone.key), persistTheme()"
                >
                  <span class="tone-chip" :style="{ background: tone.mid }" />
                  {{ tone.name }}
                </button>
              </div>
              <div class="hint">深色模式统一用 Apple 深色背景；这里的色调在浅色模式下生效</div>
            </div>

            <div class="field">
              <div class="label">窗口不透明度：{{ themeState.alpha }}%</div>
              <input
                type="range"
                min="55"
                max="100"
                :value="themeState.alpha"
                @input="setAlpha(Number(($event.target as HTMLInputElement).value)), persistTheme()"
              />
              <div class="hint">100% 完全不透明；越低背后越透</div>
            </div>

            <div class="field">
              <div class="label">背景模糊度：{{ themeState.blur }} px</div>
              <input
                type="range"
                min="0"
                max="60"
                :value="themeState.blur"
                @input="setBlur(Number(($event.target as HTMLInputElement).value)), persistTheme()"
              />
              <div class="hint">0 表示不模糊（只保留透明度），建议 16–32</div>
            </div>

            <div class="field inline">
              <div class="label">磨砂玻璃</div>
              <label class="switch">
                <input
                  type="checkbox"
                  :checked="themeState.glass"
                  @change="setGlass(($event.target as HTMLInputElement).checked), persistTheme()"
                />
                <span />
              </label>
            </div>

            <div class="field inline">
              <div class="label">浏览器多标签页</div>
              <label class="switch">
                <input
                  type="checkbox"
                  :checked="themeState.browserTabs"
                  @change="toggleBrowserTabs(($event.target as HTMLInputElement).checked)"
                />
                <span />
              </label>
            </div>

            <div class="field inline">
              <div class="label">半透明窗口（高级）</div>
              <label class="switch">
                <input
                  type="checkbox"
                  :checked="themeState.translucent"
                  @change="setTranslucent(($event.target as HTMLInputElement).checked), persistTheme()"
                />
                <span />
              </label>
            </div>
            <div class="hint">改这里的所有东西都会马上生效，并且自动保存（不用点「保存」）</div>
          </template>

          <template v-else-if="card.key === 'llm'">
            <div class="field">
              <div class="label">配置列表（存旧库 llm_profiles）</div>
              <div class="profile-list">
                <button
                  v-for="profile in aiList"
                  :key="profile.id"
                  class="profile"
                  :class="{ on: String(profile.id) === values['ai_profile'] }"
                  @click="selectProfile(profile)"
                >
                  <span class="pname">{{ profile.name || `配置 ${profile.id}` }}</span>
                  <span class="pmodel">{{ profile.model }}</span>
                  <Pill
                    :text="profile.has_key ? '已填 Key' : '没填 Key'"
                    :color="profile.has_key ? 'var(--success)' : 'var(--danger)'"
                    :font-size="10.5"
                  />
                </button>
                <div v-if="!aiList.length" class="hint">旧库里还没有模型配置</div>
              </div>
              <div class="row-actions">
                <SoftButton :height="28" :font-size="12" @click="newProfile">新增一套</SoftButton>
                <SoftButton :height="28" :font-size="12" @click="saveProfile">保存这套</SoftButton>
                <SoftButton :height="28" :font-size="12" variant="danger" @click="removeProfile">
                  删除这套
                </SoftButton>
              </div>
            </div>

            <div class="field">
              <div class="label">名称</div>
              <input v-model="aiEdit.name" type="text" placeholder="比如 DeepSeek" />
            </div>
            <div class="field">
              <div class="label">接口地址</div>
              <input v-model="aiEdit.base_url" type="text" placeholder="https://api.deepseek.com/v1" />
            </div>
            <div class="field">
              <div class="label">模型名</div>
              <input v-model="aiEdit.model" type="text" list="ai-model-options" placeholder="deepseek-chat" />
              <datalist id="ai-model-options">
                <option v-for="one in aiModelOptions" :key="one.value" :value="one.value">
                  {{ one.label }}
                </option>
              </datalist>
              <div class="hint">
                下拉里是常用名字，也可以直接手打。**长期有效**的只有官方的
                <code>deepseek-chat</code>（通用）和 <code>deepseek-reasoner</code>（带思考过程）；
                带版本号的（<code>deepseek-v3.2</code> 之类）与聚合平台前缀的
                （<code>deepseek-ai/DeepSeek-V3</code>）要看你的账号或服务商支持哪个。
              </div>
            </div>
            <div class="field">
              <div class="label">这套配置的 API Key</div>
              <input
                type="password"
                :value="drafts['__profile_key__'] || ''"
                placeholder="已保存（不可查看），留空表示不改"
                @input="drafts['__profile_key__'] = ($event.target as HTMLInputElement).value"
              />
            </div>

            <div class="field inline">
              <div class="label">思考（先想再答）</div>
              <label class="switch">
                <input
                  type="checkbox"
                  :checked="(values['ai_thinking'] ?? '1') !== '0'"
                  @change="
                    (values['ai_thinking'] = ($event.target as HTMLInputElement).checked ? '1' : '0'),
                      settingsSave({ ai_thinking: values['ai_thinking'] })
                  "
                />
                <span />
              </label>
            </div>

            <div class="field">
              <div class="label">工具审核（AI 能用哪些工具）</div>
              <div class="tools-head">
                <label class="switch">
                  <input type="checkbox" :checked="toolsGlobal" @change="setToolsGlobal" />
                  <span />
                </label>
                <span class="thead">全局总开关</span>
                <span class="tcount">
                  已开 {{ Object.values(toolsOn).filter(Boolean).length }} / {{ AI_TOOLS.length }}
                </span>
              </div>
              <div v-for="group in TOOL_GROUPS" :key="group" class="tools-group">
                <div class="ghead">
                  <span class="gname">{{ group }}</span>
                  <button class="glink" type="button" @click="setGroup(group, true)">全开</button>
                  <button class="glink" type="button" @click="setGroup(group, false)">全关</button>
                </div>
                <div class="tools">
                  <label
                    v-for="tool in AI_TOOLS.filter((one) => one.group === group)"
                    :key="tool.name"
                    class="tool"
                    :class="{ off: !toolsGlobal }"
                    :title="tool.desc"
                  >
                    <input
                      type="checkbox"
                      :checked="!!toolsOn[tool.name]"
                      :disabled="!toolsGlobal"
                      @change="toggleTool(tool.name)"
                    />
                    <span class="tname">{{ tool.label }}</span>
                    <span class="tdesc">{{ tool.desc }}</span>
                    <span class="trisk" :class="tool.risk">{{ RISK_LABELS[tool.risk] }}</span>
                  </label>
                </div>
              </div>
              <div class="hint">
                共 {{ AI_TOOLS.length }} 个工具，开关与旧版一致（tool_on_&lt;名字&gt; / ai_tools_enabled），改动即时保存。
                高风险（发邮件 / 改设置 / 解压）默认关闭；下载 PDF、导出 EndNote 每次都要你确认。
              </div>
            </div>
          </template>

          <template v-else-if="card.key === 'elearn'">
            <div class="field">
              <div class="label">要下载的课程</div>
              <select
                class="course-select"
                :value="values['canvas_last_course'] || ''"
                @change="pickCanvasCourse"
              >
                <option value="">（先点「诊断 Canvas」拿课程清单）</option>
                <option v-for="one in canvasCourses" :key="one.id" :value="String(one.id)">
                  {{ one.name }}
                </option>
              </select>
              <div class="hint">诊断会把课程清单拉下来；选中的课程就是「下载课程文件」要下的那门</div>
            </div>
            <div class="field">
              <div class="label">课程文件</div>
              <div class="row-actions">
                <SoftButton :height="30" :font-size="12.5" :disabled="uisBusy" @click="doCanvasProbe">
                  诊断 Canvas
                </SoftButton>
                <SoftButton :height="30" :font-size="12.5" :disabled="uisBusy" @click="doCanvasDownload">
                  下载课程文件
                </SoftButton>
                <SoftButton :height="30" :font-size="12.5" :disabled="uisBusy" @click="doCanvasMigrate">
                  整理旧下载文件
                </SoftButton>
              </div>
              <div v-if="uisMsg" class="hint">{{ uisMsg }}</div>
              <div class="hint">
                下载会放进「下载目录 / 课程名 / 文件名」；「整理旧下载文件」会把以前平铺在下载目录里的旧文件
                搬进对应课程文件夹（事先判断不出课程的文件不动）
              </div>
            </div>
          </template>

          <template v-else-if="card.key === 'memory'">
            <div class="field">
              <div class="label">已有记忆（共 {{ memoryTotal }} 条）</div>
              <div class="mem-search">
                <input
                  v-model="memoryKeyword"
                  type="search"
                  class="mem-input"
                  placeholder="搜内容 / 标签，回车搜索"
                  @keydown.enter="refreshMemories"
                />
                <SoftButton :height="28" :font-size="12" @click="refreshMemories">搜索</SoftButton>
                <SoftButton :height="28" :font-size="12" @click="(memoryKeyword = ''), refreshMemories()">
                  全部
                </SoftButton>
              </div>
              <div class="mem-list">
                <div v-for="row in memories" :key="row.id" class="mem-row">
                  <template v-if="memoryEditing.id === row.id">
                    <textarea v-model="memoryEditing.content" class="mem-edit" rows="2" />
                    <input v-model="memoryEditing.tags" type="text" placeholder="标签，逗号分隔" />
                    <div class="row-actions">
                      <SoftButton variant="primary" :height="26" :font-size="11.5" @click="saveMemoryEdit">
                        保存
                      </SoftButton>
                      <SoftButton :height="26" :font-size="11.5" @click="memoryEditing.id = 0">
                        取消
                      </SoftButton>
                    </div>
                  </template>
                  <template v-else>
                    <div class="mem-text">{{ row.content }}</div>
                    <div class="mem-meta">
                      <span>{{ row.source || "手动" }}</span>
                      <span v-if="row.tags">· {{ row.tags }}</span>
                      <span>· {{ row.created_at.slice(0, 16) }}</span>
                      <span v-if="row.hits">· 带进对话 {{ row.hits }} 次</span>
                      <span class="spacer" />
                      <button class="glink" type="button" @click="startEditMemory(row)">编辑</button>
                      <button class="glink danger" type="button" @click="removeMemory(row)">删除</button>
                    </div>
                  </template>
                </div>
                <div v-if="!memories.length" class="hint">
                  还没有记忆：跟 AI 说「记住……」（它会自己写），也可以在下面手动加一条。
                </div>
              </div>
            </div>

            <div class="field">
              <div class="label">新增一条记忆</div>
              <textarea
                v-model="memoryDraft"
                rows="2"
                placeholder="比如：我周三下午三点有课；不吃香菜"
              />
              <div class="row-actions">
                <SoftButton
                  variant="primary"
                  :height="28"
                  :font-size="12"
                  :disabled="!memoryDraft.trim()"
                  @click="addMemory"
                >
                  记下
                </SoftButton>
                <SoftButton :height="28" :font-size="12" @click="refreshMemories">刷新</SoftButton>
                <SoftButton variant="danger" :height="28" :font-size="12" @click="clearMemories">
                  清空全部
                </SoftButton>
              </div>
              <div class="hint">
                每次 AI 对话都会把记忆拼进系统提示词（放不下会截断，最多 4000 字）；
                模型回答里单独一行写 [[记住]] 内容 时，程序会把它收进这里、并从回答里删掉。
              </div>
              <div v-if="memoryMsg" class="hint">{{ memoryMsg }}</div>
              <div v-if="memoryError" class="err">{{ memoryError }}</div>
            </div>
          </template>

          <template v-else>
            <div
              v-for="field in card.fields"
              :key="field.label"
              class="field"
              :class="{ inline: field.kind === 'switch' }"
            >
              <div class="label">{{ field.label }}</div>
              <div class="control">
                <span v-if="field.kind === 'note'" class="note">
                  {{ card.key === "account" ? values["user_name"] || "本机用户（管理员）" : "" }}
                </span>
                <label v-else-if="field.kind === 'switch'" class="switch">
                  <input
                    type="checkbox"
                    :checked="switchOn(field.key || '')"
                    @change="setValue(field, ($event.target as HTMLInputElement).checked ? '1' : '0')"
                  />
                  <span />
                </label>
                <select
                  v-else-if="field.kind === 'select'"
                  :value="values[field.key || ''] ?? ''"
                  @change="setValue(field, ($event.target as HTMLSelectElement).value)"
                >
                  <option value="">（默认）</option>
                  <option v-for="opt in field.options" :key="opt">{{ opt }}</option>
                </select>
                <textarea
                  v-else-if="field.kind === 'textarea'"
                  rows="3"
                  :value="values[field.key || ''] ?? ''"
                  @input="setValue(field, ($event.target as HTMLTextAreaElement).value)"
                />
                <input
                  v-else-if="field.kind === 'password'"
                  type="password"
                  :value="drafts[field.secret || ''] || ''"
                  :placeholder="field.placeholder"
                  @input="drafts[field.secret || ''] = ($event.target as HTMLInputElement).value"
                />
                <input
                  v-else
                  type="text"
                  :value="values[field.key || ''] ?? ''"
                  :placeholder="field.placeholder"
                  @input="setValue(field, ($event.target as HTMLInputElement).value)"
                />
                <span v-if="field.hint" class="hint">{{ field.hint }}</span>
              </div>
            </div>
          </template>

          <!-- 看板娘：立即应用大小 / 打开 / 关掉 -->
          <template v-if="card.key === 'mascot'">
            <div class="field">
              <div class="label">现在应用</div>
              <div class="control">
                <div class="row-actions">
                  <SoftButton :height="28" :font-size="12" @click="applyMascotScale">
                    立即应用大小
                  </SoftButton>
                  <SoftButton :height="28" :font-size="12" @click="openMascot">打开梅子</SoftButton>
                  <SoftButton :height="28" :font-size="12" @click="closeMascot">关掉梅子</SoftButton>
                </div>
                <span class="hint">{{ mascotMsg || "改完大小记得点「立即应用大小」；也可以直接关了重开" }}</span>
              </div>
            </div>
          </template>

          <!-- 盯邮箱：现在看一次 / 清空「已处理」 -->
          <template v-if="card.key === 'mailwatch'">
            <div class="field">
              <div class="label">现在试一次</div>
              <div class="control">
                <div class="row-actions">
                  <SoftButton :height="28" :font-size="12" :disabled="watchBusy" @click="watchCheckNow">
                    {{ watchBusy ? "正在看…" : "现在看一次" }}
                  </SoftButton>
                  <SoftButton :height="28" :font-size="12" :disabled="watchBusy" @click="watchResetSeen">
                    清空「已处理」
                  </SoftButton>
                </div>
                <span class="hint">{{ watchSummary || "（还没读）" }}</span>
                <span v-if="watchMsg" class="hint">{{ watchMsg }}</span>
                <div v-for="item in watchItems" :key="`${item.date}-${item.title}`" class="hint">
                  {{ item.date }} {{ item.start_time || "" }} · {{ item.title }}
                </div>
              </div>
            </div>
          </template>

          <!-- 代理：测一下现在能不能连校内服务（对应旧版 vpn.probe） -->
          <template v-if="card.key === 'vpn'">
            <div class="field">
              <div class="label">当前状态</div>
              <div class="control">
                <span class="hint">{{ proxySummaryText || "（还没读）" }}</span>
                <SoftButton :height="28" :font-size="12" :disabled="proxyBusy" @click="testProxy">
                  {{ proxyBusy ? "测试中…" : "测一下能不能连" }}
                </SoftButton>
                <span v-if="proxyMsg" class="hint">{{ proxyMsg }}</span>
                <div v-for="item in proxyResults" :key="item.name" class="hint">
                  {{ item.name }}：{{ item.ok ? "通" : "不通" }}
                  {{ item.status ? `（HTTP ${item.status}，${item.ms} ms）` : item.error || "" }}
                </div>
              </div>
            </div>
          </template>

          <!-- 课表：从教务直接导入（jwgl_import 的入口） -->
          <div v-if="card.key === 'timetable'" class="field">
            <div class="label">从教务导入课表</div>
            <div class="row-actions">
              <SoftButton :height="30" :font-size="12.5" :disabled="uisBusy" @click="doJwglProbe">
                教务诊断
              </SoftButton>
              <SoftButton
                :height="30"
                :font-size="12.5"
                variant="primary"
                :disabled="uisBusy"
                @click="doJwglImport"
              >
                导入课表
              </SoftButton>
            </div>
            <div v-if="uisMsg" class="hint">{{ uisMsg }}</div>
            <div class="hint">
              导入前需要先在「统一身份认证」里「打开登录窗口」登录并「抓取会话」。
              「教务诊断」只抓返回体、打印结构（不写数据），用来核对字段；
              「导入课表」会把课表格子写成每周日程（会跳过已存在的同名同时间条目）。
            </div>
          </div>

          <!-- 统一身份认证：与 Python 版同一套（内嵌网页自动填表 + 登录后自动存会话） -->
          <div v-if="card.key === 'uis'" class="field uis-session">
            <div class="label">登录会话</div>
            <div class="uis-status">
              <template v-if="uisState">
                <span class="dot" :class="uisState.has_session ? 'ok' : 'off'" />
                <span>
                  {{ uisState.has_session ? "会话已就绪" : "还没有会话" }}
                  <template v-if="uisState.has_session">
                    · {{ uisState.cookies }} 个 cookie ·
                    {{ uisState.has_service_session ? "已含业务站会话" : "缺业务站会话（点「重新抓取会话」走一遍 eLearning / eHall）" }}
                    <template v-if="uisState.saved_at">· 抓取于 {{ uisState.saved_at }}</template>
                  </template>
                  · 学号 {{ uisState.username || "未填" }}
                  · 自动填表 {{ uisState.remember_password ? "开" : "关" }}
                </span>
              </template>
              <span v-else class="tiny">读不到会话状态</span>
            </div>
            <div class="row-actions">
              <SoftButton
                :height="30"
                :font-size="12.5"
                variant="primary"
                :disabled="uisBusy"
                @click="doUisOpen"
              >
                打开登录页（自动填表）
              </SoftButton>
              <SoftButton :height="30" :font-size="12.5" :disabled="uisBusy" @click="doUisCheck">
                检查自动填充
              </SoftButton>
              <SoftButton :height="30" :font-size="12.5" :disabled="uisBusy" @click="doUisCapture">
                重新抓取会话
              </SoftButton>
              <SoftButton :height="30" :font-size="12.5" :disabled="uisBusy" @click="doUisLogout">
                退出 UIS
              </SoftButton>
            </div>
            <div v-if="uisMsg" class="hint">{{ uisMsg }}</div>
            <div class="hint">
              和 Python 版一样：点「打开登录页」后，学号密码会**自动填好并点登录**（页面自己加密提交，
              我们不碰密码）；需要短信 / 扫码二次验证时手动过一次。登录成功后会话会**自动存进本机**
              （DPAPI 加密），Canvas 下载、eHall 实时刷新、教务课表都用它，不用再点「抓取会话」——
              这个按钮只是抓不到时的兜底。
            </div>
          </div>

                    <div v-if="card.key === 'account'" class="card-foot">
            <!-- 只有服务器账号是 admin 才给这个入口；普通用户看不到（也进不去后台） -->
            <template v-if="isServerAdmin">
              <SoftButton :height="30" :font-size="12.5" variant="primary" @click="adminOpen = true">
                账户审核（管理员）
              </SoftButton>
              <span class="tiny">批准注册申请、续期 / 停用 / 注销账户、复制回信全文</span>
            </template>
            <span v-else class="tiny">
              当前账号是普通用户：账号相关操作（封禁 / 改口令 / 看设备）由管理员在网页后台处理
            </span>
          </div>
          <div v-if="card.actions" class="card-foot">
            <SoftButton
              v-if="card.actions.includes('smtp-test')"
              :height="30"
              :font-size="12.5"
              @click="testSmtp"
            >
              测试发信配置
            </SoftButton>
            <SoftButton
              v-if="card.actions.includes('imap-test')"
              :height="30"
              :font-size="12.5"
              @click="testImap"
            >
              测试收信配置
            </SoftButton>
            <SoftButton :height="30" :font-size="12.5" @click="load">还原</SoftButton>
            <SoftButton variant="primary" :height="30" :font-size="12.5" @click="save(card)">
              保存
            </SoftButton>
          </div>
        </Card>
        <AdminPanel v-model:open="adminOpen" />
        <Card v-if="!visible.length" :padding="18">
          <div class="muted">没找到相关设置：换个词试试（vpn、字体、key、代理…）</div>
        </Card>
      </div>
    </div>

    <!-- 导入前确认：说清楚会覆盖当前数据、但一定会先备份 -->
    <div v-if="migrate.confirmOpen" class="modal" @click.self="migrate.confirmOpen = false">
      <div class="modal-card">
        <div class="modal-head">
          <div class="h2">确认导入数据包</div>
        </div>
        <div class="note">
          将用这个包里的数据<b>覆盖当前所有数据</b>（日程、备忘、通知、文献、邮件记录、设置…）。
        </div>
        <div class="path-box">{{ migrate.importPath }}</div>
        <div class="hint">
          导入前会自动把当前数据库备份到 data/backups/pre-import-时间.db，备份成功才会继续；
          导入完成后需要重新打开应用才生效。
        </div>
        <div class="hint">
          口令类设置（SMTP / IMAP 授权码、AI Key、UIS 口令、服务器登录）不会跟着包走，导入后要重新填。
        </div>
        <div v-if="migrate.error" class="err">{{ migrate.error }}</div>
        <div class="modal-foot">
          <div class="spacer" />
          <SoftButton :height="30" :font-size="12.5" @click="migrate.confirmOpen = false">
            取消
          </SoftButton>
          <SoftButton
            variant="primary"
            :height="30"
            :font-size="12.5"
            :disabled="migrate.busy"
            @click="confirmImportPackage"
          >
            {{ migrate.busy ? "导入中…" : "确认导入" }}
          </SoftButton>
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
.body {
  flex: 1;
  display: flex;
  gap: 14px;
  min-height: 0;
}
.cats {
  width: 232px;
  flex: none;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.cats input {
  margin-bottom: 8px;
}
.cat {
  height: 32px;
  padding: 0 10px;
  border: none;
  border-radius: 9px;
  background: transparent;
  color: var(--text-soft);
  font-family: inherit;
  font-size: 13.5px;
  text-align: left;
  cursor: pointer;
}
.cat:hover {
  background: var(--nav-hover-bg);
}
.cat.on {
  background: var(--card);
  color: var(--text);
  font-weight: 600;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.07);
}
.cards {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-right: 4px;
}
.card-head {
  display: flex;
  gap: 10px;
  align-items: flex-start;
}
.head-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.title {
  font-size: 15px;
  font-weight: 640;
}
.subtitle {
  font-size: 12.5px;
  color: var(--muted);
  line-height: 1.5;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 4px 0;
}
.field.inline {
  flex-direction: row;
  align-items: center;
  justify-content: space-between;
}
.label {
  font-size: 13px;
  color: var(--text-soft);
}
.control {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.control input,
.control select,
.control textarea {
  max-width: 480px;
}
/* 课程下拉框：别拉满整行 */
.course-select {
  max-width: 420px;
}
.hint {
  font-size: 11.5px;
  color: var(--muted);
}
.note {
  font-size: 13px;
  color: var(--text);
}
.profile-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.profile {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border-radius: 10px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  font-family: inherit;
  cursor: pointer;
  text-align: left;
}
.profile.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 7%, var(--card));
}
.pname {
  font-size: 13px;
  color: var(--text);
}
.pmodel {
  font-size: 11.5px;
  color: var(--muted);
  flex: 1;
}
.switch-row {
  display: flex;
  gap: 6px;
}
.mode-btn {
  height: 30px;
  padding: 0 14px;
  border-radius: var(--radius-btn);
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--text-soft);
  font-family: inherit;
  font-size: 12.5px;
  cursor: pointer;
}
.mode-btn.on {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, var(--card));
  font-weight: 600;
}
.swatches {
  display: flex;
  gap: 8px;
}
.swatch {
  width: 22px;
  height: 22px;
  border-radius: 999px;
  border: 2px solid transparent;
  cursor: pointer;
}
.swatch.on {
  border-color: var(--text);
}
/* 自定义颜色：把浏览器原生那个小方框藏掉，外面画成和第 8 个圆一样的样式 */
.swatch-pick {
  position: relative;
  display: inline-block;
  width: 22px;
  height: 22px;
  border-radius: 999px;
  border: 2px solid transparent;
  box-shadow: inset 0 0 0 1px var(--border-soft);
  background: var(--accent);
  cursor: pointer;
  overflow: hidden;
  vertical-align: middle;
}
.swatch-pick:hover {
  border-color: var(--muted);
}
.swatch-pick input[type="color"] {
  position: absolute;
  inset: -4px;
  width: calc(100% + 8px);
  height: calc(100% + 8px);
  padding: 0;
  border: 0;
  opacity: 0;
  cursor: pointer;
}
.switch {
  position: relative;
  width: 40px;
  height: 22px;
  display: inline-block;
}
.switch input {
  opacity: 0;
  width: 0;
  height: 0;
}
.switch span {
  position: absolute;
  inset: 0;
  background: var(--slider-groove);
  border-radius: 999px;
  transition: background 0.15s ease;
  cursor: pointer;
}
.switch span::after {
  content: "";
  position: absolute;
  width: 16px;
  height: 16px;
  left: 3px;
  top: 3px;
  background: #fff;
  border-radius: 999px;
  transition: transform 0.15s ease;
}
.switch input:checked + span {
  background: var(--accent);
}
.switch input:checked + span::after {
  transform: translateX(18px);
}
.card-foot {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  padding-top: 4px;
}

.tones {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.tone {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  padding: 0 10px;
  border-radius: var(--radius-btn);
  border: 1px solid var(--border);
  background: var(--card);
  color: var(--text-soft);
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
}
.tone.on {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, var(--card));
  font-weight: 600;
}
.tone-chip {
  width: 14px;
  height: 14px;
  border-radius: 4px;
  border: 1px solid var(--border-soft);
}
.swatch-pick {
  width: 22px;
  height: 22px;
  border-radius: 999px;
  border: 2px dashed var(--border);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  overflow: hidden;
}
.swatch-pick input {
  width: 26px;
  height: 26px;
  border: none;
  background: transparent;
  padding: 0;
  cursor: pointer;
}

.row-actions {
  display: flex;
  gap: 6px;
  padding-top: 4px;
}
.tools-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 0 8px;
  max-width: 760px;
}
.thead {
  font-size: 12.5px;
  color: var(--text);
}
.tcount {
  margin-left: auto;
  font-size: 11.5px;
  color: var(--muted);
}
.tools-group {
  max-width: 760px;
  border-top: 1px solid var(--border-soft);
  padding: 7px 0 3px;
}
.ghead {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 5px;
}
.gname {
  font-size: 12px;
  color: var(--muted);
  letter-spacing: 0.4px;
}
.glink {
  border: 0;
  background: none;
  padding: 0;
  font-size: 11.5px;
  color: var(--accent);
  cursor: pointer;
}
.glink:hover {
  text-decoration: underline;
}
.tools {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 4px 14px;
  max-width: 760px;
}
.tool {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--text-soft);
  text-align: left;
  padding: 2px 0;
}
.tool.off {
  opacity: 0.55;
}
.tname {
  flex: 0 0 auto;
}
.tdesc {
  flex: 1;
  min-width: 0;
  font-size: 11px;
  color: var(--muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.trisk {
  flex: 0 0 auto;
  font-size: 10.5px;
  color: var(--muted);
}
.trisk.mid {
  color: var(--warning);
}
.trisk.high {
  color: var(--danger);
}

.path-box {
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-soft);
  background: var(--card-alt);
  border: 1px solid var(--border-soft);
  border-radius: 8px;
  padding: 6px 9px;
  cursor: pointer;
  overflow-wrap: anywhere;
}

/* 导入确认（与发件箱那个 modal 同一套外观，AntD 风格的确认框） */
.modal {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.28);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 40;
}
.modal-card {
  width: 520px;
  max-width: calc(100vw - 48px);
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 16px;
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  box-shadow: 0 18px 48px rgba(0, 0, 0, 0.28);
}
.modal-head,
.modal-foot {
  display: flex;
  align-items: center;
  gap: 8px;
}
.modal-head .h2 {
  font-size: 16px;
  font-weight: 650;
}
.spacer {
  flex: 1;
}
.err {
  font-size: 12px;
  color: var(--danger);
  line-height: 1.5;
}

.cards > * {
  width: 100%;
  flex: none;
}
.card-foot {
  flex-wrap: wrap;
  justify-content: flex-start;
}
.card-foot .tiny {
  flex-basis: 100%;
}
.uis-status {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
  color: var(--text-soft);
  padding: 2px 0 6px;
}
.uis-status .dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--border-soft);
  flex: 0 0 auto;
}
.uis-status .dot.ok {
  background: var(--success, #2f9e63);
}
.uis-status .dot.off {
  background: var(--warning, #c98a1a);
}
.uis-pwd {
  width: 180px;  min-width: 0;
  height: 30px;
  border-radius: 8px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text);
  font-size: 12px;
  padding: 0 8px;
  text-align: left;
}

/* AI 记忆卡：搜索 + 条目列表（列表留一点内边距、上限高度自己滚） */
.mem-search {
  display: flex;
  align-items: center;
  gap: 6px;
}
.mem-search .mem-input {
  flex: 1;
  max-width: 320px;
}
.mem-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 320px;
  overflow-y: auto;
  padding: 2px;
}
.mem-row {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 7px 10px;
  border-radius: 10px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
}
.mem-text {
  font-size: 13px;
  color: var(--text);
  line-height: 1.55;
  overflow-wrap: anywhere;
}
.mem-meta {
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 11px;
  color: var(--muted);
  flex-wrap: wrap;
}
.mem-edit {
  min-height: 48px;
}
.glink.danger {
  color: var(--danger);
}
</style>
