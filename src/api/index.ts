// 前端 → Rust 的唯一出口。
// 在 Tauri 里走 invoke；在浏览器里（npm run dev）自动降级为本地 mock，
// 这样调界面不需要先把 Rust 编好。
import { invoke } from "@tauri-apps/api/core";

export type EventRow = {
  id: number;
  title: string;
  description: string;
  date: string;
  start_time: string;
  end_time: string;
  remind_minutes: number | null;
  color: string;
  done: boolean;
  group_id: number | null;
  repeat: string;
  created_at: string;
  updated_at: string;
};

export type UpcomingItem = EventRow & {
  day: string;
  begin: string;
  finish: string;
  starts: string;
  ongoing: boolean;
  minutes_until: number;
};

export type NoticeRow = {
  id: number;
  title: string;
  url: string;
  source: string;
  published_at: string;
  fetched_at: string;
};

export type AssignmentRow = {
  title: string;
  course: string;
  due: string;
  url: string;
};

export type WeatherInfo = {
  ok: boolean;
  text: string;
  city: string;
  cached: boolean;
  error: string;
};

export const isTauri = "__TAURI_INTERNALS__" in window;

/* ------------------------------------------------------------------ mock */

const today = new Date();
const iso = (d: Date) => d.toISOString().slice(0, 10);
const pad = (n: number) => String(n).padStart(2, "0");
const clockIn = (minutes: number) => {
  const d = new Date(Date.now() + minutes * 60000);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
};

const MOCK_EVENTS: EventRow[] = [
  {
    id: 1,
    title: "设计评审会",
    description: "",
    date: iso(today),
    start_time: clockIn(-30),
    end_time: clockIn(60),
    remind_minutes: 15,
    color: "violet",
    done: false,
    group_id: null,
    repeat: "",
    created_at: "",
    updated_at: "",
  },
  {
    id: 2,
    title: "完成季度汇报材料",
    description: "",
    date: iso(today),
    start_time: "09:00",
    end_time: "10:00",
    remind_minutes: 30,
    color: "teal",
    done: true,
    group_id: null,
    repeat: "",
    created_at: "",
    updated_at: "",
  },
  {
    id: 3,
    title: "健身训练",
    description: "",
    date: iso(today),
    start_time: "18:30",
    end_time: "19:30",
    remind_minutes: 15,
    color: "amber",
    done: false,
    group_id: null,
    repeat: "",
    created_at: "",
    updated_at: "",
  },
  {
    id: 4,
    title: "作业截止：GECC10046.01 从特洛伊到漫威世界 · Assignment for the 8th Week",
    description: "课程：GECC10046.01\n截止：2026-11-03 15:00",
    date: "2026-11-03",
    start_time: "14:30",
    end_time: "15:00",
    remind_minutes: 0,
    color: "rose",
    done: false,
    group_id: null,
    repeat: "",
    created_at: "",
    updated_at: "",
  },
];

const MOCK_SETTINGS: Record<string, string> = {
  theme_mode: "light",
  theme_accent: "#007AFF",
  dir_downloads: "elearning",
  user_name: "本机用户",
};

function mockEventsOn(date: string): EventRow[] {
  return MOCK_EVENTS.filter((e) => e.date === date).sort((a, b) =>
    a.start_time.localeCompare(b.start_time),
  );
}

function mockUpcoming(limit: number): UpcomingItem[] {
  const now = new Date();
  const rows: UpcomingItem[] = [];
  for (const event of MOCK_EVENTS) {
    const starts = new Date(`${event.date}T${event.start_time}:00`);
    const ends = new Date(`${event.date}T${event.end_time}:00`);
    if (ends < starts) ends.setDate(ends.getDate() + 1);
    if (ends < now && event.date <= iso(today)) continue;
    rows.push({
      ...event,
      day: event.date,
      begin: event.start_time,
      finish: event.end_time,
      starts: starts.toISOString(),
      ongoing: starts <= now && now < ends,
      minutes_until: Math.round((starts.getTime() - now.getTime()) / 60000),
    });
  }
  rows.sort((a, b) => a.starts.localeCompare(b.starts));
  return rows.slice(0, limit);
}

/* ------------------------------------------------------------- 命令封装 */

export async function eventsOn(date: string): Promise<EventRow[]> {
  if (!isTauri) return mockEventsOn(date);
  return invoke<EventRow[]>("events_on", { date });
}

export async function eventIndex(
  start: string,
  end: string,
): Promise<Record<string, EventRow[]>> {
  if (!isTauri) {
    const out: Record<string, EventRow[]> = {};
    for (const event of MOCK_EVENTS) out[event.date] = mockEventsOn(event.date);
    return out;
  }
  return invoke<Record<string, EventRow[]>>("event_index", { start, end });
}

export async function upcomingEvents(limit = 5): Promise<UpcomingItem[]> {
  if (!isTauri) return mockUpcoming(limit);
  return invoke<UpcomingItem[]>("upcoming_events", { limit });
}

export async function addEvent(payload: Partial<EventRow>): Promise<number> {
  if (!isTauri) {
    const id = Math.max(...MOCK_EVENTS.map((e) => e.id)) + 1;
    MOCK_EVENTS.push({
      id,
      title: payload.title || "未命名",
      description: payload.description || "",
      date: payload.date || iso(today),
      start_time: payload.start_time || "09:00",
      end_time: payload.end_time || "10:00",
      remind_minutes: payload.remind_minutes ?? 15,
      color: payload.color || "violet",
      done: false,
      group_id: null,
      repeat: "",
      created_at: "",
      updated_at: "",
    });
    return id;
  }
  return invoke<number>("add_event", { payload });
}

export async function updateEvent(
  id: number,
  patch: Partial<EventRow>,
): Promise<void> {
  if (!isTauri) {
    const target = MOCK_EVENTS.find((e) => e.id === id);
    if (target) Object.assign(target, patch);
    return;
  }
  await invoke("update_event", { id, patch });
}

export async function deleteEvent(id: number): Promise<void> {
  if (!isTauri) {
    const at = MOCK_EVENTS.findIndex((e) => e.id === id);
    if (at >= 0) MOCK_EVENTS.splice(at, 1);
    return;
  }
  await invoke("delete_event", { id });
}

export async function getSettings(): Promise<Record<string, string>> {
  if (!isTauri) return { ...MOCK_SETTINGS };
  return invoke<Record<string, string>>("get_settings");
}

export async function setSetting(key: string, value: string): Promise<void> {
  if (!isTauri) {
    MOCK_SETTINGS[key] = value;
    return;
  }
  await invoke("set_setting", { key, value });
}

export async function weather(): Promise<WeatherInfo> {
  if (!isTauri)
    return {
      ok: true,
      text: "17°C  晴 · 湿度 82% · 风速 5 km/h",
      city: "上海",
      cached: false,
      error: "",
    };
  return invoke<WeatherInfo>("weather");
}

export async function recentNotices(limit = 5): Promise<NoticeRow[]> {
  if (!isTauri) return [];
  return invoke<NoticeRow[]>("recent_notices", { limit });
}

/** 第 N 周：旧版显示的是当年的 ISO 周数，交给 Rust 算（浏览器预览时本地算） */
export async function weekNumber(): Promise<number> {
  if (!isTauri) {
    const now = new Date();
    const first = new Date(now.getFullYear(), 0, 1);
    const days = Math.floor((now.getTime() - first.getTime()) / 86400000);
    return Math.max(1, Math.ceil((days + first.getDay()) / 7));
  }
  return invoke<number>("week_number");
}

export async function recentAssignments(limit = 4): Promise<{
  ok: boolean;
  error: string;
  items: AssignmentRow[];
}> {
  if (!isTauri) return { ok: true, error: "", items: [] };
  return invoke("recent_assignments", { limit });
}

export async function bootInfo(): Promise<{
  data_dir: string;
  version: string;
  db: string;
  start_page?: string;
}> {
  if (!isTauri)
    return { data_dir: "（浏览器预览，没有真实数据）", version: "1.0.0", db: "" };
  return invoke("boot_info");
}

/* ------------------------------------------------------------ 通知 / 备忘 */

export type NoticeItem = NoticeRow & { read_at?: string | null };

const MOCK_NOTICES: NoticeItem[] = [
  {
    id: 1,
    title: "关于 2026 年国庆节放假安排的通知",
    url: "https://ehall.fudan.edu.cn/",
    source: "notice",
    published_at: "2026-10-27",
    fetched_at: "2026-10-27 10:00",
    read_at: null,
  },
  {
    id: 2,
    title: "2026 年秋季学期选课调整说明",
    url: "https://ehall.fudan.edu.cn/",
    source: "notice",
    published_at: "2026-10-26",
    fetched_at: "2026-10-26 09:00",
    read_at: "2026-10-26 12:00",
  },
];

export async function noticeList(
  unreadOnly = false,
  limit = 200,
): Promise<{ items: NoticeItem[]; total: number; unread: number }> {
  if (!isTauri) {
    const items = unreadOnly
      ? MOCK_NOTICES.filter((one) => !one.read_at)
      : MOCK_NOTICES;
    return {
      items,
      total: items.length,
      unread: MOCK_NOTICES.filter((one) => !one.read_at).length,
    };
  }
  return invoke("notice_list", { limit, unreadOnly });
}

export async function markNoticeRead(id: number): Promise<void> {
  if (!isTauri) {
    const target = MOCK_NOTICES.find((one) => one.id === id);
    if (target) target.read_at = new Date().toISOString().slice(0, 16);
    return;
  }
  await invoke("mark_notice_read", { id });
}

export async function markAllNoticesRead(): Promise<number> {
  if (!isTauri) {
    let count = 0;
    for (const one of MOCK_NOTICES) {
      if (!one.read_at) {
        one.read_at = new Date().toISOString().slice(0, 16);
        count += 1;
      }
    }
    return count;
  }
  return invoke<number>("mark_all_notices_read");
}

export async function refreshNotices(): Promise<{
  ok: boolean;
  added: number;
  error: string;
}> {
  if (!isTauri) return { ok: false, added: 0, error: "浏览器预览里不能抓取" };
  return invoke("refresh_notices");
}

export type MemoRow = {
  id: number;
  title: string;
  html: string;
  created_at: string;
  updated_at: string;
};

const MOCK_MEMOS: MemoRow[] = [
  {
    id: 1,
    title: "这周要做的事",
    html: "<div>① 交第八周作业</div><div>② 约助教问实验</div>",
    created_at: "2026-10-28 09:00",
    updated_at: "2026-10-30 20:26",
  },
];

export async function memos(): Promise<MemoRow[]> {
  if (!isTauri) return [...MOCK_MEMOS];
  return invoke<MemoRow[]>("memos");
}

export async function addMemo(title: string, html: string): Promise<number> {
  if (!isTauri) {
    const id = Math.max(0, ...MOCK_MEMOS.map((one) => one.id)) + 1;
    MOCK_MEMOS.unshift({
      id,
      title,
      html,
      created_at: "刚刚",
      updated_at: "刚刚",
    });
    return id;
  }
  return invoke<number>("add_memo", { title, html });
}

export async function updateMemo(
  id: number,
  title: string,
  html: string,
): Promise<boolean> {
  if (!isTauri) {
    const target = MOCK_MEMOS.find((one) => one.id === id);
    if (target) {
      target.title = title;
      target.html = html;
    }
    return true;
  }
  return invoke<boolean>("update_memo", { id, title, html });
}

export async function deleteMemo(id: number): Promise<boolean> {
  if (!isTauri) {
    const at = MOCK_MEMOS.findIndex((one) => one.id === id);
    if (at >= 0) MOCK_MEMOS.splice(at, 1);
    return true;
  }
  return invoke<boolean>("delete_memo", { id });
}

/* ---------------------------------------------------------------- 文档 */

export type DocEntry = {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified: string;
  kind: string;
};

export async function listDir(
  dir: string,
): Promise<{ dir: string; entries: DocEntry[] }> {
  if (!isTauri) return { dir, entries: [] };
  return invoke("list_dir", { dir });
}

export async function readTextFile(
  path: string,
  limit = 200000,
): Promise<string> {
  if (!isTauri) return "（浏览器预览里没有真实文件）";
  return invoke<string>("read_text_file", { path, limit });
}

/** 读二进制文件（PDF 预览）：返回 base64 与大小 */
export async function readFileBase64(
  path: string,
  limit?: number,
): Promise<{ name: string; size: number; data: string }> {
  if (!isTauri) return { name: path, size: 0, data: "" };
  return invoke("read_file_base64", { path, limit });
}

export async function openPath(path: string): Promise<void> {
  if (!isTauri) return;
  await invoke("open_path", { path });
}

export async function revealPath(path: string): Promise<void> {
  if (!isTauri) return;
  await invoke("reveal_path", { path });
}

export async function defaultDocsDir(): Promise<{
  downloads: string;
  data: string;
}> {
  if (!isTauri) return { downloads: "—", data: "—" };
  return invoke("default_docs_dir");
}

/* ---------------------------------------------------------------- 邮箱 */

export type MailSummary = {
  uid: string;
  subject: string;
  from: string;
  date: string;
  unread: boolean;
  has_attachments: boolean;
};

export type MailAttachment = { name: string; size: number; path: string };

export type MailBody = {
  uid: string;
  subject: string;
  from: string;
  date: string;
  html: string;
  text: string;
  attachments: MailAttachment[];
};

export type MailRecord = {
  id: number;
  to: string;
  subject: string;
  html: string;
  text: string;
  attachments: string;
  status: string;
  error: string;
  created_at: string;
  sent_at: string | null;
};

export async function mailAccountState(): Promise<{
  imap: boolean;
  smtp: boolean;
  user: string;
  from: string;
  imap_error: string;
  smtp_error: string;
}> {
  if (!isTauri)
    return {
      imap: true,
      smtp: true,
      user: "demo@example.com",
      from: "demo@example.com",
      imap_error: "",
      smtp_error: "",
    };
  return invoke("mail_account_state");
}

const MOCK_MAILS: MailSummary[] = [
  {
    uid: "1",
    subject: "《GEMH10013.01 心理健康与大学生活》有新公告",
    from: "李响 <li@fudan.edu.cn>",
    date: new Date().toUTCString(),
    unread: true,
    has_attachments: false,
  },
];

export async function mailList(
  limit = 30,
): Promise<{ ok: boolean; items: MailSummary[]; error: string }> {
  if (!isTauri) return { ok: true, items: MOCK_MAILS, error: "" };
  return invoke("mail_list", { limit });
}

export async function mailBody(
  uid: string,
): Promise<{ ok: boolean; body: MailBody | null; error: string }> {
  if (!isTauri)
    return {
      ok: true,
      body: {
        uid,
        subject: "（浏览器预览）示例邮件",
        from: "李响 <li@fudan.edu.cn>",
        date: new Date().toUTCString(),
        html: "<p>各位同学好：<br>本课程将于本周六 23:59 结束本轮修读。</p>",
        text: "",
        attachments: [],
      },
      error: "",
    };
  return invoke("mail_body", { uid });
}

const MOCK_SENT: MailRecord[] = [];

export async function mailSend(
  to: string,
  subject: string,
  html: string,
  attachments: string[] = [],
): Promise<{ ok: boolean; id: number; error: string }> {
  if (!isTauri) {
    MOCK_SENT.unshift({
      id: MOCK_SENT.length + 1,
      to,
      subject,
      html,
      text: "",
      attachments: "[]",
      status: "sent",
      error: "",
      created_at: new Date().toISOString().slice(0, 16),
      sent_at: new Date().toISOString().slice(0, 16),
    });
    return { ok: true, id: 1, error: "" };
  }
  return invoke("mail_send", { to, subject, html, attachments });
}

export async function mailTest(): Promise<{ ok: boolean; error: string }> {
  if (!isTauri) return { ok: true, error: "" };
  return invoke("mail_test");
}

export async function mails(limit = 100): Promise<MailRecord[]> {
  if (!isTauri) return [...MOCK_SENT];
  return invoke<MailRecord[]>("mails", { limit });
}

export async function deleteMail(id: number): Promise<boolean> {
  if (!isTauri) {
    const at = MOCK_SENT.findIndex((one) => one.id === id);
    if (at >= 0) MOCK_SENT.splice(at, 1);
    return true;
  }
  return invoke<boolean>("delete_mail", { id });
}

/* ---------------------------------------------------------------- 作业 */

export type HomeworkItem = {
  id: number | null;
  title: string;
  due: string;
  url: string | null;
  files: string[];
};

export type HomeworkCourse = { course: string; items: HomeworkItem[] };

export async function homeworkItems(): Promise<{
  ok: boolean;
  total: number;
  courses: HomeworkCourse[];
  downloads: string;
}> {
  if (!isTauri) return { ok: true, total: 0, courses: [], downloads: "" };
  return invoke("homework_items");
}

export async function homeworkWriteDeadlines(
  force = true,
): Promise<{ ok: boolean; added: number; updated: number; skipped: number; error: string }> {
  if (!isTauri)
    return { ok: false, added: 0, updated: 0, skipped: 0, error: "浏览器预览里不能写日程" };
  return invoke("homework_write_deadlines", { force });
}

/* -------------------------------------------------------------- 文献库 */

export type Paper = {
  id: number;
  title: string;
  authors: string;
  year: number;
  venue: string;
  doi: string;
  abstract: string;
  url: string;
  pdf_url: string;
  file_path: string;
  project: string;
  note: string;
  summary_zh: string;
  stars: number;
  source: string;
  added_at: string;
};

export async function papersList(
  project = "",
  starsMin = 0,
  keyword = "",
): Promise<{
  ok: boolean;
  items: Paper[];
  projects: { name: string; total: number }[];
}> {
  if (!isTauri) return { ok: true, items: [], projects: [] };
  return invoke("papers_list", { project, starsMin, keyword });
}

export async function paperUpdate(
  id: number,
  patch: { stars?: number; note?: string; summary?: string },
): Promise<boolean> {
  if (!isTauri) return true;
  return invoke<boolean>("paper_update", {
    id,
    stars: patch.stars ?? null,
    note: patch.note ?? null,
    summary: patch.summary ?? null,
  });
}

export async function paperCitation(id: number): Promise<{ ok: boolean; text: string }> {
  if (!isTauri) return { ok: false, text: "" };
  return invoke("paper_citation", { id });
}

/* ---------------------------------------------------------------- 设置 */

export async function settingsAll(): Promise<{
  values: Record<string, string>;
  secrets: string[];
}> {
  if (!isTauri) return { values: {}, secrets: [] };
  return invoke("settings_all");
}

export async function settingsSave(
  values: Record<string, string>,
): Promise<number> {
  if (!isTauri) return Object.keys(values).length;
  return invoke<number>("settings_save", { values });
}

/** 密码：明文交给 Rust 用 DPAPI 加密后写库（键名与旧版一致） */
export async function saveSecret(key: string, value: string): Promise<void> {
  if (!isTauri) return;
  await invoke("save_secret", { key, value });
}

export async function hasSecret(key: string): Promise<boolean> {
  if (!isTauri) return false;
  return invoke<boolean>("has_secret", { key });
}

export async function imapTest(): Promise<{
  ok: boolean;
  error: string;
  server?: string;
  user?: string;
}> {
  if (!isTauri) return { ok: true, error: "", server: "demo:993", user: "demo" };
  return invoke("imap_test");
}

/* ------------------------------------------------- 内嵌网页（eHall 等） */

export async function openWebWindow(
  label: string,
  url: string,
  title: string,
): Promise<void> {
  if (!isTauri) {
    window.open(url, "_blank");
    return;
  }
  const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.setFocus();
    return;
  }
  new WebviewWindow(label, {
    url,
    title,
    width: 1200,
    height: 820,
    center: true,
  });
}

/* ------------------------------------------------------------ ehall 卡片 */

export type EhallCard = {
  key: string;
  title: string;
  ok: boolean;
  value: string;
  lines: string[];
  error: string;
  source: string;
  age_minutes: number;
};

export async function ehallCards(): Promise<EhallCard[]> {
  if (!isTauri) return [];
  const result = await invoke<{ cards: EhallCard[] }>("ehall_cards");
  return result.cards || [];
}

/** 在 WebView2 里打开某张卡的数据页并抓取（登录态在那个窗口里，抓完写缓存） */
export async function ehallOpen(key: string): Promise<{
  ok: boolean;
  error: string;
  url?: string;
}> {
  if (!isTauri) return { ok: false, error: "浏览器预览里不能抓取" };
  return invoke("ehall_open", { key });
}

/* -------------------------------------------------------------- 空教室 */

export type ClassroomView = {
  ok: boolean;
  maps: Record<string, string[]>;
  rooms: Record<string, string[]>;
  building: string | null;
  room: string | null;
  value: string | null;
  url: string | null;
  ts: number | null;
  headers: string[];
  rows: string[][];
};

export async function classroomView(): Promise<ClassroomView> {
  if (!isTauri)
    return {
      ok: false,
      maps: {},
      rooms: {},
      building: null,
      room: null,
      value: null,
      url: null,
      ts: null,
      headers: [],
      rows: [],
    };
  return invoke<ClassroomView>("classroom_view");
}

/* -------------------------------------------------------------------- AI */

export type AiState = {
  ok: boolean;
  id: number;
  name: string;
  base_url: string;
  model: string;
  has_key: boolean;
  error: string;
};

export type AiProfile = {
  id: number;
  name: string;
  base_url: string;
  model: string;
  has_key: boolean;
};

export type ChatMessage = { role: "system" | "user" | "assistant"; content: string };

export async function aiState(): Promise<AiState> {
  if (!isTauri)
    return {
      ok: true,
      id: 1,
      name: "浏览器预览",
      base_url: "https://api.deepseek.com/v1",
      model: "deepseek-chat",
      has_key: true,
      error: "",
    };
  return invoke("ai_state");
}

export async function aiProfiles(): Promise<AiProfile[]> {
  if (!isTauri) return [];
  return invoke<AiProfile[]>("ai_profiles");
}

export interface ToolLogEntry {
  name: string;
  args: Record<string, unknown>;
  ok: boolean;
  summary: string;
}

export interface ToolInfo {
  name: string;
  label: string;
  description: string;
  risk: string;
  default: boolean;
  on: boolean;
}

/** 带工具的对话：工具审核里开着的工具才会给模型，最多 6 轮 */
export async function aiChatTools(
  messages: ChatMessage[],
  temperature = 0.7,
): Promise<{ text: string; rounds: number; log: ToolLogEntry[]; tools_on: number }> {
  if (!isTauri) {
    const last = messages.filter((one) => one.role === "user").pop();
    return {
      text: `（浏览器预览）你说：${last?.content ?? ""}`,
      rounds: 0,
      log: [],
      tools_on: 0,
    };
  }
  return invoke("ai_chat_tools", { messages, temperature });
}

/** 工具清单（Rust 侧真值，与设置页的开关同源） */
export async function aiToolCatalog(): Promise<ToolInfo[]> {
  if (!isTauri) return [];
  return invoke<ToolInfo[]>("ai_tool_catalog");
}

export async function aiChat(
  messages: ChatMessage[],
  temperature = 0.7,
): Promise<{ ok: boolean; reply: string }> {
  if (!isTauri) {
    const last = messages.filter((one) => one.role === "user").pop();
    return {
      ok: true,
      reply: `（浏览器预览）你说：${last?.content ?? ""}\n\n这里是**示例回复**，桌面版会真的调用模型。行内公式 $a^2+b^2=c^2$，行间：\n\n$$\\int_0^1 x^2\\,dx = \\frac{1}{3}$$`,
    };
  }
  return invoke("ai_chat", { messages, temperature });
}

// ---------------------------------------------------------------- 账户审核（管理员）
export interface AdminKeyState {
  has_private_key: boolean;
  fingerprint: string;
  public_pem: string;
  valid_days: number;
  device_limit: number;
}

export interface RegRequest {
  username: string;
  email: string;
  device?: string;
  nonce?: string;
  ts?: string;
  via?: string;
  already?: boolean;
  expires_at?: string;
}

export interface RegApproval {
  ok?: boolean;
  error?: string;
  reused?: boolean;
  username?: string;
  email?: string;
  code?: string;
  receipt?: string;
  expires_at?: string;
  subject?: string;
  body?: string;
}

export interface RegUser {
  id: number;
  username: string;
  email: string;
  status: string;
  issued_at: string;
  expires_at: string;
  note: string;
  devices: string[];
}

export async function adminKeyState(): Promise<AdminKeyState> {
  if (!isTauri) {
    return {
      has_private_key: false,
      fingerprint: "",
      public_pem: "",
      valid_days: 180,
      device_limit: 3,
    };
  }
  return invoke<AdminKeyState>("admin_key_state");
}

export async function adminGenerateKey(): Promise<{
  ok: boolean;
  fingerprint?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "浏览器预览里不能生成密钥" };
  return invoke("admin_generate_key");
}

export async function registrationRequests(
  limit = 60,
): Promise<{ requests: RegRequest[]; errors: string[] }> {
  if (!isTauri) return { requests: [], errors: [] };
  return invoke("registration_requests", { limit });
}

export async function registrationApprove(
  request: RegRequest,
  days?: number,
): Promise<RegApproval> {
  if (!isTauri) return { ok: false, error: "浏览器预览里不能批准" };
  return invoke<RegApproval>("registration_approve", { request, days });
}

export async function registrationImportEml(
  path: string,
): Promise<{ ok: boolean; request: RegRequest; error?: string }> {
  if (!isTauri) {
    return { ok: false, request: { username: "", email: "" }, error: "预览里不可用" };
  }
  return invoke("registration_import_eml", { path });
}

export async function registrationRegistry(): Promise<RegUser[]> {
  if (!isTauri) return [];
  return invoke<RegUser[]>("registration_registry");
}

export async function registrationAction(
  action: string,
  username: string,
  days?: number,
): Promise<RegApproval> {
  if (!isTauri) return { ok: false, error: "预览里不可用" };
  return invoke<RegApproval>("registration_action", { action, username, days });
}

export type ChatSession = {
  id: number;
  title: string;
  model: string;
  scene: string;
  updated_at: string;
  payload: string;
};

export async function aiProfileSave(
  id: number,
  name: string,
  baseUrl: string,
  model: string,
): Promise<number> {
  if (!isTauri) return id;
  return invoke<number>("ai_profile_save", { id, name, baseUrl, model });
}

export async function aiProfileDelete(id: number): Promise<boolean> {
  if (!isTauri) return true;
  return invoke<boolean>("ai_profile_delete", { id });
}

// ---------------------------------------------------------------- 专注（番茄钟）
export interface FocusState {
  active: boolean;
  label: string;
  minutes: number;
  remaining: number;
  away_seconds: number;
  away_threshold: number;
  away_enabled: boolean;
  over: boolean;
}

export async function focusStart(minutes?: number, label = ""): Promise<{ ok: boolean }> {
  if (!isTauri) return { ok: false };
  return invoke("focus_start", { minutes, label });
}

export async function focusStop(): Promise<{ ok: boolean; away_seconds?: number }> {
  if (!isTauri) return { ok: false };
  return invoke("focus_stop");
}

export async function focusStatus(): Promise<FocusState> {
  if (!isTauri) {
    return {
      active: false,
      label: "",
      minutes: 25,
      remaining: 0,
      away_seconds: 0,
      away_threshold: 180,
      away_enabled: true,
      over: false,
    };
  }
  return invoke<FocusState>("focus_status");
}

export async function aiSessions(limit = 30): Promise<ChatSession[]> {
  if (!isTauri) return [];
  return invoke<ChatSession[]>("ai_sessions", { limit });
}

export async function aiSaveSession(
  title: string,
  model: string,
  payload: string,
): Promise<number> {
  if (!isTauri) return 0;
  return invoke<number>("ai_save_session", { title, model, payload });
}

export type DataInfo = {
  data_dir: string;
  db_path: string;
  db_size: number;
  counts: Record<string, number>;
  version: string;
  kernel: string;
};

export async function dataInfo(): Promise<DataInfo> {
  if (!isTauri)
    return {
      data_dir: "—",
      db_path: "—",
      db_size: 0,
      counts: {},
      version: "1.0.0",
      kernel: "浏览器预览",
    };
  return invoke<DataInfo>("data_info");
}

export async function backupDb(): Promise<{ ok: boolean; path: string; bytes: number; error: string }> {
  if (!isTauri) return { ok: false, path: "", bytes: 0, error: "浏览器预览里不能备份" };
  return invoke("backup_db");
}

/* ------------------------------------------------------ 数据迁移（.dlpk 数据包） */

/** 数据包里 manifest.json 的字段（与 Python 版**完全一致**，别单方面改名） */
export type BackupManifest = {
  format: string;
  format_version: number;
  app: string;
  app_version: string;
  exported_at: string;
  schema_version: number;
  tables: Record<string, number>;
  secrets_stripped: boolean;
  stripped_keys: string[];
};

export type DataExportResult = {
  ok: boolean;
  path: string;
  bytes: number;
  manifest?: BackupManifest;
  tables: Record<string, number>;
  stripped_keys: string[];
  error: string;
};

/** 导出数据包：`path` 是用户选的 .dlpk 路径（zip） */
export async function dataExport(path: string): Promise<DataExportResult> {
  if (!isTauri)
    return {
      ok: false,
      path: "",
      bytes: 0,
      tables: {},
      stripped_keys: [],
      error: "浏览器预览里不能导出数据包",
    };
  return invoke<DataExportResult>("data_export", { path });
}

export type DataImportResult = {
  ok: boolean;
  needs_restart: boolean;
  /** 导入前自动备份的旧库路径 */
  backup: string;
  /** 解出来的待换库文件（data/lumina.db.imported） */
  imported?: string;
  tables: Record<string, number>;
  stripped_keys: string[];
  app?: string;
  app_version?: string;
  exported_at?: string;
  error: string;
};

/** 从数据包导入：先备份、再展开成 lumina.db.imported，下次启动才真正换库 */
export async function dataImport(path: string): Promise<DataImportResult> {
  if (!isTauri)
    return {
      ok: false,
      needs_restart: false,
      backup: "",
      tables: {},
      stripped_keys: [],
      error: "浏览器预览里不能导入数据包",
    };
  return invoke<DataImportResult>("data_import", { path });
}

/** 重新启动本程序（导入数据后「立即重启」用） */
export async function appRelaunch(): Promise<{ ok: boolean; error: string }> {
  if (!isTauri) return { ok: false, error: "浏览器预览里不能重启" };
  return invoke<{ ok: boolean; error: string }>("app_relaunch");
}

/* ---------------------------------------------------------------- 提醒 */

export type Reminder = {
  id: number;
  title: string;
  date: string;
  start_time: string;
  end_time: string;
  remind_at: string;
};

export async function dueReminders(): Promise<Reminder[]> {
  if (!isTauri) return [];
  return invoke<Reminder[]>("due_reminders");
}

export async function markNotified(id: number): Promise<void> {
  if (!isTauri) return;
  await invoke("mark_notified", { id });
}

export async function snoozeEvent(id: number, minutes = 10): Promise<string> {
  if (!isTauri) return "";
  return invoke<string>("snooze_event", { id, minutes });
}

/** 兜底：把页面原文粘进来，交给 Rust 解析并写缓存 */
export async function ehallIngestPasted(
  key: string,
  text: string,
): Promise<{ ok: boolean; value: string; error: string }> {
  if (!isTauri) return { ok: false, value: "", error: "浏览器预览里不能解析" };
  return invoke("ehall_ingest_pasted", { key, text });
}

/* ------------------------------------------------------------ 文件选择 */

/** 打开原生「选文件」对话框；取消返回空数组 */
export async function pickFiles(
  filters?: { name: string; extensions: string[] }[],
  multiple = false,
): Promise<string[]> {
  if (!isTauri) return [];
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple,
    directory: false,
    filters: filters && filters.length ? filters : undefined,
  });
  if (!picked) return [];
  return Array.isArray(picked) ? picked : [picked];
}

/** 打开原生「另存为」对话框；取消返回空字符串 */
export async function pickSavePath(
  defaultName: string,
  filters?: { name: string; extensions: string[] }[],
): Promise<string> {
  if (!isTauri) return "";
  const { save } = await import("@tauri-apps/plugin-dialog");
  const picked = await save({
    defaultPath: defaultName,
    filters: filters && filters.length ? filters : undefined,
  });
  return picked || "";
}

/* ------------------------------------------------------------ 公式导出 */

export async function latexPdf(
  body: string,
  name?: string,
): Promise<{ ok: boolean; path: string; error: string }> {
  if (!isTauri)
    return { ok: false, path: "", error: "浏览器预览里不能编译 PDF" };
  return invoke("latex_pdf", { body, name: name ?? null });
}

/* ---------------------------------------------------------------- 窗口 */

export async function windowMinimize(): Promise<void> {
  if (!isTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().minimize();
}

export async function windowToggleMaximize(): Promise<void> {
  if (!isTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().toggleMaximize();
}

export async function windowClose(): Promise<void> {
  if (!isTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().close();
}

// ---------------------------------------------------------------- UIS / CAS login
export interface UisState {
  username: string;
  has_session: boolean;
  cookies: number;
  has_castgc: boolean;
  remember_password: boolean;
  has_saved_password: boolean;
}

export async function uisStatus(): Promise<UisState> {
  if (!isTauri) {
    return {
      username: "",
      has_session: false,
      cookies: 0,
      has_castgc: false,
      remember_password: false,
      has_saved_password: false,
    };
  }
  return invoke<UisState>("uis_status");
}

export async function uisLogin(
  username: string,
  password: string,
): Promise<{ ok: boolean; username?: string; cookies?: number; has_castgc?: boolean; error?: string }> {
  if (!isTauri) return { ok: false, error: "browser preview cannot login" };
  return invoke("uis_login", { username, password });
}

export async function uisLogout(): Promise<{ ok: boolean }> {
  if (!isTauri) return { ok: false };
  return invoke("uis_logout");
}

export async function uisOpenLogin(): Promise<{ ok: boolean; error?: string }> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  try {
    await invoke("uis_open_login");
    return { ok: true };
  } catch (err) {
    return { ok: false, error: String(err) };
  }
}

export async function uisCapture(): Promise<{
  ok: boolean;
  cookies?: number;
  names?: string[];
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("uis_capture");
}

export async function jwglProbe(semester?: string): Promise<{
  ok: boolean;
  url?: string;
  shape?: string;
  bytes?: number;
  top_keys?: string[];
  head?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("jwgl_probe", { semester });
}

export async function canvasProbe(): Promise<{
  ok: boolean;
  courses?: number;
  names?: string[];
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("canvas_probe");
}

export async function canvasDownload(
  courseId: string,
  limit = 40,
): Promise<{
  ok: boolean;
  /** 这次用的课程目录名（已做文件名安全化） */
  course?: string;
  saved?: number;
  skipped?: number;
  failed?: number;
  files?: string[];
  target?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("canvas_download", { courseId, limit });
}

export async function canvasMigrate(): Promise<{
  ok: boolean;
  /** [旧文件名, 课程名] */
  moved?: [string, string][];
  /** 目标已有一份同样大小的，根目录那份被删掉 */
  deduped?: string[];
  /** 判断不出归属、留在下载目录根下的文件 */
  unresolved?: string[];
  /** [文件名, 原因] */
  failed?: [string, string][];
  /** 这次用的下载基准目录 */
  root?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("canvas_migrate");
}

export interface EhallRefreshResult {
  ok: boolean;
  refreshed?: number;
  total?: number;
  need_login?: boolean;
  cards?: { key: string; title: string; ok: boolean; value?: string; error?: string }[];
}

export async function ehallRefreshOnline(): Promise<EhallRefreshResult> {
  if (!isTauri) return { ok: false };
  return invoke("ehall_refresh_online");
}

export async function papersAutoSummary(limit = 5): Promise<{
  ok: boolean;
  done?: number;
  failed?: number;
  errors?: string[];
  message?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("papers_auto_summary", { limit });
}

/* ------------------------------------------------ 账号服务器（自家 FastAPI）
 *
 * Rust 侧的 rejection 不是字符串，而是一个带 `kind` 的 JSON 对象（见 account.rs 的 ApiError）。
 * 前端统一用 asAccountError() 归一化，再按 kind 分支：
 *   offline      连不上服务器（显示「服务器暂不可用」+ 改为邮件申请）
 *   unauthorized 401/403 凭据错、注册关闭
 *   device_limit 409 设备超限（devices 里有设备列表，可以直接画出来让用户踢）
 *   unsupported  服务器没这个接口（老版本没有 /announcements）
 *   server/config 其它
 * 邮箱验证码另外几类（`code` 里是服务器的原始机器码）：
 *   cooldown      429 email_cooldown / too_many_requests -> retry_after 秒后重试
 *   email         503 email_send_failed / email_not_configured -> 服务器发不出信
 *   invalid_email 400 invalid_email / email_domain_not_allowed
 *   invalid_code  400 invalid_code / code_expired -> attempts_left 次机会
 */

export type ServerDevice = {
  id: number;
  name: string;
  platform: string;
  last_seen_at: string;
};

export type AccountError = {
  ok: false;
  kind: string;
  /** 服务器原始的 error 机器码（email_cooldown / invalid_code…）；本地校验失败时为空串 */
  code: string;
  error: string;
  http_status: number;
  offline: boolean;
  devices: ServerDevice[];
  devices_used: number;
  device_limit: number;
  /** 429 冷却：还要等多少秒（没有就是 0） */
  retry_after: number;
  /** 400 验证码错 / 过期：还能再试几次（兼容 attempts_left / remaining_attempts，没有就是 0） */
  attempts_left: number;
  body: string;
};

/** 把 invoke 抛出来的各种形状（对象 / 字符串）统一成 AccountError */
export function asAccountError(err: unknown): AccountError {
  const fallback: AccountError = {
    ok: false,
    kind: "other",
    code: "",
    error: String(err ?? "未知错误"),
    http_status: 0,
    offline: false,
    devices: [],
    devices_used: 0,
    device_limit: 0,
    retry_after: 0,
    attempts_left: 0,
    body: "",
  };
  if (err && typeof err === "object") {
    const raw = err as Record<string, unknown>;
    return {
      ok: false,
      kind: typeof raw.kind === "string" ? raw.kind : "other",
      code: typeof raw.code === "string" ? raw.code : "",
      error:
        typeof raw.error === "string" && raw.error
          ? raw.error
          : fallback.error,
      http_status: Number(raw.http_status || 0),
      offline: raw.offline === true || raw.kind === "offline",
      devices: Array.isArray(raw.devices) ? (raw.devices as ServerDevice[]) : [],
      devices_used: Number(raw.devices_used || 0),
      device_limit: Number(raw.device_limit || 0),
      retry_after: Number(raw.retry_after || 0),
      // 服务端实际字段是 remaining_attempts（见 account.rs 同名兜底）
      attempts_left: Number(raw.attempts_left ?? raw.remaining_attempts ?? 0),
      body: typeof raw.body === "string" ? raw.body : "",
    };
  }
  return fallback;
}

export type NotifyStatus = {
  mode: string;
  reason: string;
  interval_secs: number;
  max_backoff_secs: number;
  started: boolean;
  polls: number;
  delivered: number;
  last_poll_at: string;
  last_ok_at: string;
  last_error: string;
  last_error_kind: string;
  unsupported: boolean;
  backoff_secs: number;
  next_delay_secs: number;
  event: string;
};

export type AccountState = {
  server_url: string;
  default_server: string;
  enabled: boolean;
  logged_in: boolean;
  username: string;
  account_id: string;
  device_fingerprint: string;
  token_expires_at: string;
  has_refresh_token: boolean;
  /** 上次登录响应里的设备数 / 设备上限（服务器说了算） */
  devices_used: number;
  device_limit: number;
  last_announcement_id: string;
  announcements_unread: number;
  notify: NotifyStatus;
};

export type AccountSession = {
  ok?: boolean;
  logged_in?: boolean;
  server_url?: string;
  username?: string;
  account_id?: string;
  device_fingerprint?: string;
  token?: string;
  refresh_token?: string;
  expires_in?: number;
  devices_used?: number;
  device_limit?: number;
  account?: { id?: number | string; username?: string; email?: string };
  [key: string]: unknown;
};

export type ServerAnnouncement = {
  id: number;
  title: string;
  body: string;
  /** info / warning / critical */
  level: string;
  pinned: boolean;
  created_at: string;
  expires_at: string;
  received_at: string;
  read_at: string | null;
};

export type HealthResult = {
  ok: boolean;
  online: boolean;
  version: string;
  uptime_seconds: number;
  latency_ms: number;
  url: string;
};

const MOCK_ACCOUNT_STATE: AccountState = {
  server_url: "https://43.108.99.38.sslip.io",
  default_server: "https://43.108.99.38.sslip.io",
  enabled: true,
  logged_in: false,
  username: "",
  account_id: "",
  device_fingerprint: "00000000-0000-4000-8000-000000000000",
  token_expires_at: "",
  has_refresh_token: false,
  devices_used: 0,
  device_limit: 0,
  last_announcement_id: "0",
  announcements_unread: 0,
  notify: {
    mode: "polling",
    reason: "浏览器预览",
    interval_secs: 20,
    max_backoff_secs: 60,
    started: false,
    polls: 0,
    delivered: 0,
    last_poll_at: "",
    last_ok_at: "",
    last_error: "",
    last_error_kind: "",
    unsupported: false,
    backoff_secs: 0,
    next_delay_secs: 0,
    event: "danlume://announcement",
  },
};

/** 探测服务器：设置页「测试连接」 */
export async function accountHealth(): Promise<HealthResult> {
  if (!isTauri) {
    return {
      ok: false,
      online: false,
      version: "",
      uptime_seconds: 0,
      latency_ms: 0,
      url: MOCK_ACCOUNT_STATE.server_url,
    };
  }
  return invoke<HealthResult>("account_health");
}

export type EmailCodeResult = {
  ok: boolean;
  email: string;
  /** 验证码有效期（秒），服务器默认 600 */
  expires_in: number;
  /** 多少秒后能再要一封（服务器默认 60，前端拿它做倒计时） */
  cooldown: number;
  raw?: Record<string, unknown>;
};

/**
 * 注册前要一封邮箱验证码：`POST /api/v1/register/request-code`。
 *
 * 失败时抛的 rejection 用 asAccountError() 归一化：
 * 429 -> kind `cooldown`（retry_after 秒后重试）、503 -> kind `email`（服务器发不出信）、
 * 400 -> kind `invalid_email`（邮箱格式 / 域名白名单）。
 */
export async function requestEmailCode(email: string): Promise<EmailCodeResult> {
  if (!isTauri) {
    // 浏览器预览：假装发信成功，倒计时照常走
    return { ok: true, email: email.trim(), expires_in: 600, cooldown: 60 };
  }
  return invoke<EmailCodeResult>("account_request_email_code", { email });
}

export async function accountRegister(
  username: string,
  email: string,
  password: string,
  inviteCode = "",
  code = "",
): Promise<AccountSession> {
  if (!isTauri) {
    return { ok: true, logged_in: true, username, account_id: "1" };
  }
  return invoke<AccountSession>("account_register", {
    username,
    email,
    password,
    inviteCode,
    code,
  });
}

export async function accountLogin(
  usernameOrEmail: string,
  password: string,
): Promise<AccountSession> {
  if (!isTauri) {
    return {
      ok: true,
      logged_in: true,
      username: usernameOrEmail,
      account_id: "1",
    };
  }
  return invoke<AccountSession>("account_login", {
    usernameOrEmail,
    password,
  });
}

export async function accountLogout(): Promise<{ ok: boolean; username?: string }> {
  if (!isTauri) return { ok: true };
  return invoke("account_logout");
}

export async function accountMe(): Promise<Record<string, unknown>> {
  if (!isTauri) return {};
  return invoke("account_me");
}

/** 设备列表：`{devices:[{id,name,platform,last_seen_at}], devices_used?, device_limit?}` */
export async function accountDevices(): Promise<{
  devices?: ServerDevice[];
  devices_used?: number;
  device_limit?: number;
  [key: string]: unknown;
}> {
  if (!isTauri) return { devices: [] };
  return invoke("account_devices");
}

export async function accountKickDevice(deviceId: number): Promise<{ ok?: boolean }> {
  if (!isTauri) return { ok: true };
  return invoke("account_kick_device", { deviceId });
}

export async function accountDeleteMe(): Promise<{ ok?: boolean }> {
  if (!isTauri) return { ok: true };
  return invoke("account_delete_me");
}

export async function accountState(): Promise<AccountState> {
  if (!isTauri) return { ...MOCK_ACCOUNT_STATE };
  return invoke<AccountState>("account_state");
}

/** 保存服务器地址（空字符串 = 不用服务器），Rust 侧会让公告轮询按新地址重建 */
export async function accountSaveServer(
  url: string,
): Promise<{ ok: boolean; server_url: string }> {
  if (!isTauri) {
    MOCK_ACCOUNT_STATE.server_url = url.trim().replace(/\/+$/, "");
    return { ok: true, server_url: MOCK_ACCOUNT_STATE.server_url };
  }
  return invoke("account_save_server", { url });
}

/** 本地存着的服务器公告（标题栏徽标 / 通知页） */
export async function accountAnnouncements(limit = 100): Promise<{
  ok: boolean;
  items: ServerAnnouncement[];
  total: number;
  unread: number;
  last_id: string;
  notify?: NotifyStatus;
}> {
  if (!isTauri) {
    return { ok: true, items: [], total: 0, unread: 0, last_id: "0" };
  }
  return invoke("account_announcements", { limit });
}

/** 标为已读：不传 id = 全部 */
export async function accountMarkAnnouncementsRead(
  id?: number,
): Promise<{ ok: boolean; changed: number; unread: number }> {
  if (!isTauri) return { ok: true, changed: 0, unread: 0 };
  return invoke("account_mark_announcements_read", { id: id ?? null });
}

/** 立刻拉一次公告（不等下一轮轮询） */
export async function accountCheckAnnouncements(): Promise<{
  ok: boolean;
  added: number;
  since: number;
  last_announcement_id?: number;
  items?: ServerAnnouncement[];
  unread?: number;
  kind?: string;
  offline?: boolean;
  error?: string;
  http_status?: number;
}> {
  if (!isTauri) {
    return { ok: false, added: 0, since: 0, error: "浏览器预览不支持" };
  }
  return invoke("account_check_announcements");
}

export async function accountNotifyStatus(): Promise<NotifyStatus> {
  if (!isTauri) return { ...MOCK_ACCOUNT_STATE.notify };
  return invoke<NotifyStatus>("account_notify_status");
}

/**
 * 订阅服务器公告事件（Rust 侧 `danlume://announcement`）。
 *
 * 万一这个环境的 capability 不允许 `core:event:listen`，这里会安静地失败，
 * 调用方应该再挂一个 30 秒的 `accountAnnouncements()` 轮询兜底（组件里就是这么做的）。
 */
export async function onAnnouncement(
  handler: (item: ServerAnnouncement) => void,
): Promise<() => void> {
  if (!isTauri) return () => {};
  try {
    const { listen } = await import("@tauri-apps/api/event");
    const unlisten = await listen<ServerAnnouncement>(
      "danlume://announcement",
      (event) => handler(event.payload),
    );
    return () => {
      try {
        unlisten();
      } catch {
        // 卸载失败无所谓
      }
    };
  } catch (err) {
    console.warn("订阅公告事件失败（改为轮询兜底）", err);
    return () => {};
  }
}

