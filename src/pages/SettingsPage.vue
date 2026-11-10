<script setup lang="ts">
// 设置：左侧分类 + 搜索 + 20 张设置卡；字段与旧库的设置键一一对应，
// 保存直接写回 data/lumina.db（密码走 DPAPI，只写不读）。
import { computed, onMounted, reactive, ref } from "vue";
import Card from "../components/Card.vue";
import SoftButton from "../components/SoftButton.vue";
import IconBadge from "../components/IconBadge.vue";
import AdminPanel from "../components/AdminPanel.vue";
import AccountPanel from "../components/AccountPanel.vue";
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
  settingsAll,
  settingsSave,
  uisLogin,
  uisLogout,
  canvasDownload,
  canvasMigrate,
  canvasProbe,
  jwglProbe,
  uisCapture,
  uisOpenLogin,
  uisStatus,
  setSetting,
  type AiProfile,
  type AiState,
  type DataImportResult,
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
  has_saved_password: boolean;
} | null>(null);

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
    uisMsg.value = result.ok
      ? `Canvas 可用：${result.courses} 门课程（${(result.names || []).join("、")}）`
      : String(result.error);
    if (result.ok) {
      // 诊断会把首门课记进设置，这里只补这一个键，免得「下载课程文件」读到旧值
      const fresh = await settingsAll();
      const course = fresh.values["canvas_last_course"];
      if (course) values["canvas_last_course"] = course;
    }
  } catch (err) {
    uisMsg.value = String(err);
  } finally {
    uisBusy.value = false;
  }
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
    const result = await jwglProbe(values["jwgl_semester"] || "");
    uisMsg.value = result.ok
      ? `教务接口可用：shape=${result.shape} bytes=${result.bytes} keys=${(result.top_keys || []).join(",") || "(无)"}`
      : String(result.error);
  } catch (err) {
    uisMsg.value = String(err);
  } finally {
    uisBusy.value = false;
  }
}

async function doUisOpen() {
  const result = await uisOpenLogin();
  uisMsg.value = result.ok
    ? "已打开登录窗口：在里面完成登录后，回来点「抓取会话」"
    : String(result.error);
}

async function doUisCapture() {
  uisBusy.value = true;
  try {
    const result = await uisCapture();
    uisMsg.value = result.ok
      ? `已抓取 ${result.cookies} 个 cookie：${(result.names || []).join(", ")}`
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
    subtitle: "作息时间与教学周，用来算「第几周」和出发提醒",
    category: "日程",
    fields: [
      { label: "开学日期", kind: "text", key: "term_start", placeholder: "2026-09-07" },
      { label: "一节课几分钟", kind: "text", key: "period_minutes", placeholder: "45" },
      { label: "课间休息", kind: "text", key: "period_break", placeholder: "10" },
      { label: "上午第一节", kind: "text", key: "morning_start", placeholder: "08:00" },
      { label: "下午第一节", kind: "text", key: "afternoon_start", placeholder: "13:30" },
      { label: "晚上第一节", kind: "text", key: "evening_start", placeholder: "18:30" },
      { label: "单双周显示", kind: "switch", key: "week_parity" },
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
    key: "prompts",
    title: "系统提示词",
    subtitle: "AI 的人设与规则",
    category: "AI",
    fields: [{ label: "系统提示词", kind: "textarea", key: "system_prompt" }],
    actions: ["save"],
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
      { label: "UIS 学号 / 工号", kind: "text", key: "uis_username", placeholder: "如 26303050178" },
      { label: "记住 UIS 密码（DPAPI 加密）", kind: "switch", key: "uis_autofill" },
    ],
    actions: ["save"],
  },
  {
    key: "uis",
    title: "统一身份认证",
    subtitle: "内嵌网页跳到登录页时自动填表",
    category: "其他",
    fields: [
      { label: "学号 / 账号", kind: "text", key: "uis_user" },
      { label: "密码", kind: "password", secret: "uis_password" },
      { label: "自动填表并登录", kind: "switch", key: "uis_autofill" },
      { label: "登录页地址", kind: "text", key: "uis_url", placeholder: "https://id.fudan.edu.cn/ac/#/index" },
      { label: "登录后回跳地址", kind: "text", key: "uis_redirect" },
      { label: "验证码处理", kind: "select", key: "uis_captcha", options: ["人工（推荐）", "自动识别"] },
      { label: "掉登录时自动重登", kind: "switch", key: "uis_relogin" },
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
      { label: "代理地址", kind: "text", key: "proxy_host" },
      { label: "代理端口", kind: "text", key: "proxy_port" },
      { label: "认证方式", kind: "select", key: "proxy_auth", options: ["账号密码", "免认证"] },
      { label: "账号", kind: "text", key: "proxy_user" },
      { label: "密码", kind: "password", secret: "proxy_password" },
      { label: "不走代理的域名", kind: "textarea", key: "proxy_bypass", placeholder: "fudan.edu.cn, localhost" },
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

async function doBackup() {
  const result = await backupDb();
  message.value = result.ok
    ? `已备份：${result.path}（${Math.round(result.bytes / 1024)} KB）`
    : result.error;
}

onMounted(async () => {
  await load();
  info.value = await dataInfo();
});
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
        <input v-model="keyword" type="search" placeholder="搜索设置（比如 vpn、字体、key）" />
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
                  @change="setBrowserTabs(($event.target as HTMLInputElement).checked), persistTheme()"
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
              <input v-model="aiEdit.model" type="text" placeholder="deepseek-chat" />
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
                    :checked="(values[field.key || ''] ?? '') !== '0'"
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

                    <div v-if="card.key === 'account'" class="card-foot">
            <SoftButton :height="30" :font-size="12.5" variant="primary" @click="adminOpen = true">
              账户审核（管理员）
            </SoftButton>
            <span class="tiny">批准注册申请、续期 / 停用 / 注销账户、复制回信全文</span>
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
.uis-pwd {
  width: 180px;
  min-width: 0;
  height: 30px;
  border-radius: 8px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text);
  font-size: 12px;
  padding: 0 8px;
  text-align: left;
}</style>
