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
  /** 重复日程里被跳过的那几天（逗号分隔） */
  repeat_skip?: string;
  /** 重复到哪一天为止（空 = 一直重复） */
  repeat_until?: string;
  /** 重复日程里已经完成的那几次（逗号分隔日期） */
  done_dates?: string;
  /** 展开出来的一次重复：母事件 id（母事件本身那一天没有这个字段） */
  repeat_of?: number | null;
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
  patch: Partial<EventRow> & { clear_remind?: boolean },
): Promise<void> {
  if (!isTauri) {
    const target = MOCK_EVENTS.find((e) => e.id === id);
    if (target) Object.assign(target, patch);
    return;
  }
  await invoke("update_event", { id, patch });
}

/**
 * 标记完成 / 取消完成。
 * `occurrenceDate` 是「哪一天的那一次」（重复日程只影响那一次，不会把整条系列
 * 都标成已完成——这是用户反馈的那个 bug）。
 */
export async function eventSetDone(
  id: number,
  done: boolean,
  occurrenceDate?: string,
): Promise<boolean> {
  if (!isTauri) {
    const target = MOCK_EVENTS.find((e) => e.id === id);
    if (target) {
      if (target.repeat) {
        const days = new Set((target.done_dates || "").split(",").filter(Boolean));
        const day = occurrenceDate || target.date;
        if (done) days.add(day);
        else days.delete(day);
        target.done_dates = [...days].sort().join(",");
      } else {
        target.done = done;
      }
    }
    return true;
  }
  return invoke<boolean>("event_set_done", {
    id,
    done,
    occurrenceDate: occurrenceDate ?? null,
  });
}

/** 重复日程：只删这一次（记进 repeat_skip，母事件与以后的重复都不动） */
export async function skipEventOccurrence(
  id: number,
  date: string,
): Promise<{ ok: boolean; error?: string; skipped?: string[] }> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("event_skip_occurrence", { id, date });
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
): Promise<{ ok: boolean; items: MailSummary[]; error: string; cached?: number }> {
  if (!isTauri) return { ok: true, items: MOCK_MAILS, error: "" };
  return invoke("mail_list", { limit });
}

/** 增量同步收件箱：只取服务器上 UID 更大的新邮件（旧版 uid 缓存做法） */
export async function mailSync(
  limit = 50,
): Promise<{ ok: boolean; added?: number; fetched?: number; cached?: number; error?: string }> {
  if (!isTauri) return { ok: true, added: 0 };
  return invoke("mail_sync", { limit });
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

/** 摘要顺手抽出来的「可能要加进日程的事」（前端画成可勾选的圆圈） */
export type MailEventSuggestion = {
  title: string;
  date: string;
  start_time: string;
  end_time: string;
  note: string;
};

/**
 * 邮件顶端的 AI 摘要：走后端缓存的 `summary_zh`，同一封邮件只花一次 token。
 * `force = true` 是「重新生成」；没配接口时返回 `ok:false` + 中文提示。
 */
export async function mailSummarize(
  uid: string,
  force = false,
): Promise<{
  ok: boolean;
  summary: string;
  cached: boolean;
  error: string;
  events?: MailEventSuggestion[];
  model?: string;
  /** 钓鱼 / 风险判断（摘要最后一行「风险：低|中|高 —— 理由」解析出来的） */
  risk?: { level: "" | "low" | "medium" | "high"; reason: string };
}> {
  if (!isTauri)
    return {
      ok: true,
      summary: "· 这是浏览器预览里的示例摘要，真的跑起来会调用你在 AI 页配的接口。",
      cached: false,
      error: "",
      events: [
        { title: "提交材料", date: "2026-11-18", start_time: "12:00", end_time: "", note: "示例" },
      ],
      model: "demo",
    };
  return invoke("mail_summarize", { uid, force });
}

/** 把勾选出来的「可能要加进日程的事」真的加进日历（走 AI 的 schedule_add 工具） */
export async function mailAddEvents(
  items: MailEventSuggestion[],
): Promise<{ ok: boolean; added: number; results: unknown[]; error: string }> {
  if (!isTauri) return { ok: true, added: items.length, results: [], error: "" };
  return invoke("mail_add_events", { items });
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

/** 真去 eLearning 拉一次 To Do（旧版 `elearning.todo_items` + `dashboard.save_assignments`） */
export async function homeworkSync(): Promise<{
  ok: boolean;
  items?: { title: string; course: string; due: string; due_at?: string; url?: string; kind?: string }[];
  at?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("homework_sync");
}

/** 下载目录里还没归类的文件 + 可选的课程目录（用户手动归类） */
export type Unclassified = {
  ok: boolean;
  root: string;
  files: { name: string; size: number }[];
  courses: string[];
};

export async function homeworkUnclassified(): Promise<Unclassified> {
  if (!isTauri) return { ok: true, root: "", files: [], courses: [] };
  return invoke<Unclassified>("homework_unclassified");
}

/** 把根目录里的文件挪进指定课程目录 */
export async function homeworkClassify(
  path: string,
  course: string,
): Promise<{ ok: boolean; moved: boolean; deduped?: boolean; target?: string; error?: string }> {
  if (!isTauri) return { ok: false, moved: false, error: "browser preview" };
  return invoke("homework_classify", { path, course });
}

/** 文献在线检索（arXiv + Crossref）与入库（对应旧版 `lit.search_all` / `add_paper`） */
export type PaperSearchHit = {
  source: string;
  source_id: string;
  title: string;
  authors: string;
  year: string;
  venue: string;
  abstract?: string;
  url?: string;
  pdf_url?: string;
  doi?: string;
};

export type PaperSearchPage = {
  ok: boolean;
  results: PaperSearchHit[];
  errors?: string[];
  /** 每个来源这一页返回了多少条（过滤之后） */
  counts?: { arxiv: number; crossref: number; semantic?: number };
  /** 每个来源这一页**取回**了多少条（过滤之前，用来判断还能不能翻） */
  raw?: { arxiv: number; crossref: number; semantic?: number };
  /** Crossref / Semantic Scholar 报的「一共多少条」（arXiv 也报，但常常是 0） */
  totals?: { arxiv: number; crossref: number; semantic?: number };
  /** 这次检索实际要求出现的关键词（多词时按 AND 过滤） */
  terms?: string[];
  /**
   * 严格匹配（每个词都要出现）一条都没有时，后端会自动放宽成「任一关键词命中」
   * 并按命中词数排序；这时 `relaxed = true`。
   */
  relaxed?: boolean;
  offset?: number;
  limit?: number;
  /** 还有下一页（某个来源这一页拿满了） */
  more?: boolean;
};

/** 检索结果里额外带的「命中词数」字段 */
export type PaperSearchHitWithCoverage = PaperSearchHit & {
  matched_terms?: number;
  total_terms?: number;
};

/**
 * 在线检索（arXiv + Crossref）。`offset` 用来「再看下一页」：
 * 以前固定只取 10 条/来源，搜 MRI 也只有 20 条，用户直接质疑「怎么可能只有 20 条」。
 */
export async function papersSearch(
  query: string,
  limit = 20,
  offset = 0,
): Promise<PaperSearchPage> {
  if (!isTauri) return { ok: false, results: [] };
  return invoke<PaperSearchPage>("papers_search", { query, limit, offset });
}

export async function papersAdd(
  item: PaperSearchHit,
  project?: string,
): Promise<{ ok: boolean; id?: number; error?: string }> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("papers_add", { item, project });
}

/* ---------------------------------------------------------------- 按意思搜（语义检索） */
/*
 * 分工：服务器只把文字算成 512 维向量（B 方案：模型跑服务器本机，不花 API 钱），
 * 向量存在本地 `paper_vectors` 表，检索时在本地算余弦 —— 每次搜索只有问句那一句联网。
 */
export type SemanticStatus = {
  ok: boolean;
  indexed: number;
  papers: number;
  need_index: boolean;
  logged_in: boolean;
};

export type SemanticHit = {
  id: number;
  title: string;
  authors: string;
  year: string;
  venue: string;
  file_path: string;
  /** 余弦相似度（0–1，越大越接近） */
  score: number;
};

export async function papersSemanticStatus(): Promise<SemanticStatus> {
  if (!isTauri) return { ok: false, indexed: 0, papers: 0, need_index: false, logged_in: false };
  return invoke<SemanticStatus>("papers_semantic_status");
}

export async function papersSemanticIndex(force = false): Promise<{
  ok: boolean;
  indexed?: number;
  total?: number;
  dim?: number;
  model?: string;
  message?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("papers_semantic_index", { force });
}

export async function papersSemanticSearch(
  query: string,
  limit = 10,
): Promise<{
  ok: boolean;
  hits?: SemanticHit[];
  indexed?: number;
  /** 有多少篇的向量是「旧模型」建的（维度对不上，已被跳过） */
  stale?: number;
  note?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("papers_semantic_search", { query, limit });
}

/**
 * 把这篇文献的 PDF 下到文献库目录（`library/<项目>/<年份>/<标题>.pdf`）。
 * 对应旧版 `lit.download_pdf`：已经有文件就直接返回，不是 `%PDF` 就报「要登录」。
 */
export async function papersDownload(
  id: number,
): Promise<{ ok: boolean; path?: string; folder?: string; bytes?: number; summary: string }> {
  if (!isTauri) return { ok: false, summary: "浏览器预览里不能下载" };
  return invoke("papers_download", { id });
}

/** 参考文献（BibTeX / RIS / DOI）：对应旧版 `refs.py` */
/* ---------------------------------------------------------------- 学生工作（通讯录 / 定时通知） */

export type Contact = {
  id: number;
  name: string;
  title?: string;
  email?: string;
  phone?: string;
  /** 你在微信里给这个人的备注名 */
  wechat?: string;
  qq?: string;
  /** 其它联系方式（飞书 / 钉钉 / webhook…） */
  extra?: string;
  note?: string;
  updated_at?: string;
};

export async function contactsList(keyword = ""): Promise<Contact[]> {
  if (!isTauri) return [];
  return invoke<Contact[]>("contacts_list", { keyword });
}

export async function contactsSave(item: Partial<Contact>): Promise<number> {
  if (!isTauri) return 0;
  return invoke<number>("contacts_save", { item });
}

export async function contactsDelete(id: number): Promise<void> {
  if (!isTauri) return;
  await invoke("contacts_delete", { id });
}

/** AI 能不能读通讯录（默认关；勾选状态存在设置里） */
export async function contactsAiAllowed(): Promise<boolean> {
  if (!isTauri) return false;
  return invoke<boolean>("contacts_ai_allowed");
}

export async function contactsSetAiAllowed(on: boolean): Promise<boolean> {
  if (!isTauri) return on;
  return invoke<boolean>("contacts_set_ai_allowed", { on });
}

/* ---------------------------------------------------------------- 看板娘悬浮球 */

export async function mascotOpen(): Promise<void> {
  if (!isTauri) return;
  await invoke("mascot_open");
}

export async function mascotClose(): Promise<void> {
  if (!isTauri) return;
  await invoke("mascot_close");
}

/** 悬浮球收到的东西交给主窗口的 AI 面板（文件走路径，文字走 text） */
export async function mascotSend(text: string, paths: string[] = []): Promise<void> {
  if (!isTauri) return;
  await invoke("mascot_send", { text, paths });
}

/** 主窗口这边监听悬浮球发来的内容 */
export async function onMascotSend(
  handler: (payload: { text: string; paths: string[] }) => void,
): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ text: string; paths: string[] }>("mascot:send", (event) =>
    handler(event.payload),
  );
}

/**
 * 读当前微信聊天（J8-4，对应旧版 `wechat_read.py`）：
 * 截图 → Windows 自带 OCR → 清洗；直读失败会自动退回剪贴板。
 * `useClipboard = true` 时直接读剪贴板（用户自己右键复制了消息）。
 */
export async function wechatRead(
  useClipboard = false,
): Promise<{ ok: boolean; text?: string; how?: string; warning?: string; error?: string }> {
  if (!isTauri) return { ok: false, error: "浏览器预览里读不了微信" };
  return invoke("wechat_read", { useClipboard });
}

/** 定时通知的收件人：可以是通讯录里的人，也可以直接填地址 */
export type NoticeTarget = { type: "contact" | "raw"; id?: number; address?: string; label?: string };

export type StudentNotice = {
  id: number;
  title: string;
  body: string;
  /** email / webhook / wechat */
  channel: string;
  targets: NoticeTarget[];
  send_at: string;
  /** "" / daily / weekly */
  repeat: string;
  status: string;
  last_error: string;
  last_sent_at: string;
};

export async function studentNotices(): Promise<StudentNotice[]> {
  if (!isTauri) return [];
  return invoke<StudentNotice[]>("student_notices");
}

export async function studentNoticeSave(item: Partial<StudentNotice>): Promise<number> {
  if (!isTauri) return 0;
  return invoke<number>("student_notice_save", { item });
}

export async function studentNoticeDelete(id: number): Promise<void> {
  if (!isTauri) return;
  await invoke("student_notice_delete", { id });
}

export async function studentNoticeSend(
  id: number,
): Promise<{ ok: boolean; sent: number; error: string }> {
  if (!isTauri) return { ok: true, sent: 1, error: "" };
  return invoke("student_notice_send", { id });
}

export type RefEntry = {
  id: number;
  kind: string;
  title: string;
  authors: string;
  year: string;
  venue: string;
  doi: string;
  url: string;
  note: string;
  created_at?: string;
};

export async function refsList(limit = 200): Promise<{ ok: boolean; items: RefEntry[]; total: number }> {
  if (!isTauri) return { ok: false, items: [], total: 0 };
  return invoke("refs_list", { limit });
}

export async function refsImport(input: {
  text?: string;
  path?: string;
  format?: "auto" | "bibtex" | "ris";
}): Promise<{ ok: boolean; parsed?: number; added?: number; skipped?: number; error?: string }> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("refs_import", input);
}

export async function refsAddDoi(doi: string): Promise<{
  ok: boolean;
  added?: number;
  entry?: RefEntry;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("refs_add_doi", { doi });
}

export async function refsDelete(id: number): Promise<{ ok: boolean }> {
  if (!isTauri) return { ok: false };
  return invoke("refs_delete", { id });
}

export async function refsExport(
  format: "ris" | "bibtex" | "enw" = "ris",
  path?: string,
): Promise<{ ok: boolean; path?: string; error?: string }> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("refs_export", { format, path });
}

/** EndNote 装没装（旧版 `endnote.detect()`：注册表 + 常见安装目录） */
export type EndnoteState = { found: boolean; path: string; summary: string };

export async function endnoteDetect(): Promise<EndnoteState> {
  if (!isTauri)
    return { found: false, path: "", summary: "浏览器预览里查不了本机装没装 EndNote" };
  return invoke<EndnoteState>("endnote_detect");
}

/** 用 EndNote 打开刚导出的 .enw / .ris；没装返回 `{ok:false, found:false}` */
export async function endnoteOpen(
  path: string,
): Promise<{ ok: boolean; found: boolean; error: string }> {
  if (!isTauri) return { ok: false, found: false, error: "浏览器预览里打不开" };
  return invoke("endnote_open", { path });
}

/** 退信识别：扫收件箱里的退信，把发件箱对应邮件标成 bounced（旧版 `bounce.py`） */
export async function bounceSync(): Promise<{
  ok: boolean;
  scanned?: number;
  marked?: { mail_id: number; to: string; reason: string; code: string }[];
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("bounce_sync");
}

/** 重要日（倒计时）：对应旧版 `important.py`，Rust 以前只有表没有界面 */
export type ImportantDate = {
  id: number;
  title: string;
  date: string;
  note: string;
  color: string;
  kind: string;
  days: number;
  left: string;
};

export async function importantList(): Promise<{ ok: boolean; items: ImportantDate[] }> {
  if (!isTauri) return { ok: false, items: [] };
  return invoke("important_list");
}

export async function importantSave(input: {
  id?: number;
  title: string;
  date: string;
  note?: string;
  color?: string;
}): Promise<{ ok: boolean; date?: string; error?: string }> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("important_save", input);
}

export async function importantDelete(id: number): Promise<{ ok: boolean; error?: string }> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("important_delete", { id });
}

export async function homeworkWriteDeadlines(
  force = true,
): Promise<{ ok: boolean; added: number; updated: number; skipped: number; error: string }> {
  if (!isTauri)
    return { ok: false, added: 0, updated: 0, skipped: 0, error: "浏览器预览里不能写日程" };
  return invoke("homework_write_deadlines", { force });
}

/* -------------------------------------------------------------- 作业截止（日历页 + 当天提醒） */
/*
 * 数据来自 eLearning 的 `/dash`（Canvas 的 To Do），由「同步」写进本机缓存
 * `homework_assignments`；这里读缓存，不联网。
 */
export type AssignmentDue = {
  /** 「课程|标题|截止」：标记「已完成」时用它 */
  key: string;
  title: string;
  course: string;
  /** "YYYY-MM-DD HH:MM"（本地时间） */
  due: string;
  day: string;
  clock: string;
  /** 还差几天：0 = 今天，负数 = 已经过了 */
  days_left: number;
  /** 还差多少分钟（负数 = 已经过了；提醒按这个算，<1440 就提醒） */
  minutes_left: number;
  /** 「还有 5 小时」/「还有 2 天」/「已过 30 分钟」这种人话 */
  left_text: string;
  overdue: boolean;
  url: string;
  kind: string;
};

export async function homeworkUpcoming(days = 14): Promise<{
  ok: boolean;
  items: AssignmentDue[];
  total: number;
  synced_at: string;
}> {
  if (!isTauri) return { ok: true, items: [], total: 0, synced_at: "" };
  return invoke("homework_upcoming", { days });
}

/**
 * 把一条作业标成「已完成 / 已提交」（`done=false` 取消）。
 * 标了之后卡片里不再出现，也不会再提醒（用户 2026-11-12）。
 */
export async function homeworkMarkDone(
  key: string,
  done = true,
): Promise<{ ok: boolean; done_keys: string[] }> {
  if (!isTauri) return { ok: false, done_keys: [] };
  return invoke("homework_mark_done", { key, done });
}

/** 今天该提醒的作业（`mark=true` 记成「今天提过」，默认只用来看） */
export async function elearnDueReminders(mark = false): Promise<{
  ok: boolean;
  items: DueAssignment[];
  /** 提前多少分钟开始提醒（现在是 24 小时） */
  ahead_minutes: number;
}> {
  if (!isTauri) return { ok: true, items: [], ahead_minutes: 1440 };
  return invoke("elearn_due_reminders", { mark });
}

/** 快到截止时间的一条（Rust 说「进入 24 小时窗口」了） */
export type DueAssignment = {
  key: string;
  title: string;
  course: string;
  clock: string;
  /** "YYYY-MM-DD HH:MM" */
  due: string;
  minutes_left: number;
  left_text: string;
  url: string;
  /** 直接给卡片用的一句话：「课程 · 作业 还有 5 小时（11-12 23:59 截止）」 */
  text: string;
};

/* -------------------------------------------------------------- 考试安排（J2-1） */
/*
 * 教务「我的考试」是统一身份认证 + SPA：在登录窗口里落到考试页，把页面已经渲染出来的
 * 表格读回来再解析（对应旧版 exams.py / exam_dialog.py）。
 */
export type ExamItem = {
  title: string;
  /** "YYYY-MM-DD" */
  date: string;
  start_time: string;
  end_time: string;
  location: string;
  seat: string;
  note: string;
  /** "table" = 从表格读的，"text" = 扫正文兜底 */
  confidence: string;
};

export async function examsImport(
  commit = false,
  asImportant = true,
): Promise<{
  ok: boolean;
  items?: ExamItem[];
  committed?: boolean;
  url?: string;
  title?: string;
  result?: { ok: boolean; added: number; skipped: number };
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "浏览器预览里读不了教务" };
  return invoke("exams_import", { commit, asImportant });
}

/* -------------------------------------------------------------- 盯邮箱（J2-6） */
/*
 * 对应旧版 `mail_watch.py`：只处理「指定发件人」的新邮件，把里面的待办抽出来，
 * 等用户确认再加日程。后台每 N 分钟看一眼（Rust 里的线程），发现候选会发 `mail:watch`。
 */
export type MailWatchState = {
  ok: boolean;
  enabled: boolean;
  senders: string[];
  minutes: number;
  seen: number;
  summary: string;
};

export type MailWatchItem = MailEventSuggestion & {
  /** 来源邮件的 uid（加入日程后把这封记成已处理） */
  uid?: string;
  subject?: string;
  from?: string;
};

export async function mailWatchState(): Promise<MailWatchState> {
  if (!isTauri)
    return { ok: true, enabled: false, senders: [], minutes: 10, seen: 0, summary: "" };
  return invoke<MailWatchState>("mail_watch_state");
}

export async function mailWatchCheck(): Promise<{
  ok: boolean;
  mails: { uid: string; subject: string; from: string; date: string }[];
  items: MailWatchItem[];
  errors: string[];
  error?: string;
}> {
  if (!isTauri) return { ok: false, mails: [], items: [], errors: [] };
  return invoke("mail_watch_check");
}

export async function mailWatchConsume(uids: string[]): Promise<{ ok: boolean; seen: number }> {
  if (!isTauri) return { ok: false, seen: 0 };
  return invoke("mail_watch_consume", { uids });
}

export async function mailWatchResetSeen(): Promise<{ ok: boolean; summary: string }> {
  if (!isTauri) return { ok: false, summary: "" };
  return invoke("mail_watch_reset_seen");
}

/** 后台盯到候选待办时发的事件 */
export async function onMailWatch(
  handler: (payload: { items: MailWatchItem[] }) => void,
): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ items: MailWatchItem[] }>("mail:watch", (event) =>
    handler(event.payload as { items: MailWatchItem[] }),
  );
}

/* -------------------------------------------------------------- VPN / 代理（J2-3） */
/*
 * 后台请求（ureq）每次都现读设置，所以改完立刻生效；
 * 内嵌网页（WebView2）靠进程启动参数，改完要重启一次。
 */
export async function proxySummary(): Promise<{
  ok: boolean;
  summary: string;
  webview_args: string;
}> {
  if (!isTauri) return { ok: true, summary: "", webview_args: "" };
  return invoke("proxy_summary");
}

export async function proxyProbe(): Promise<{
  ok: boolean;
  mode: string;
  proxy: string;
  summary: string;
  results: { name: string; ok: boolean; status: number; ms: number; error?: string }[];
}> {
  if (!isTauri)
    return { ok: false, mode: "直连", proxy: "", summary: "", results: [] };
  return invoke("proxy_probe");
}

/** 后台提醒到点时 Rust 会发这个事件（前端弹出卡片） */
export async function onElearnDue(
  handler: (payload: { items: DueAssignment[] }) => void,
): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<{ items: DueAssignment[] }>("elearn:due", (event) =>
    handler(event.payload as { items: DueAssignment[] }),
  );
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

export type ClassroomCell = {
  label: string;
  occupied: boolean;
  slot?: number;
  raw?: string;
};

export type ClassroomRoom = {
  name: string;
  cells: ClassroomCell[];
};

export type ClassroomView = {
  ok: boolean;
  maps: Record<string, string[]>;
  rooms: Record<string, string[]>;
  /** 抓取好的「教室 × 节次」（新版缓存；旧版缓存没有这一项） */
  grid?: ClassroomRoom[];
  periods?: string[];
  building: string | null;
  room: string | null;
  value: string | null;
  url: string | null;
  ts: number | null;
  headers: string[];
  rows: string[][];
};

export type ClassroomScrape = {
  ok: boolean;
  rooms: ClassroomRoom[];
  periods?: string[];
  value: string;
  error: string;
  url: string;
  building: string;
  room: string;
  ts: number;
};

/** 抓当前（后台隐藏的）查询页里的教室表，对应旧版「抓取当前页面」 */
export async function classroomScrape(
  building: string,
  room: string,
): Promise<ClassroomScrape> {
  return invoke<ClassroomScrape>("classroom_scrape", { building, room });
}

/**
 * 查询：让后台那个隐藏的查询页选好校区/教学楼/教室、点「查询」，再把表格读回来。
 * 对应旧版 `page_classroom.query_requested` → `ehall_bridge.classroom_query`。
 */
export async function classroomQuery(
  campus: string,
  building: string,
  room: string,
  url: string,
): Promise<ClassroomScrape> {
  return invoke<ClassroomScrape>("classroom_query", { campus, building, room, url });
}

export async function classroomView(): Promise<ClassroomView> {
  if (!isTauri)
    return {
      ok: false,
      maps: {},
      rooms: {},
      grid: [],
      periods: [],
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
  images: string[] = [],
  scene = "",
): Promise<AiChatResult> {
  if (!isTauri) {
    const last = messages.filter((one) => one.role === "user").pop();
    return {
      text: `（浏览器预览）你说：${last?.content ?? ""}`,
      rounds: 0,
      log: [],
      tools_on: 0,
    };
  }
  return invoke("ai_chat_tools", {
    messages,
    temperature,
    images: images.length ? images : null,
    scene: scene || null,
  });
}

/** 流式对话的一小段增量（后端 `ai:delta` 事件） */
export type AiDelta = { id: string; kind: "text" | "reasoning" | "tool"; text: string };

/** 一次带工具的对话结果（一次性 / 流式两条路返回同一个形状） */
export type AiChatResult = {
  text: string;
  rounds: number;
  log: ToolLogEntry[];
  tools_on: number;
  memory_injected?: number;
  remembered?: number;
  reasoning?: string;
  usage?: { prompt: number; completion: number; total: number };
  /** 用户中途点了「停止生成」 */
  stopped?: boolean;
};

/** 订阅流式增量，返回取消订阅的函数 */
export async function onAiDelta(handler: (payload: AiDelta) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<AiDelta>("ai:delta", (event) => handler(event.payload));
}

/**
 * 流式对话：正文一小段一小段推回来（事件 `ai:delta`），`reqId` 用来认领自己的增量。
 *
 * 想中途停下就调 `aiCancelStream(reqId)`；返回里的 `stopped` 会是 true，已经收到的字不丢。
 */
export async function aiChatStream(
  reqId: string,
  messages: ChatMessage[],
  temperature = 0.7,
  images: string[] = [],
  scene = "",
): Promise<AiChatResult> {
  if (!isTauri) {
    const last = messages.filter((one) => one.role === "user").pop();
    return {
      text: `（浏览器预览）你说：${last?.content ?? ""}`,
      rounds: 0,
      log: [],
      tools_on: 0,
    };
  }
  return invoke("ai_chat_stream", {
    reqId,
    messages,
    temperature,
    images: images.length ? images : null,
    scene: scene || null,
  });
}

/** 请后端停下某个流式请求（「停止生成」按钮） */
export async function aiCancelStream(reqId: string): Promise<boolean> {
  if (!isTauri) return true;
  return invoke<boolean>("ai_cancel_stream", { reqId });
}

/** 危险工具要用户点头时后端发来的请求 */
export type AiConfirm = {
  id: string;
  tool: string;
  label: string;
  target: string;
  reason: string;
  risk: "high" | "low";
  args: Record<string, unknown>;
};

/** 订阅「要你点头」的请求，返回取消订阅的函数 */
export async function onAiConfirm(handler: (payload: AiConfirm) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<AiConfirm>("ai:confirm", (event) => handler(event.payload));
}

/** 回答：allow（就这一次）/ session（本次会话都允许）/ deny */
export async function aiConfirm(id: string, choice: "allow" | "session" | "deny"): Promise<boolean> {
  if (!isTauri) return false;
  return invoke<boolean>("ai_confirm", { id, choice });
}

/** 清掉「本次会话都允许」的记录 */
export async function aiConfirmReset(): Promise<boolean> {
  if (!isTauri) return false;
  return invoke<boolean>("ai_confirm_reset");
}

/** 场景提示词清单（旧版 `prompts.load()["scenes"]`，给 AI 面板「场景」下拉用） */
export type AiScene = { key: string; title: string };

export async function aiScenes(): Promise<{ ok: boolean; scenes: AiScene[] }> {
  if (!isTauri) return { ok: true, scenes: [] };
  return invoke("ai_scenes");
}

/** 拖进来的路径展开成可当附件的文件（文件夹递归、按后缀过滤、去重） */
export type AiFileCandidate = {
  path: string;
  name: string;
  size: number;
  kind: "text" | "office" | "pdf" | "image" | "";
};

export async function aiExpandPaths(
  paths: string[],
): Promise<{ ok: boolean; files: AiFileCandidate[]; notes: string[] }> {
  if (!isTauri || !paths.length) return { ok: true, files: [], notes: [] };
  return invoke("ai_expand_paths", { paths });
}

/** Agent 沙箱：AI 只能在自己的目录里读写；跑命令默认关闭 */
export type SandboxEntry = { name: string; path: string; dir: boolean; size: number };
export type SandboxState = {
  ok: boolean;
  root: string;
  run: boolean;
  entries: SandboxEntry[];
};

export async function agentSandboxState(): Promise<SandboxState> {
  if (!isTauri) return { ok: true, root: "", run: false, entries: [] };
  return invoke<SandboxState>("agent_sandbox_state");
}

export async function agentSandboxOpen(): Promise<void> {
  if (!isTauri) return;
  await invoke("agent_sandbox_open");
}

export async function agentSandboxSetRun(on: boolean): Promise<{ ok: boolean; run: boolean }> {
  if (!isTauri) return { ok: true, run: on };
  return invoke("agent_sandbox_set_run", { on });
}

export async function agentSandboxRun(
  command: string,
): Promise<{ ok: boolean; timed_out?: boolean; code?: number; output?: string; error?: string }> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("agent_sandbox_run", { command });
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

/** 改历史会话的标题 */
export async function aiSessionRename(id: number, title: string): Promise<void> {
  if (!isTauri) return;
  await invoke("ai_session_rename", { id, title });
}

/** 删掉一条历史会话 */
export async function aiSessionDelete(id: number): Promise<void> {
  if (!isTauri) return;
  await invoke("ai_session_delete", { id });
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
  remind_minutes?: number | null;
  repeat?: string;
};

export async function dueReminders(): Promise<Reminder[]> {
  if (!isTauri) return [];
  return invoke<Reminder[]>("due_reminders");
}

/** 记「这一次提醒过了」；重复日程传 occurrenceDate，只按天记 */
export async function markNotified(id: number, occurrenceDate?: string): Promise<void> {
  if (!isTauri) return;
  await invoke("mark_notified", { id, occurrenceDate: occurrenceDate ?? null });
}

/** 「稍后 10 分钟」；重复日程只推迟那一天 */
export async function snoozeEvent(
  id: number,
  minutes = 10,
  occurrenceDate?: string,
): Promise<string> {
  if (!isTauri) return "";
  return invoke<string>("snooze_event", {
    id,
    minutes,
    occurrenceDate: occurrenceDate ?? null,
  });
}

/** 整条重复系列一起标完成 / 取消 */
export async function eventSeriesDone(id: number, done: boolean): Promise<boolean> {
  if (!isTauri) return true;
  return invoke<boolean>("event_series_done", { id, done });
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

/** 把选中文字里的公式转成 LaTeX（AI）；`latex` 是替换选区后的完整文本 */
export async function latexFormulaFromText(
  text: string,
  display = false,
): Promise<{ ok: boolean; latex: string; advice: string; error: string }> {
  if (!isTauri)
    return { ok: false, latex: "", advice: "", error: "浏览器预览里不能调用 AI" };
  return invoke("latex_formula_from_text", { text, display });
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
  has_castgc?: boolean;
  has_service_session?: boolean;
  verdict?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("uis_capture");
}

export async function jwglProbe(semester?: string): Promise<{
  ok: boolean;
  url?: string;
  semester?: string;
  semester_sniffed?: boolean;
  shape?: string;
  bytes?: number;
  top_keys?: string[];
  /** 诊断时顺手解析出来的课表格子数（不写库） */
  slots?: number;
  sample_titles?: string[];
  /** 教务页面上写的「学年起始日期」 */
  term_start?: string;
  /** 换算出来的第一周周一 */
  first_monday?: string;
  head?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("jwgl_probe", { semester });
}

/**
 * 导入教务课表：先抓 `semester/<数字 id>/print-data`，再解析成日程。
 * `termStart` 是学期第一周周一（YYYY-MM-DD），`semester` 是教务 URL 里的数字学期号。
 */
export async function jwglImport(
  termStart: string,
  semester?: string,
): Promise<{
  ok: boolean;
  added?: number;
  skipped?: number;
  slots?: number;
  bytes?: number;
  semester?: string;
  semester_sniffed?: boolean;
  /** 这次实际用的第一周周一（留空时会从教务页面的「学年起始日期」换算） */
  term_start?: string;
  term_start_sniffed?: boolean;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("jwgl_import", { termStart, semester });
}

export async function canvasProbe(): Promise<{
  ok: boolean;
  courses?: number;
  names?: string[];
  /** 课程清单（id + 课程名），诊断时顺手记进 canvas_course_list */
  list?: { id: string; name: string }[];
  /** 诊断挑出来的默认课程（第一门真的能列文件的课） */
  current?: { id: string; name: string; files: number } | null;
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
  /** 服务器账号的角色（user / admin）；由 /me 缓存下来，客户端据此决定显示不显示管理员入口 */
  role?: string;
  is_admin?: boolean;
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

/** 忘记密码第一步：请求重置验证码（服务器只在邮箱注册过时才真发，但响应是一样的） */
export async function requestResetCode(email: string): Promise<EmailCodeResult> {
  if (!isTauri) return { ok: true, email: email.trim(), expires_in: 600, cooldown: 60 };
  return invoke<EmailCodeResult>("account_request_reset_code", { email });
}

/** 忘记密码第二步：验证码 + 新口令 */
export async function resetPassword(
  email: string,
  code: string,
  password: string,
): Promise<{ ok: boolean; message: string }> {
  if (!isTauri) return { ok: true, message: "（浏览器预览）密码已重置" };
  return invoke("account_reset_password", { email, code, password });
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

/* ------------------------------------------------------- 文献方法流程图
 *
 * 对齐旧版 lit_flow.py：后端问模型要「结构化步骤」，前端用 CSS + SVG 自己画
 * （旧版是 QPainter 画 PNG，v2 不落图片，直接画节点卡片）。
 * 结果按文献 id 存 data/lit_flows/flow-<id>.json，重启后 litFlowLoad 还能读回来。
 */

export type FlowNode = {
  id: string;
  label: string;
  detail: string;
};

export type FlowEdge = {
  from: string;
  to: string;
  label?: string;
};

export type LitFlow = {
  ok: boolean;
  paper_id?: number;
  title: string;
  nodes: FlowNode[];
  edges: FlowEdge[];
  /** true = 模型没按 JSON 回，已经降级成「纯文本步骤列表」 */
  fallback?: boolean;
  /** true = 直接用上次存下来的结果（没再花钱问一次模型） */
  cached?: boolean;
  saved?: boolean;
  path?: string;
  generated_at?: string;
  /** true = 没有可用的 API Key / 配置，界面要引导去 设置 → AI */
  need_key?: boolean;
  /** 降级时模型的原话（前端「步骤列表」下面可以显示） */
  raw?: string;
  error: string;
};

function mockFlow(): LitFlow {
  return {
    ok: true,
    title: "示例：方法流程",
    nodes: [
      { id: "n1", label: "读入数据", detail: "浏览器预览里的假数据" },
      { id: "n2", label: "预处理", detail: "真实环境会由模型拆出来" },
      { id: "n3", label: "训练模型", detail: "" },
      { id: "n4", label: "汇报结果", detail: "" },
    ],
    edges: [
      { from: "n1", to: "n2", label: "" },
      { from: "n2", to: "n3", label: "" },
      { from: "n3", to: "n4", label: "" },
    ],
    fallback: false,
    cached: false,
    saved: false,
    generated_at: "",
    error: "",
  };
}

export async function litFlowGenerate(
  paperId: number,
  opts: { text?: string; title?: string; force?: boolean } = {},
): Promise<LitFlow> {
  if (!isTauri) return mockFlow();
  return invoke<LitFlow>("lit_flow_generate", {
    paperId,
    text: opts.text ?? "",
    title: opts.title ?? "",
    force: opts.force ?? false,
  });
}

/** 读回这篇文献上次生成的流程图（重启后还在） */
export async function litFlowLoad(paperId: number): Promise<LitFlow> {
  if (!isTauri) return mockFlow();
  return invoke<LitFlow>("lit_flow_load", { paperId });
}

/** 删掉存下来的流程图 */
export async function litFlowDelete(
  paperId: number,
): Promise<{ ok: boolean; deleted?: boolean; error: string }> {
  if (!isTauri) return { ok: true, deleted: true, error: "" };
  return invoke("lit_flow_delete", { paperId });
}

/**
 * 用系统默认浏览器打开 http(s) 地址。
 *
 * 内嵌网页版（右栏 AI →「网页版」）被站点用 X-Frame-Options / CSP 挡住时，
 * 这是唯一的出口：iframe 无法绕过，跨域也读不到内容，只能请用户去浏览器。
 */
export async function openUrl(url: string): Promise<void> {
  if (!isTauri) {
    // 浏览器预览界面时没有后端，直接开一个标签页
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  await invoke("web_open_url", { url });
}

/* --------------------------------------------------------- 记忆库 + 提示词
 *
 * 对应 Rust 侧 `memory.rs` / `prompts.rs`；旧版是 `lumina/memory.py`（memory.md 纯文本）
 * 与 `lumina/prompts.py` + `lumina/prompt_gen.py`。
 * 这一块的命令出错时不抛异常，统一返回 `{ok:false, error}`，界面原样显示就行。
 */

export type MemoryRow = {
  id: number;
  content: string;
  /** 从哪儿来的：手动 / AI 自动写入 */
  source: string;
  /** 逗号分隔的标签 */
  tags: string;
  created_at: string;
  updated_at: string;
  /** 被带进对话几次 */
  hits: number;
  last_used_at: string | null;
};

/** 浏览器预览（npm run dev）时的假数据，让设置卡还能点 */
let MOCK_MEMORY: MemoryRow[] = [];
let MOCK_MEMORY_SEQ = 0;

export async function memoryList(
  limit = 200,
): Promise<{ ok: boolean; items: MemoryRow[]; total: number }> {
  if (!isTauri) return { ok: true, items: [...MOCK_MEMORY], total: MOCK_MEMORY.length };
  return invoke("memory_list", { limit });
}

export async function memoryAdd(
  content: string,
  source = "手动",
  tags = "",
): Promise<{ ok: boolean; id?: number; duplicated?: boolean; total?: number; message?: string; error?: string }> {
  if (!isTauri) {
    const stamp = new Date().toISOString().slice(0, 19).replace("T", " ");
    const row: MemoryRow = {
      id: ++MOCK_MEMORY_SEQ,
      content: content.trim(),
      source,
      tags,
      created_at: stamp,
      updated_at: stamp,
      hits: 0,
      last_used_at: null,
    };
    MOCK_MEMORY = [row, ...MOCK_MEMORY];
    return { ok: true, id: row.id, duplicated: false, total: MOCK_MEMORY.length, message: "已记下" };
  }
  return invoke("memory_add", { content, source, tags });
}

export async function memorySearch(
  keyword: string,
): Promise<{ ok: boolean; items: MemoryRow[]; total: number }> {
  if (!isTauri) {
    const text = keyword.trim();
    const items = MOCK_MEMORY.filter((one) => !text || one.content.includes(text));
    return { ok: true, items, total: items.length };
  }
  return invoke("memory_search", { keyword });
}

export async function memoryUpdate(
  id: number,
  content?: string,
  tags?: string,
): Promise<{ ok: boolean; error?: string }> {
  if (!isTauri) {
    MOCK_MEMORY = MOCK_MEMORY.map((one) =>
      one.id === id ? { ...one, content: content ?? one.content, tags: tags ?? one.tags } : one,
    );
    return { ok: true };
  }
  return invoke("memory_update", {
    id,
    content: content ?? null,
    tags: tags ?? null,
  });
}

export async function memoryDelete(
  id: number,
): Promise<{ ok: boolean; total?: number; error?: string }> {
  if (!isTauri) {
    MOCK_MEMORY = MOCK_MEMORY.filter((one) => one.id !== id);
    return { ok: true, total: MOCK_MEMORY.length };
  }
  return invoke("memory_delete", { id });
}

export async function memoryClear(): Promise<{ ok: boolean; removed?: number; error?: string }> {
  if (!isTauri) {
    const removed = MOCK_MEMORY.length;
    MOCK_MEMORY = [];
    return { ok: true, removed };
  }
  return invoke("memory_clear");
}

export type PromptTemplate = {
  key: string;
  /** system = 系统提示词（内置几条，只能改）；scene = 场景模板（可增删） */
  kind: "system" | "scene";
  title: string;
  text: string;
  enabled: boolean;
  builtin: boolean;
};

function mockTemplates(): PromptTemplate[] {
  return [
    {
      key: "assistant",
      kind: "system",
      title: "通用助手",
      text: "你是 Lume（卢梅），DanLume 里的写作与学习助手。（浏览器预览里的假数据）",
      enabled: true,
      builtin: true,
    },
    {
      key: "weekly_report",
      kind: "scene",
      title: "生成周报",
      text: "根据我的日程数据写一份周报。",
      enabled: true,
      builtin: true,
    },
  ];
}

export async function promptTemplates(): Promise<{
  ok: boolean;
  items: PromptTemplate[];
  version?: number;
  updated_at?: string;
  note?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: true, items: mockTemplates(), version: 1, note: "浏览器预览" };
  return invoke("prompt_templates");
}

export async function promptTemplateSave(
  kind: string,
  key: string,
  title: string,
  text: string,
): Promise<{ ok: boolean; kind?: string; key?: string; total?: number; error?: string }> {
  if (!isTauri) {
    const items = mockTemplates();
    const found = items.find((one) => one.kind === kind && one.key === key);
    if (found) {
      found.text = text;
      found.title = title || found.title;
      return { ok: true, kind, key };
    }
    return { ok: false, error: "浏览器预览不改假数据" };
  }
  return invoke("prompt_template_save", { kind, key, title, text });
}

export async function promptTemplateDelete(
  kind: string,
  key: string,
): Promise<{ ok: boolean; key?: string; error?: string }> {
  if (!isTauri) return { ok: false, error: "浏览器预览不改假数据" };
  return invoke("prompt_template_delete", { kind, key });
}

/** 生成器：一段需求描述 → 一份结构化提示词（没配 AI 时 ok=false + 中文提示） */
export async function promptGenerate(requirement: string): Promise<{
  ok: boolean;
  text?: string;
  hint?: string;
  model?: string;
  chars?: number;
  error?: string;
}> {
  if (!isTauri) {
    return {
      ok: true,
      text: `角色：你是写作助手。\n目标：${requirement}\n输入：{用户提供的材料}\n输出格式：三句话摘要\n约束：不要编造。`,
      model: "浏览器预览",
      chars: 60,
    };
  }
  return invoke("prompt_generate", { requirement });
}



/** 内嵌网页（Tauri 子 Webview）：eHall / eLearning 贴在页面里，不弹窗 */
export async function embedOpen(
  kind: string,
  url: string,
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<{ ok: boolean; label?: string; error?: string }> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("embed_open", { kind, url, x, y, width, height });
}

export async function embedBounds(
  kind: string,
  x: number,
  y: number,
  width: number,
  height: number,
): Promise<{ ok: boolean; error?: string }> {
  if (!isTauri) return { ok: false };
  return invoke("embed_bounds", { kind, x, y, width, height });
}

export async function embedClose(kind: string): Promise<{ ok: boolean }> {
  if (!isTauri) return { ok: false };
  return invoke("embed_close", { kind });
}

export async function embedReload(kind: string): Promise<{ ok: boolean }> {
  if (!isTauri) return { ok: false };
  return invoke("embed_reload", { kind });
}

export async function embedNavigate(kind: string, url: string): Promise<{ ok: boolean; error?: string }> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("embed_navigate", { kind, url });
}

export async function embedStatus(kind: string): Promise<{ open: boolean; url: string }> {
  if (!isTauri) return { open: false, url: "" };
  return invoke("embed_status", { kind });
}

/** 从主窗口 cookie 抓一次 UIS 会话（内嵌网页登录后调用，和登录窗口共用同一份 profile） */
export async function uisGrabProfile(): Promise<{
  ok: boolean;
  cookies?: number;
  has_service_session?: boolean;
  verdict?: string;
  error?: string;
}> {
  if (!isTauri) return { ok: false, error: "browser preview" };
  return invoke("uis_grab_profile");
}
