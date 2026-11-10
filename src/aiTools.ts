// AI 工具清单 —— 严格对照旧版 Python 应用 _lumina/lumina/ai_tools.py 的 _SPECS。
// 存储语义保持一致：全局开关 ai_tools_enabled（"1"/"0"，默认开），
// 单个工具开关 tool_on_<name>（"1"/"0"，未设过用默认值）。
export type AiRisk = "low" | "mid" | "high";

export interface AiTool {
  name: string;
  label: string;
  desc: string;
  risk: AiRisk;
  on: boolean;
  group: string;
}

export const RISK_LABELS: Record<AiRisk, string> = {
  low: "低风险",
  mid: "中风险",
  high: "高风险",
};

export const HIGH_RISK_NAMES = [
  "mail_send",
  "setting_set",
  "powershell_run",
  "screen_capture",
  "zip_extract",
  "delete_file",
];

/** 每次执行都要用户确认的（下载/导出由用户自己决定） */
export const ALWAYS_CONFIRM = ["pdf_download", "endnote_export"];

/** 后台线程跑的慢工具（网络 / 磁盘） */
export const SLOW_TOOLS = [
  "elearning_courses",
  "elearning_download",
  "timetable_fetch",
  "fs_search",
];

export const TOOL_GROUPS = [
  "日程与专注",
  "课程与教务",
  "本地文件",
  "只读信息",
  "网络与资料",
  "文献与引用",
  "邮件与设置",
];

export const AI_TOOLS: AiTool[] = [
  // ---------------- 日程与专注 ----------------
  {
    name: "schedule_list",
    label: "查日程",
    desc: "查看某段时间的日程（只读应用内的日程表）",
    risk: "low",
    on: true,
    group: "日程与专注",
  },
  {
    name: "schedule_add",
    label: "加日程",
    desc: "新建一条日程",
    risk: "low",
    on: true,
    group: "日程与专注",
  },
  {
    name: "schedule_update",
    label: "改日程",
    desc: "修改已有日程",
    risk: "low",
    on: true,
    group: "日程与专注",
  },
  {
    name: "schedule_delete",
    label: "删日程",
    desc: "删除日程（删了就没了）",
    risk: "mid",
    on: true,
    group: "日程与专注",
  },
  {
    name: "countdown_add",
    label: "加倒计时",
    desc: "新建重要日倒计时（考试 / 截止日）",
    risk: "low",
    on: true,
    group: "日程与专注",
  },
  {
    name: "pomodoro_start",
    label: "开始专注",
    desc: "启动应用里的专注计时器",
    risk: "low",
    on: true,
    group: "日程与专注",
  },
  // ---------------- 课程与教务 ----------------
  {
    name: "elearning_courses",
    label: "读 eLearning",
    desc: "读课程与作业截止时间（需要已登录）",
    risk: "low",
    on: true,
    group: "课程与教务",
  },
  {
    name: "elearning_download",
    label: "下载课程文件",
    desc: "把课程文件 / 作业附件下载到下载目录",
    risk: "mid",
    on: true,
    group: "课程与教务",
  },
  {
    name: "timetable_fetch",
    label: "抓课表",
    desc: "用教务登录态抓课表并写进日历",
    risk: "mid",
    on: true,
    group: "课程与教务",
  },
  // ---------------- 本地文件 ----------------
  {
    name: "fs_list",
    label: "看本地目录",
    desc: "列出允许目录里的文件（只读）",
    risk: "low",
    on: true,
    group: "本地文件",
  },
  {
    name: "fs_read",
    label: "读本地文件",
    desc: "读取允许目录里的文本文件（只读）",
    risk: "low",
    on: true,
    group: "本地文件",
  },
  {
    name: "fs_search",
    label: "搜本地文件",
    desc: "在允许目录里按内容搜关键词（只读）",
    risk: "low",
    on: true,
    group: "本地文件",
  },
  {
    name: "fs_write",
    label: "写本地文件",
    desc: "新建 / 覆盖 / 追加文本文件（需要「读写」档）",
    risk: "mid",
    on: false,
    group: "本地文件",
  },
  {
    name: "fs_discard",
    label: "丢弃文件（改名）",
    desc: "「删除」时不真删：改名成 [discarded] 原名，并告诉你去哪个目录彻底删",
    risk: "mid",
    on: false,
    group: "本地文件",
  },
  // ---------------- 只读信息 ----------------
  {
    name: "time_now",
    label: "看时间",
    desc: "返回精确日期 / 时间 / 星期 / 时区（只读）",
    risk: "low",
    on: true,
    group: "只读信息",
  },
  {
    name: "sys_info",
    label: "看电脑状况",
    desc: "CPU / 内存 / 磁盘 / 电池 / 系统版本 / 本机 IP（只读）",
    risk: "low",
    on: true,
    group: "只读信息",
  },
  {
    name: "app_state",
    label: "看应用状态",
    desc: "今天日程、未读通知、邮件数、备忘数（只读）",
    risk: "low",
    on: true,
    group: "只读信息",
  },
  {
    name: "memo_search",
    label: "搜备忘",
    desc: "在本地备忘里搜关键词（只读）",
    risk: "low",
    on: true,
    group: "只读信息",
  },
  {
    name: "memo_append",
    label: "记到备忘",
    desc: "往备忘里追加一段（没有同名就新建），只写应用内数据",
    risk: "low",
    on: true,
    group: "只读信息",
  },
  {
    name: "usage_stats",
    label: "看用量",
    desc: "本机 API 调用次数 / token / 时长（只读）",
    risk: "low",
    on: true,
    group: "只读信息",
  },
  {
    name: "tool_help",
    label: "查可用工具",
    desc: "列出现在能用哪些工具（只读）",
    risk: "low",
    on: true,
    group: "只读信息",
  },
  {
    name: "window_list",
    label: "看窗口标题",
    desc: "列出当前可见窗口标题（暴露你开着什么，默认关）",
    risk: "mid",
    on: false,
    group: "只读信息",
  },
  {
    name: "flashcards_create",
    label: "生成复习卡片",
    desc: "把知识点做成间隔重复卡片（默认关）",
    risk: "mid",
    on: false,
    group: "只读信息",
  },
  // ---------------- 网络与资料 ----------------
  {
    name: "web_search",
    label: "上网搜索",
    desc: "通用网页搜索（DDG 主、Bing 兜底）",
    risk: "low",
    on: true,
    group: "网络与资料",
  },
  {
    name: "web_fetch",
    label: "读网页",
    desc: "抓网页正文转纯文本（限长）",
    risk: "low",
    on: true,
    group: "网络与资料",
  },
  {
    name: "video_search",
    label: "找课程视频",
    desc: "找 B 站 / MOOC / YouTube 上的课程视频",
    risk: "low",
    on: true,
    group: "网络与资料",
  },
  {
    name: "course_resource_search",
    label: "找课件资料",
    desc: "找讲义 / 课件 / 习题：先翻 eLearning 课程文件，再上网搜",
    risk: "low",
    on: true,
    group: "网络与资料",
  },
  {
    name: "resource_push",
    label: "推送资源",
    desc: "把资源存进「学习资源」并推送成卡片",
    risk: "low",
    on: true,
    group: "网络与资料",
  },
  // ---------------- 文献与引用 ----------------
  {
    name: "arxiv_search",
    label: "搜 arXiv",
    desc: "arXiv 检索（不用 Key）",
    risk: "low",
    on: true,
    group: "文献与引用",
  },
  {
    name: "crossref_search",
    label: "搜 Crossref",
    desc: "Crossref 检索（DOI / 期刊 / 出版社）",
    risk: "low",
    on: true,
    group: "文献与引用",
  },
  {
    name: "semantic_scholar_search",
    label: "搜 Semantic Scholar",
    desc: "带引用数与摘要的检索",
    risk: "low",
    on: true,
    group: "文献与引用",
  },
  {
    name: "doi_lookup",
    label: "按 DOI 查",
    desc: "DOI 精确查元数据 + BibTeX",
    risk: "low",
    on: true,
    group: "文献与引用",
  },
  {
    name: "literature_search",
    label: "搜文献",
    desc: "三家一起搜并按 DOI / 标题合并去重",
    risk: "low",
    on: true,
    group: "文献与引用",
  },
  {
    name: "pdf_download",
    label: "下载文献 PDF",
    desc: "下载用户勾选的文献到「我的文献库」（每次都要你确认）",
    risk: "mid",
    on: true,
    group: "文献与引用",
  },
  {
    name: "citation_format",
    label: "生成引用",
    desc: "GB/T 7714 / APA / BibTeX / RIS",
    risk: "low",
    on: true,
    group: "文献与引用",
  },
  {
    name: "endnote_export",
    label: "导出给 EndNote",
    desc: "导出 .enw / .ris；没装 EndNote 会先问你是否经 ehall 装",
    risk: "low",
    on: true,
    group: "文献与引用",
  },
  {
    name: "library_open",
    label: "打开数据库",
    desc: "打开图书馆 / IEEE / ScienceDirect 搜索页（验证交给你）",
    risk: "low",
    on: true,
    group: "文献与引用",
  },
  // ---------------- 邮件与设置 ----------------
  {
    name: "mail_send",
    label: "发邮件",
    desc: "代发邮件：先弹可编辑窗口让你确认 / 修改，点发送才发",
    risk: "high",
    on: false,
    group: "邮件与设置",
  },
  {
    name: "setting_get",
    label: "读设置",
    desc: "读白名单里允许修改的设置",
    risk: "low",
    on: true,
    group: "邮件与设置",
  },
  {
    name: "setting_set",
    label: "改设置",
    desc: "改白名单设置（每次都要你确认；密码 / 账号一律不给改）",
    risk: "high",
    on: false,
    group: "邮件与设置",
  },
  {
    name: "zip_extract",
    label: "解压压缩包",
    desc: "解压到工作区（防路径穿越，每次都要你确认）",
    risk: "high",
    on: false,
    group: "邮件与设置",
  },
];

/** 单个工具开关的存储键（与旧版一致） */
export function toolKey(name: string): string {
  return "tool_on_" + name;
}

/** 全局工具总开关的存储键 */
export const TOOLS_ENABLED_KEY = "ai_tools_enabled";

/** 旧版语义：未设过就取默认值 */
export function toolEnabled(values: Record<string, string>, name: string): boolean {
  const raw = String(values[toolKey(name)] ?? "").trim();
  if (raw === "0" || raw === "1") return raw === "1";
  const spec = AI_TOOLS.find((one) => one.name === name);
  if (HIGH_RISK_NAMES.includes(name)) return false;
  return spec ? spec.on : true;
}

export function toolsGloballyOn(values: Record<string, string>): boolean {
  const raw = String(values[TOOLS_ENABLED_KEY] ?? "1").trim();
  return raw !== "0";
}
