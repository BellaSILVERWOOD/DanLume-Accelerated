<script setup lang="ts">
// AI 面板（对应旧版 `_lumina/lumina/ai_panel.py` + `ai_api_panel.py`）。
//
// 三个标签：**API 服务**（默认）→ WebUI 对话 → 系统提示词。
//
// 对话区按「聊天产品」的样子排（用户给参考图要求的效果）：
// * 助手回答**没有气泡**，直接铺在背景上；上面一行小字「用时 12 秒」，
//   下面一排小图标（复制 / 朗读 / 更多）；
// * 用户消息是一张卡片，附件在里面显示成「文件卡」（图标 + 名字 + 类型；图片给缩略图）；
// * 底部是一个大圆角撰写框：左下角「＋」加附件，右下角模型选择 + 圆形发送键。
//
// 其它照 Python 版：场景提示词、⋯ 菜单、token 用量、思考过程折叠、历史会话、
// 附件（＋ / 拖拽 / Ctrl+V，路径记在 `ai_attach_paths`）、Agent 沙箱。
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import SoftButton from "./SoftButton.vue";
import EmbedWebview from "./EmbedWebview.vue";
import PromptDrawer from "./PromptDrawer.vue";
import { renderMarkdown } from "../markdown";
import {
  agentSandboxOpen,
  agentSandboxSetRun,
  agentSandboxState,
  aiCancelStream,
  aiChatStream,
  aiChatTools,
  aiConfirm,
  aiProfiles,
  aiSaveSession,
  aiScenes,
  aiSessionDelete,
  aiSessionRename,
  aiSessions,
  contactsAiAllowed,
  contactsSetAiAllowed,
  onAiConfirm,
  onAiDelta,
  wechatRead,
  aiState,
  openUrl,
  pickFiles,
  settingsAll,
  settingsSave,
  type AiChatResult,
  type AiConfirm,
  type AiScene,
  type AiState,
  type ChatMessage,
  type ChatSession,
  type SandboxState,
  type ToolLogEntry,
} from "../api";
import {
  asContext,
  describeAttachments,
  imageParts,
  loadAttachments,
  type Attachment,
} from "../lib/aiAttach";
import { pendingMascot, takeMascot, type MascotPayload } from "../lib/mascotBus";

/**
 * 梅子现在是什么状态（用户 2026-11-14）：
 * * 模型还没出字 / 在调工具 → 「思考中」，放 kanban_thinking
 * * 已经开始出字 → 「回答中」，放恍然大悟那张
 * 都没有就返回空串（聊天区不挂图）。
 */
const meiziState = computed<"" | "think" | "answer">(() => {
  const last = messages.value[messages.value.length - 1];
  if (!last || !last.streaming) return "";
  return last.text && last.text.trim() ? "answer" : "think";
});

const ATTACH_KEY = "ai_attach_paths";
const SCENE_KEY = "ai_scene";

const SERVICES: { id: string; name: string; url: string }[] = [
  { id: "deepseek", name: "DeepSeek", url: "https://chat.deepseek.com/" },
  { id: "doubao", name: "豆包", url: "https://www.doubao.com/chat/" },
  { id: "kimi", name: "Kimi", url: "https://kimi.moonshot.cn/" },
];

type Bubble = {
  role: "user" | "assistant";
  text: string;
  reasoning?: string;
  tools?: ToolLogEntry[];
  memory?: number;
  remembered?: number;
  attach?: Attachment[];
  /** 助手这条用时（秒） */
  seconds?: number;
  /** 出错时把错误也放在这条上 */
  error?: string;
  /** 正在流式接收（正文末尾显示闪烁光标） */
  streaming?: boolean;
  /** 流式时正在调用哪个工具（实时显示「正在用 xxx…」） */
  toolRunning?: string;
  /** 用户中途点了「停止生成」 */
  stopped?: boolean;
};

const tab = ref<"api" | "webui" | "prompt">("api");
const state = ref<AiState | null>(null);
const scenes = ref<AiScene[]>([]);
const scene = ref("");
const profiles = ref<{ id: number; label: string; model: string }[]>([]);
const profileId = ref(0);
const note = ref("");
const error = ref("");

const messages = ref<Bubble[]>([]);
const draft = ref("");
const busy = ref(false);
const scroller = ref<HTMLElement | null>(null);
const follow = ref(true);
const showReasoning = ref(true);
const showHistory = ref(false);
const showPrompts = ref(false);
const showMore = ref(false);
const history = ref<ChatSession[]>([]);
const sessionId = ref(0);
const sessionTitle = ref("");

const attachments = ref<Attachment[]>([]);
const attachNote = ref("");
const loadingAttach = ref(false);
const dragging = ref(false);

const usage = ref({ prompt: 0, completion: 0, total: 0 });
const sandbox = ref<SandboxState>({ ok: true, root: "", run: false, entries: [] });
const showSandboxHelp = ref(false);
/** 「允许读取通讯录」（默认关，状态从设置里读，不用每次重设） */
const contactsAllowed = ref(false);

const webService = ref("deepseek");
const webUrl = ref(SERVICES[0].url);
const customSites = ref<{ id: string; name: string; url: string }[]>([]);
const localSites = ref<{ id: string; name: string; url: string }[]>([]);

const modelLabel = computed(() => {
  const one = state.value;
  if (!one) return "AI";
  if (!one.ok) return "未配置";
  return `${one.model || "（未填模型名）"} · ${one.has_key ? "有 Key" : "没填 Key"}`;
});
const attachSummary = computed(() => describeAttachments(attachments.value));
const canSend = computed(
  () => !busy.value && (draft.value.trim() !== "" || attachments.value.length > 0),
);
const usageText = computed(() =>
  usage.value.total
    ? `${usage.value.prompt} + ${usage.value.completion} = ${usage.value.total} tokens`
    : "",
);
const allSites = computed(() => [...SERVICES, ...customSites.value, ...localSites.value]);
/** 悬停提示：一句话说清这个文件夹是干什么的（「沙箱」这个词太技术） */
const sandboxTip = computed(
  () =>
    `AI 只能读写这个文件夹里的文件，碰不到别的地方。${
      sandbox.value.run ? "现在允许它在这里跑命令。" : "现在只允许读写文件，不允许跑命令。"
    }\n${sandbox.value.root || ""}`,
);
/** 配置下拉太窄时字会被省略，悬停给出「名字 · 模型」全文 */
const profileTitle = computed(() => {
  const hit = profiles.value.find((one) => one.id === profileId.value);
  if (!hit) return "切换 API 配置";
  return `${hit.label}${hit.model ? " · " + hit.model : ""}（点一下换一个配置）`;
});

function parseSites(raw: string) {
  try {
    const data = JSON.parse(raw || "[]");
    if (!Array.isArray(data)) return [];
    return data
      .filter((one) => one && typeof one.url === "string" && one.url.startsWith("http"))
      .map((one) => ({ id: String(one.id || one.url), name: String(one.name || one.url), url: String(one.url) }));
  } catch {
    return [];
  }
}

async function loadSettings() {
  try {
    const all = await settingsAll();
    const values = all.values || {};
    customSites.value = parseSites(values["ai_custom_sites"] || "");
    localSites.value = parseSites(values["ai_local_sites"] || "");
    webService.value = values["ai_service"] || "deepseek";
    scene.value = values[SCENE_KEY] || "";
    const rawPaths = (values[ATTACH_KEY] || "").trim();
    if (rawPaths) {
      let paths: string[] = [];
      if (rawPaths.startsWith("[")) {
        try {
          const data = JSON.parse(rawPaths);
          if (Array.isArray(data)) {
            paths = data
              .map((one) => (typeof one === "string" ? one : String(one?.path || "")))
              .filter(Boolean);
          }
        } catch {
          paths = [];
        }
      } else {
        paths = rawPaths.split("\n").map((one) => one.trim()).filter(Boolean);
      }
      if (paths.length) void addAttachments(paths, true);
    }
    const service = allSites.value.find((one) => one.id === webService.value);
    webUrl.value = service ? service.url : SERVICES[0].url;
  } catch {
    /* 读不到就用默认 */
  }
}

function saveAttachPaths() {
  void settingsSave({ [ATTACH_KEY]: attachments.value.map((one) => one.path).join("\n") });
}

async function refreshState() {
  try {
    state.value = await aiState();
    error.value = state.value.ok ? "" : state.value.error;
    const list = await aiProfiles();
    profiles.value = (list || []).map((one) => ({
      id: Number(one.id),
      label: String(one.name || one.model || "配置"),
      model: String(one.model || ""),
    }));
    const hit = profiles.value.find((one) => one.model && one.model === state.value?.model);
    if (hit) profileId.value = hit.id;
    sandbox.value = await agentSandboxState();
    contactsAllowed.value = await contactsAiAllowed();
  } catch (err) {
    error.value = String(err);
  }
}

async function loadScenes() {
  try {
    scenes.value = (await aiScenes()).scenes || [];
  } catch {
    scenes.value = [];
  }
}

async function loadHistory() {
  try {
    history.value = await aiSessions(30);
  } catch {
    history.value = [];
  }
}

/** 历史会话：改名用行内输入框，删除要点两下（第一次变「确定删？」） */
const renamingId = ref(0);
const renameDraft = ref("");
const confirmingId = ref(0);

function startRename(item: ChatSession) {
  renamingId.value = Number(item.id || 0);
  renameDraft.value = item.title || "";
  confirmingId.value = 0;
}

async function commitRename(item: ChatSession) {
  const id = Number(item.id || 0);
  if (!id) return;
  try {
    await aiSessionRename(id, renameDraft.value);
    renamingId.value = 0;
    await loadHistory();
    note.value = "标题改好了";
  } catch (err) {
    note.value = `改名失败：${String(err)}`;
  }
}

async function removeSession(item: ChatSession) {
  const id = Number(item.id || 0);
  if (!id) return;
  if (confirmingId.value !== id) {
    confirmingId.value = id;
    return;
  }
  confirmingId.value = 0;
  try {
    await aiSessionDelete(id);
    // 删的正好是当前这条，就把当前会话的 id 清掉（内容还留在界面上）
    if (sessionId.value === id) sessionId.value = 0;
    await loadHistory();
    note.value = "已删掉这条历史会话";
  } catch (err) {
    note.value = `删除失败：${String(err)}`;
  }
}

async function onSceneChange() {
  const title = scenes.value.find((one) => one.key === scene.value)?.title || scene.value;
  note.value = scene.value ? `用场景提示词「${title}」` : "";
  await settingsSave({ [SCENE_KEY]: scene.value });
}

async function onProfileChange() {
  await settingsSave({ ai_profile: String(profileId.value) });
  await refreshState();
  note.value = "已切换 API 配置";
}

async function toggleSandboxRun() {
  const next = !sandbox.value.run;
  await agentSandboxSetRun(next);
  sandbox.value = await agentSandboxState();
  note.value = next ? "已允许 AI 在 AI 文件夹里跑命令" : "已关闭「让 AI 跑命令」";
}

/**
 * 「允许读取通讯录」：默认关，勾选状态**存进设置**（用户 2026-11-12 的要求：
 * 不要每次打开 AI 页面都重新设置一次）。关着的时候 AI 根本拿不到通讯录工具。
 */
async function toggleContacts() {
  const next = !contactsAllowed.value;
  try {
    contactsAllowed.value = await contactsSetAiAllowed(next);
    note.value = next
      ? "已允许 AI 读取通讯录（它才能按姓名查电话 / 邮箱）"
      : "已关闭「读取通讯录」";
  } catch (err) {
    note.value = `切换失败：${String(err)}`;
  }
}

/**
 * 「读微信消息」（J8-4，照旧版 `wechat_read.py`）：截图当前微信聊天 → 系统 OCR →
 * 清洗后填进输入框，再由用户决定发不发。`useClipboard` 走「用户自己右键复制」的退路。
 */
const wechatBusy = ref(false);
async function readWechat(useClipboard: boolean) {
  if (wechatBusy.value) return;
  wechatBusy.value = true;
  showMore.value = false;
  note.value = useClipboard ? "正在读剪贴板…" : "正在读微信窗口（截图 + 识别，约 2–5 秒）…";
  try {
    const result = await wechatRead(useClipboard);
    if (!result.ok || !result.text) {
      note.value = result.error || "没读到内容";
      return;
    }
    const lines = result.text.split("\n").filter((one) => one.trim()).length;
    draft.value = result.text;
    const how = result.how || "微信";
    note.value = result.warning
      ? `${result.warning}（共 ${lines} 行，已填进输入框）`
      : `从${how}读到 ${lines} 行，已填进输入框`;
  } catch (err) {
    note.value = `读微信失败：${String(err)}`;
  } finally {
    wechatBusy.value = false;
  }
}

// ---------------------------------------------------------------- 附件
async function addAttachments(paths: string[], quiet = false) {
  if (!paths.length) return;
  loadingAttach.value = true;
  try {
    const result = await loadAttachments(paths, attachments.value);
    if (result.files.length) {
      attachments.value = [...attachments.value, ...result.files];
      saveAttachPaths();
    }
    if (!quiet) {
      attachNote.value = result.notes.length
        ? result.notes.join("；")
        : result.files.length
          ? `已加上 ${result.files.length} 个附件`
          : "";
    }
  } catch (err) {
    attachNote.value = `读附件失败：${String(err)}`;
  } finally {
    loadingAttach.value = false;
  }
}

async function pickAttach() {
  const picked = await pickFiles();
  await addAttachments(picked);
}

function clearAttachments() {
  attachments.value = [];
  attachNote.value = "";
  saveAttachPaths();
}

/** Tauri 会把拖进来的真实路径交给我们（旧版是 Qt 的 files_dropped） */
let unlistenDrop: (() => void) | null = null;
async function setupDragDrop() {
  try {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload as { type: string; paths?: string[] };
      if (payload.type === "enter") dragging.value = true;
      else if (payload.type === "leave") dragging.value = false;
      else if (payload.type === "drop") {
        dragging.value = false;
        const paths = payload.paths || [];
        if (paths.length) void addAttachments(paths);
      }
    });
  } catch {
    /* 浏览器里没有这个 API */
  }
}

// ---------------------------------------------------------------- 发送
async function scrollDown() {
  await nextTick();
  if (scroller.value) scroller.value.scrollTop = scroller.value.scrollHeight;
  follow.value = true;
}

function onScroll() {
  const el = scroller.value;
  if (!el) return;
  follow.value = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
}

/**
 * 流式增量：只认自己那个请求 id 的片段。
 *
 * 后端在 `ai:delta` 事件里推 `{id, kind, text}`：
 * `text` 是正文增量、`reasoning` 是思考增量、`tool` 是「正在调用哪个工具」。
 */
const streamId = ref("");
let unlistenDelta: (() => void) | null = null;

async function ensureDeltaListener() {
  if (unlistenDelta) return;
  unlistenDelta = await onAiDelta((payload) => {
    if (!payload || payload.id !== streamId.value) return;
    const target = messages.value[messages.value.length - 1];
    if (!target || target.role !== "assistant") return;
    if (payload.kind === "tool") {
      target.toolRunning = payload.text;
      return;
    }
    if (payload.kind === "reasoning") {
      target.reasoning = (target.reasoning || "") + payload.text;
      return;
    }
    target.toolRunning = "";
    target.text = (target.text || "") + payload.text;
    if (follow.value) void scrollDown();
  });
}

/** 用户点「停止生成」：让后端收手，已经收到的字留着 */
async function stopStream() {
  if (!busy.value || !streamId.value) return;
  note.value = "正在停止…";
  try {
    await aiCancelStream(streamId.value);
  } catch {
    /* 停不下来就让请求自己结束 */
  }
}

// ---------------------------------------------------------------- 工具审核（J8-3）
//
// 危险工具（发邮件 / 改设置 / 跑命令 / 写文件 / 下载）动手前后端会等在这儿，
// 用户点「允许一次 / 本次会话都允许 / 拒绝」之后才继续。
const pendingConfirm = ref<AiConfirm | null>(null);
let unlistenConfirm: (() => void) | null = null;

async function ensureConfirmListener() {
  if (unlistenConfirm) return;
  unlistenConfirm = await onAiConfirm((payload) => {
    if (!payload) return;
    pendingConfirm.value = payload;
  });
}

async function answerConfirm(choice: "allow" | "session" | "deny") {
  const item = pendingConfirm.value;
  if (!item) return;
  pendingConfirm.value = null;
  try {
    await aiConfirm(item.id, choice);
    note.value =
      choice === "deny"
        ? `没同意「${item.label}」这一次`
        : choice === "session"
          ? `「${item.label}」这次会话不再问`
          : `已允许「${item.label}」这一次`;
  } catch (err) {
    note.value = `回复审核失败：${String(err)}`;
  }
}

async function send() {
  if (busy.value) return;
  const text = draft.value.trim();
  if (!text && !attachments.value.length) return;
  const files = [...attachments.value];
  const context = asContext(files);
  const images = imageParts(files);
  const composed = context ? `${text}\n\n${context}` : text;

  messages.value.push({
    role: "user",
    text: text || (images.length ? "看看这些图片" : "（附件）"),
    attach: files,
  });
  draft.value = "";
  busy.value = true;
  error.value = "";
  attachNote.value = "";
  await scrollDown();
  const started = Date.now();

  const payload: ChatMessage[] = messages.value
    .filter((one) => one.role === "user" || one.role === "assistant")
    .map((one, index) => ({
      role: one.role,
      content:
        index === messages.value.length - 1 && one.role === "user" && context ? composed : one.text,
    }));

  // 先放一条空白的助手气泡，增量直接往里拼（这就是「边收边显示」）
  messages.value.push({ role: "assistant", text: "", streaming: true });
  const reqId = `req-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  streamId.value = reqId;
  await ensureDeltaListener();

  let result: AiChatResult | null = null;
  try {
    result = await aiChatStream(reqId, payload, 0.7, images, scene.value);
  } catch (err) {
    // 流式这条路不通（比如服务端不支持 SSE），退回一次性请求，别让用户白等
    const target = messages.value[messages.value.length - 1];
    if (target && target.role === "assistant") {
      target.streaming = false;
      // 已经收到一部分字就别再发一次请求（那是双倍 token），把错误挂在 note 上
      if (target.text) {
        note.value = `流式断了（${String(err)}），上面是已经收到的部分`;
        target.stopped = true;
      } else {
        messages.value.pop();
        note.value = `流式没成功（${String(err)}），改成一次性返回`;
        try {
          result = await aiChatTools(payload, 0.7, images, scene.value);
        } catch (again) {
          error.value = String(again);
        }
      }
    }
  } finally {
    streamId.value = "";
  }

  const target = messages.value[messages.value.length - 1];
  try {
    if (result) {
      if (target && target.role === "assistant") {
        target.text = result.text || target.text;
        target.reasoning = result.reasoning || target.reasoning || "";
        target.tools = result.log || [];
        target.memory = result.memory_injected ?? 0;
        target.remembered = result.remembered ?? 0;
        target.seconds = Math.max(1, Math.round((Date.now() - started) / 1000));
        target.streaming = false;
        target.toolRunning = "";
        target.stopped = !!result.stopped;
      } else {
        messages.value.push({
          role: "assistant",
          text: result.text,
          reasoning: result.reasoning || "",
          tools: result.log || [],
          memory: result.memory_injected ?? 0,
          remembered: result.remembered ?? 0,
          seconds: Math.max(1, Math.round((Date.now() - started) / 1000)),
        });
      }
      if (result.stopped) note.value = "已停止生成";
      const one = result.usage || { prompt: 0, completion: 0, total: 0 };
      if (one.total) {
        usage.value = {
          prompt: usage.value.prompt + (one.prompt || 0),
          completion: usage.value.completion + (one.completion || 0),
          total: usage.value.total + (one.total || 0),
        };
      }
      const last = messages.value.filter((one) => one.role === "user").slice(-1)[0];
      try {
        const saved = await aiSaveSession(
          sessionTitle.value || (last?.text || "新对话").slice(0, 30),
          state.value?.model || "",
          JSON.stringify(messages.value),
        );
        if (typeof saved === "number" && saved > 0) sessionId.value = saved;
      } catch {
        /* 存历史失败不影响对话 */
      }
      await loadHistory();
    } else if (target && target.role === "assistant") {
      target.streaming = false;
      if (!target.text) messages.value.pop();
    }
  } finally {
    busy.value = false;
    if (follow.value) await scrollDown();
  }
}

function newChat() {
  messages.value = [];
  draft.value = "";
  error.value = "";
  note.value = "新对话";
  showHistory.value = false;
  showMore.value = false;
  sessionId.value = 0;
  sessionTitle.value = "";
}

function clearChat() {
  messages.value = [];
  error.value = "";
  showMore.value = false;
  note.value = "已清空当前对话（历史会话里还留着）";
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    void send();
  }
}

function restore(session: ChatSession) {
  try {
    const rows = JSON.parse(session.payload || "[]");
    messages.value = (Array.isArray(rows) ? rows : [])
      .filter((one) => one && (one.role === "user" || one.role === "assistant"))
      .map((one) => ({
        role: one.role as "user" | "assistant",
        text: String(one.text ?? one.content ?? ""),
        tools: one.tools || [],
        memory: one.memory,
        remembered: one.remembered,
        attach: one.attach || [],
      }));
    sessionId.value = Number(session.id || 0);
    sessionTitle.value = session.title || "";
    showHistory.value = false;
    void scrollDown();
  } catch {
    error.value = "这条历史读不出来";
  }
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    note.value = "已复制";
  } catch {
    note.value = "复制失败（浏览器不允许）";
  }
}

function speak(text: string) {
  try {
    const utter = new SpeechSynthesisUtterance(text.slice(0, 600));
    utter.lang = "zh-CN";
    window.speechSynthesis.speak(utter);
  } catch {
    note.value = "这台机器没法朗读";
  }
}

function formatSeconds(seconds?: number): string {
  if (!seconds) return "";
  if (seconds < 60) return `${seconds} 秒`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return rest ? `${minutes} 分 ${rest} 秒` : `${minutes} 分钟`;
}

function fileTone(kind: string): string {
  if (kind === "pdf") return "rose";
  if (kind === "office") return "sky";
  if (kind === "image") return "violet";
  return "teal";
}

function fileKindLabel(kind: string): string {
  if (kind === "pdf") return "PDF";
  if (kind === "office") return "文档";
  if (kind === "image") return "图片";
  return "文本";
}

function humanSize(size: number): string {
  if (size >= 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  if (size >= 1024) return `${Math.round(size / 1024)} KB`;
  return `${size} B`;
}

// ---------------------------------------------------------------- WebUI / 提示词
async function onWebServiceChange() {
  const site = allSites.value.find((one) => one.id === webService.value);
  if (!site) return;
  webUrl.value = site.url;
  await settingsSave({ ai_service: site.id });
}

async function addCustomSite(local: boolean) {
  const url = window.prompt(
    local
      ? "本地模型服务的 Web UI 地址（llama.cpp / SGLang / Ollama 都可以）："
      : "要内嵌的 AI 网址（例如自建的 ChatGPT 页面）：",
    local ? "http://127.0.0.1:8080" : "",
  );
  const text = (url || "").trim();
  if (!text.startsWith("http")) return;
  const name = text.replace(/^https?:\/\//, "").replace(/^www\./, "").split("/")[0];
  const item = { id: `${local ? "local" : "custom"}-${Date.now()}`, name, url: text };
  if (local) localSites.value = [...localSites.value, item];
  else customSites.value = [...customSites.value, item];
  await settingsSave({
    ai_local_sites: JSON.stringify(localSites.value),
    ai_custom_sites: JSON.stringify(customSites.value),
    ai_service: item.id,
  });
  webService.value = item.id;
  webUrl.value = item.url;
}

function usePrompt(text: string) {
  showPrompts.value = false;
  draft.value = text;
}

onMounted(async () => {
  await loadSettings();
  await refreshState();
  await loadScenes();
  await loadHistory();
  await setupDragDrop();
  await ensureConfirmListener();
});

/**
 * 看板娘悬浮球丢过来的东西：切到 AI 标签、文字填进输入框、文件当附件。
 *
 * 用户 2026-11-12：把文件或文字拖到球上就一键发给 AI。
 * 事件由 App.vue 收（那边一直在），这里只从 mascotBus 取——右栏切到 AI 时调用。
 */
function applyMascot(payload: MascotPayload) {
  tab.value = "api";
  showHistory.value = false;
  if (payload.text) draft.value = payload.text;
  const paths = payload.paths || [];
  if (paths.length) void addAttachments(paths);
  note.value = paths.length
    ? `从看板娘收到 ${paths.length} 个文件，可以补充说明后发送`
    : "从看板娘收到一句话";
}

// 挂载时先看有没有留着没处理的，然后一直盯着（面板显示时收到的也能接住）
onMounted(() => {
  const waiting = takeMascot();
  if (waiting) applyMascot(waiting);
});
watch(pendingMascot, (value) => {
  if (!value) return;
  const payload = takeMascot();
  if (payload) applyMascot(payload);
});

watch(tab, async (value) => {
  if (value === "webui") note.value = "网页登录一次就会记住；切走再回来不用重新登录";
  else if (value === "api") {
    note.value = "";
    await refreshState();
  }
});

onBeforeUnmount(() => {
  unlistenDrop?.();
  unlistenDelta?.();
  unlistenDelta = null;
  unlistenConfirm?.();
  unlistenConfirm = null;
});
</script>

<template>
  <div class="ai-panel" :class="{ dragging }">
    <div v-if="dragging" class="drop-hint">松手就加进对话附件</div>

    <div class="tabs">
      <button class="tab" :class="{ on: tab === 'api' }" @click="tab = 'api'">API 服务</button>
      <button class="tab" :class="{ on: tab === 'webui' }" @click="tab = 'webui'">WebUI 对话</button>
      <button class="tab" :class="{ on: tab === 'prompt' }" @click="tab = 'prompt'">系统提示词</button>
    </div>

    <!-- ---------------------------------------------------- API 服务 -->
    <template v-if="tab === 'api'">
      <PromptDrawer v-if="showPrompts" @close="showPrompts = false" @use="usePrompt" />
      <template v-else>
        <div class="head">
          <select v-model="scene" class="scene" title="场景提示词（在「系统提示词」里编辑）" @change="onSceneChange">
            <option value="">通用助手</option>
            <option v-for="one in scenes" :key="one.key" :value="one.key">{{ one.title }}</option>
          </select>
          <button class="more" title="更多" @click="showMore = !showMore">⋯</button>
        </div>

        <div v-if="showMore" class="pop">
          <button @click="newChat">新对话</button>
          <button @click="clearChat">清空当前对话</button>
          <button @click="showHistory = !showHistory; loadHistory()">历史会话</button>
          <button @click="showPrompts = true">提示词模板</button>
          <button @click="showReasoning = !showReasoning">
            {{ showReasoning ? "隐藏思考过程" : "显示思考过程" }}
          </button>
          <button @click="toggleSandboxRun">
            {{ sandbox.run ? "关闭「让 AI 跑命令」" : "允许「让 AI 跑命令」" }}
          </button>
          <button @click="toggleContacts">
            {{ contactsAllowed ? "关闭「读取通讯录」" : "允许「读取通讯录」" }}
          </button>
          <button @click="readWechat(false)">读微信消息</button>
          <button @click="readWechat(true)">读剪贴板</button>
        </div>

        <div v-if="note" class="note">{{ note }}</div>

        <div v-if="showHistory" class="history">
          <div class="history-head">
            <span>历史会话</span>
            <span class="spacer" />
            <SoftButton :height="24" :font-size="11" @click="showHistory = false">收起</SoftButton>
          </div>
          <div v-for="item in history" :key="item.id" class="history-item">
            <button class="history-row" @click="restore(item)">
              <span class="name">{{ item.title || "（未命名）" }}</span>
              <span class="meta">{{ item.model }} · {{ item.updated_at.slice(5, 16) }}</span>
            </button>
            <div v-if="renamingId === item.id" class="rename-row">
              <input
                v-model="renameDraft"
                class="rename-input"
                placeholder="换个标题"
                @keyup.enter="commitRename(item)"
                @keyup.esc="renamingId = 0"
              />
              <button class="mini" @click="commitRename(item)">存</button>
              <button class="mini" @click="renamingId = 0">取消</button>
            </div>
            <div v-else class="row-tools">
              <button class="mini" title="改个名字" @click="startRename(item)">改名</button>
              <button
                class="mini"
                :class="{ danger: confirmingId === item.id }"
                :title="confirmingId === item.id ? '再点一下就真删了' : '删掉这条历史'"
                @click="removeSession(item)"
              >
                {{ confirmingId === item.id ? "确定删？" : "删" }}
              </button>
            </div>
          </div>
          <div v-if="!history.length" class="empty">还没有历史会话</div>
        </div>

        <!-- 梅子在思考 / 在回答：右下角一张小图（用户 2026-11-14 指定用哪两张） -->
        <div v-if="meiziState" class="meizi-live">
          <img
            :src="meiziState === 'answer' ? '/mascot/meizi-answer.webp' : '/mascot/meizi-thinking.webp'"
            :alt="meiziState === 'answer' ? '梅子在回答' : '梅子在思考'"
            draggable="false"
          />
          <span>{{ meiziState === "answer" ? "梅子在回答…" : "梅子在思考…" }}</span>
        </div>
        <div ref="scroller" class="chat" @scroll.passive="onScroll">
          <!-- 空对话时摆梅子的立绘（用户 2026-11-14：第一张图放 AI 页面） -->
          <div v-if="!messages.length" class="empty ai-empty">
            <img class="ai-portrait" src="/mascot/meizi-ai.png" alt="梅子" draggable="false" />
            <div class="ai-greet">我是梅子，有什么要做？</div>
            <div class="ai-sub">可以直接把 PDF / Word / 图片拖进来当上下文，也可以让我帮你写日程、改公式。</div>
          </div>
          <div v-for="(message, index) in messages" :key="index" class="msg" :class="message.role">
            <!-- 助手：时间行 + 正文 + 操作条 -->
            <template v-if="message.role === 'assistant'">
              <div v-if="message.seconds" class="elapsed">
                用时 {{ formatSeconds(message.seconds) }}
                <template v-if="message.stopped"> · 已停止</template>
                ›
              </div>
              <div v-else-if="message.streaming" class="elapsed">
                {{ message.toolRunning ? `正在用 ${message.toolRunning}…` : "正在生成…" }}
              </div>
              <details v-if="message.reasoning && showReasoning" class="reasoning">
                <summary>思考过程（{{ message.reasoning.length }} 字）</summary>
                <div class="reasoning-body">{{ message.reasoning }}</div>
              </details>
              <div class="md" :class="{ typing2: message.streaming }" v-html="renderMarkdown(message.text)" />
              <div v-if="message.streaming && !message.text" class="typing">正在思考…</div>
              <div v-if="message.tools && message.tools.length" class="tools-used">
                <span class="tool-head">用了 {{ message.tools.length }} 次工具</span>
                <span
                  v-for="(tool, index2) in message.tools"
                  :key="index2"
                  class="tool-chip"
                  :class="{ bad: !tool.ok }"
                  :title="tool.summary"
                >
                  {{ tool.name }}
                </span>
              </div>
              <div v-if="message.memory !== undefined" class="mem-note">
                本次注入了 {{ message.memory }} 条记忆<template v-if="message.remembered">
                  ，另记下 {{ message.remembered }} 条</template
                >
              </div>
              <div class="actions-line">
                <button title="复制" @click="copyText(message.text)">⧉</button>
                <button title="朗读" @click="speak(message.text)">🔊</button>
                <button title="更多" @click="showMore = !showMore">⋯</button>
              </div>
            </template>
            <!-- 用户：卡片 + 附件文件卡 -->
            <template v-else>
              <div v-if="message.attach && message.attach.length" class="attach-cards">
                <div v-for="file in message.attach" :key="file.path" class="file-card">
                  <img v-if="file.kind === 'image' && file.dataUrl" :src="file.dataUrl" class="thumb" alt="" />
                  <span v-else class="file-icon" :class="fileTone(file.kind)">{{ fileKindLabel(file.kind) }}</span>
                  <span class="file-meta">
                    <span class="file-name">{{ file.name }}</span>
                    <span class="file-sub">{{ fileKindLabel(file.kind) }} · {{ humanSize(file.size) }}</span>
                  </span>
                </div>
              </div>
              <div class="user-text">{{ message.text }}</div>
            </template>
          </div>
          <div v-if="error" class="error">{{ error }}</div>
        </div>

        <!-- 工具审核：AI 想动真格的，先让用户点头（旧版 `_confirm_tool` 的弹窗） -->
        <div v-if="pendingConfirm" class="confirm-card">
          <div class="confirm-head">
            <span class="confirm-title">AI 想用「{{ pendingConfirm.label }}」</span>
            <span v-if="pendingConfirm.risk === 'high'" class="confirm-risk">危险操作</span>
          </div>
          <div class="confirm-why">{{ pendingConfirm.reason }}</div>
          <div v-if="pendingConfirm.target" class="confirm-target" :title="pendingConfirm.target">
            {{ pendingConfirm.target }}
          </div>
          <div class="confirm-actions">
            <button class="mini danger" @click="answerConfirm('deny')">拒绝</button>
            <button class="mini" @click="answerConfirm('allow')">允许一次</button>
            <button class="mini primary" @click="answerConfirm('session')">这次会话都允许</button>
          </div>
        </div>

        <!-- 撰写区：大圆角，左下 ＋，右下 模型 + 圆形发送 -->
        <div class="composer">
          <div v-if="attachSummary || attachNote || !follow || usageText" class="chips">
            <span v-if="attachSummary" class="attached">{{ attachSummary }}</span>
            <span v-if="attachNote" class="attach-note">{{ attachNote }}</span>
            <span class="spacer" />
            <button v-if="!follow" class="mini" @click="scrollDown">回到底部</button>
            <span v-if="usageText" class="usage">{{ usageText }}</span>
            <button v-if="attachments.length" class="mini" @click="clearAttachments">清除附件</button>
          </div>
          <textarea
            v-model="draft"
            class="input"
            rows="2"
            placeholder="有什么要做？Enter 发送，Shift+Enter 换行"
            @keydown="onKeydown"
          />
          <div class="composer-foot">
            <button class="plus" :disabled="loadingAttach" title="加附件（也可以拖文件进来）" @click="pickAttach">
              <span>＋</span>
            </button>
            <span class="spacer" />
            <select
              v-if="profiles.length"
              v-model.number="profileId"
              class="profile"
              :title="profileTitle"
              @change="onProfileChange"
            >
              <option v-for="one in profiles" :key="one.id" :value="one.id">{{ one.label }}</option>
            </select>
            <!-- 生成中这个键就是「停止」：点一下后端收手，已经收到的字留着 -->
            <button
              class="send"
              :class="{ busy }"
              :disabled="!canSend && !busy"
              :title="busy ? '停止生成' : '发送（Enter）'"
              @click="busy ? stopStream() : send()"
            >
              {{ busy ? "■" : "↑" }}
            </button>
          </div>
        </div>

        <!-- AI 文件夹（沙箱）：一行摘要 + 打开目录 + 说明 -->
        <div class="sandbox">
          <span class="sb-label">AI 文件夹</span>
          <span class="sb-path" :title="sandboxTip">{{ sandbox.entries.length }} 个文件</span>
          <!-- 开了「跑命令」才亮的小圆点：这是个安全开关，得看得见 -->
          <span v-if="sandbox.run" class="sb-dot" :title="sandboxTip">●</span>
          <span class="spacer" />
          <button class="mini" title="在资源管理器里打开这个文件夹" @click="agentSandboxOpen()">
            打开
          </button>
          <button
            class="mini"
            :class="{ on: showSandboxHelp }"
            title="这个文件夹是干什么的"
            @click="showSandboxHelp = !showSandboxHelp"
          >
            ?
          </button>
        </div>
        <!-- 看不懂「沙箱」的人（用户 2026-11-11）：「我不能理解你的这个 sandbox」→ 直接写清楚 -->
        <div v-if="showSandboxHelp" class="sb-help">
          <p>这是给 AI 单独划出来的一个文件夹（技术上叫「沙箱」）。</p>
          <p>· AI 读写文件<strong>只能</strong>动这里，碰不到你电脑别的地方；</p>
          <p>· 想让它处理什么，把文件拖进聊天框，或者直接丢进这个文件夹；</p>
          <p>· 「让 AI 跑命令」默认关着（在 ⋯ 菜单里开），开了才能在这里执行命令，不限时长；</p>
          <p>· 位置：<span class="sb-where">{{ sandbox.root || "还没建出来" }}</span></p>
        </div>
      </template>
    </template>

    <!-- ---------------------------------------------------- WebUI 对话 -->
    <template v-else-if="tab === 'webui'">
      <div class="head">
        <select v-model="webService" class="scene" @change="onWebServiceChange">
          <option v-for="one in SERVICES" :key="one.id" :value="one.id">{{ one.name }}</option>
          <option v-for="one in customSites" :key="one.id" :value="one.id">{{ one.name }}</option>
          <option v-for="one in localSites" :key="one.id" :value="one.id">🖥 {{ one.name }}</option>
        </select>
        <button class="more" @click="addCustomSite(false)">＋网址</button>
        <button class="more" @click="addCustomSite(true)">＋本地</button>
        <button class="more" @click="openUrl(webUrl)">浏览器</button>
      </div>
      <div v-if="note" class="note">{{ note }}</div>
      <EmbedWebview kind="ai-web" :url="webUrl" home-label="回首页" />
    </template>

    <!-- ---------------------------------------------------- 系统提示词 -->
    <template v-else>
      <PromptDrawer @close="tab = 'api'" @use="usePrompt" />
    </template>
  </div>
</template>

<style scoped>
/* 空对话时的梅子立绘 */
.ai-empty {
  align-items: center;
  text-align: center;
  gap: 6px;
}
.ai-portrait {
  width: 150px;
  max-height: 200px;
  object-fit: contain;
  border-radius: 14px;
  margin-bottom: 2px;
  filter: drop-shadow(0 6px 14px rgba(0, 0, 0, 0.12));
}
.ai-greet {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-soft);
}
.ai-sub {
  font-size: 11.5px;
  color: var(--muted);
  line-height: 1.5;
  max-width: 260px;
}

/* 梅子状态图：贴在聊天区右下角，不挡文字 */
.meizi-live {
  position: absolute;
  right: 6px;
  bottom: 6px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  pointer-events: none;
  opacity: 0.96;
}
.meizi-live img {
  height: 96px;
  width: auto;
  border-radius: 10px;
  filter: drop-shadow(0 4px 10px rgba(0, 0, 0, 0.18));
}
.meizi-live span {
  font-size: 10.5px;
  color: var(--muted);
}

.ai-panel {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  position: relative;
}
.drop-hint {
  position: absolute;
  inset: 0;
  z-index: 5;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 2px dashed var(--accent);
  border-radius: 12px;
  background: color-mix(in srgb, var(--accent) 6%, transparent);
  color: var(--accent);
  font-size: 12.5px;
  pointer-events: none;
}
.tabs,
.head,
.chips {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: nowrap;
  overflow-x: auto;
  scrollbar-width: none;
}
.tabs::-webkit-scrollbar,
.head::-webkit-scrollbar,
.chips::-webkit-scrollbar {
  display: none;
}
.tabs > *,
.head > *,
.chips > * {
  flex: none;
}
.tab {
  border: 1px solid var(--border-soft);
  background: var(--card);
  color: var(--text-soft);
  border-radius: 8px;
  height: 26px;
  padding: 0 10px;
  font-family: inherit;
  font-size: 11.5px;
  white-space: nowrap;
  cursor: pointer;
}
.tab.on {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}
.scene {
  height: 28px;
  font-size: 11.5px;
  min-width: 96px;
  max-width: 150px;
  /* 只给左右内边距：给了上下内边距 + 固定高度，中文字会只露上半截 */
  padding: 0 8px;
}
.more {
  border: none;
  background: transparent;
  color: var(--muted);
  font-family: inherit;
  font-size: 12px;
  cursor: pointer;
  padding: 2px 6px;
  white-space: nowrap;
}
.more:hover {
  color: var(--accent);
}
.pop {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  border: 1px solid var(--border-soft);
  border-radius: 10px;
  background: var(--card);
  padding: 6px;
}
.pop button {
  border: none;
  background: transparent;
  font-family: inherit;
  font-size: 12px;
  color: var(--text-soft);
  padding: 4px 8px;
  border-radius: 6px;
  cursor: pointer;
  text-align: left;
  width: 100%;
}
.pop button:hover {
  background: var(--card-hover);
  color: var(--accent);
}
.note {
  font-size: 11px;
  color: var(--muted);
}
.chat {
  flex: 1;
  min-height: 120px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 6px 2px;
}
.msg.assistant {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.msg.user {
  display: flex;
  flex-direction: column;
  gap: 6px;
  align-items: flex-end;
}
.user-text {
  background: var(--card-alt);
  border: 1px solid var(--border-soft);
  border-radius: 14px;
  padding: 8px 11px;
  font-size: 13px;
  line-height: 1.6;
  white-space: pre-wrap;
  max-width: 92%;
  overflow-wrap: anywhere;
}
.attach-cards {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  justify-content: flex-end;
}
.file-card {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border: 1px solid var(--border-soft);
  background: var(--card);
  border-radius: 12px;
  padding: 6px 10px 6px 6px;
  max-width: 240px;
}
.thumb {
  width: 34px;
  height: 34px;
  border-radius: 8px;
  object-fit: cover;
  flex: none;
}
.file-icon {
  width: 34px;
  height: 34px;
  border-radius: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 9.5px;
  font-weight: 650;
  flex: none;
}
.file-icon.rose {
  background: color-mix(in srgb, var(--danger) 12%, transparent);
  color: var(--danger);
}
.file-icon.sky {
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
}
.file-icon.violet {
  background: #f3edfe;
  color: #6c3fa6;
}
.file-icon.teal {
  background: color-mix(in srgb, var(--success) 14%, transparent);
  color: color-mix(in srgb, var(--success) 75%, var(--text));
}
.file-meta {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
}
.file-name {
  font-size: 12px;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.file-sub {
  font-size: 10.5px;
  color: var(--faint);
}
.elapsed {
  font-size: 11px;
  color: var(--faint);
}
.actions-line {
  display: flex;
  gap: 2px;
  margin-top: 2px;
}
.actions-line button {
  border: none;
  background: transparent;
  color: var(--faint);
  font-size: 12px;
  cursor: pointer;
  padding: 2px 5px;
  border-radius: 6px;
}
.actions-line button:hover {
  color: var(--accent);
  background: var(--card-alt);
}
.reasoning {
  border-left: 2px solid var(--border-soft);
  padding-left: 8px;
  font-size: 11.5px;
  color: var(--muted);
}
.reasoning summary {
  cursor: pointer;
  color: var(--faint);
}
.reasoning-body {
  margin-top: 4px;
  white-space: pre-wrap;
  line-height: 1.6;
}
.tools-used {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  align-items: center;
}
.tool-head {
  font-size: 10.5px;
  color: var(--muted);
}
.tool-chip {
  font-size: 10px;
  padding: 1px 6px;
  border-radius: 999px;
  border: 1px solid var(--border-soft);
  color: var(--muted);
}
.tool-chip.bad {
  color: var(--danger);
}
.mem-note {
  font-size: 10.5px;
  color: var(--faint);
}
.typing {
  color: var(--muted);
  font-size: 12px;
}
.error {
  color: var(--danger);
  font-size: 12px;
}
.empty {
  color: var(--muted);
  font-size: 12.5px;
  line-height: 1.7;
  padding: 16px 6px;
}
/* 撰写区：大圆角卡片，像参考图那样底部一排操作 */
.composer {
  border: 1px solid var(--border-soft);
  background: var(--card);
  border-radius: 16px;
  padding: 10px 10px 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
}
.chips {
  font-size: 10.5px;
  color: var(--muted);
}
.attached {
  color: var(--text-soft);
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.attach-note {
  color: var(--faint);
}
.usage {
  color: var(--muted);
  white-space: nowrap;
}
.mini {
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text-soft);
  border-radius: 7px;
  height: 22px;
  padding: 0 8px;
  font-family: inherit;
  font-size: 10.5px;
  cursor: pointer;
  white-space: nowrap;
}
.mini.on {
  border-color: var(--accent);
  color: var(--accent);
}
.input {
  width: 100%;
  border: none;
  background: transparent;
  resize: none;
  font-family: inherit;
  font-size: 13.5px;
  color: var(--text);
  line-height: 1.6;
  outline: none;
}
.composer-foot {
  display: flex;
  align-items: center;
  gap: 6px;
}
/* ＋ 必须真正居中：用 flex + 行高 0 的 span */
.plus {
  width: 28px;
  height: 28px;
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  color: var(--text-soft);
  cursor: pointer;
  padding: 0;
  line-height: 0;
}
.plus span {
  font-size: 15px;
  line-height: 1;
  display: block;
}
.plus:hover {
  color: var(--accent);
  border-color: var(--accent);
}
/* 现在的 API 配置：给个看得清的胶囊外形 + 深一点的字，放不下就在末尾省略 */
.profile {
  height: 24px;
  font-size: 11px;
  max-width: 118px;
  min-width: 56px;
  flex: 0 1 auto;
  border: 1px solid var(--border-soft);
  border-radius: 999px;
  background: var(--card-alt);
  color: var(--text-soft);
  padding: 0 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}
/* 圆形发送键 */
.send {
  width: 30px;
  height: 30px;
  flex: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  border: none;
  background: var(--accent);
  color: #fff;
  font-size: 14px;
  line-height: 1;
  cursor: pointer;
  padding: 0;
}
.send:disabled {
  opacity: 0.45;
  cursor: default;
}
.send.busy {
  background: var(--danger);
}
.sandbox {
  display: flex;
  align-items: center;
  gap: 6px;
  /* 右边的 AI 栏只有 250px：允许换行，宁可占两行也不要把字切掉半截 */
  flex-wrap: wrap;
  row-gap: 4px;
  font-size: 11px;
  /* 原来是 --faint（#a1a1a6）+ 10.5px，白底上像「只显示了一半」，现在用 --muted */
  color: var(--muted);
}
.sb-label {
  color: var(--text-soft);
  font-weight: 600;
}
.sb-path {
  /* 不再用 ellipsis：放不下就换行，绝不出现「字只有一半」 */
  white-space: normal;
  overflow-wrap: anywhere;
}
.sb-dot {
  font-size: 9px;
  line-height: 1;
  color: var(--accent);
}
.sb-help {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px solid var(--border-soft);
  background: var(--card-alt);
  font-size: 11.5px;
  line-height: 1.65;
  color: var(--text-soft);
}
.sb-help p {
  margin: 0;
}
.sb-where {
  color: var(--muted);
  overflow-wrap: anywhere;
}
.history {
  border: 1px solid var(--border-soft);
  border-radius: 10px;
  background: var(--card);
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 220px;
  overflow-y: auto;
}
.history-head {
  display: flex;
  align-items: center;
  font-size: 11.5px;
  color: var(--muted);
}
.history-row {
  display: flex;
  flex-direction: column;
  gap: 2px;
  text-align: left;
  border: none;
  background: transparent;
  padding: 4px 6px;
  border-radius: 8px;
  font-family: inherit;
  cursor: pointer;
}
.history-row:hover {
  background: var(--card-hover);
}
.history-row .name {
  font-size: 12px;
  color: var(--text);
}
.history-row .meta {
  font-size: 10.5px;
  color: var(--faint);
}
.history-item {
  display: flex;
  flex-direction: column;
  gap: 3px;
  border-radius: 8px;
  padding: 2px;
}
.history-item:hover .row-tools {
  opacity: 1;
}
.row-tools {
  display: flex;
  gap: 4px;
  padding: 0 4px 4px;
  /* 平时淡一点，鼠标移到这条上才明显；不用 display:none，免得键盘用户摸不到 */
  opacity: 0.55;
}
.rename-row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 4px 4px;
}
.rename-input {
  flex: 1;
  min-width: 0;
  height: 24px;
  padding: 0 6px;
  font-size: 11.5px;
}
.mini.danger {
  border-color: var(--danger);
  color: var(--danger);
}
.mini.primary {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 10%, var(--card-alt));
  color: var(--accent);
}
/* 工具审核卡片：挡在撰写区上面，等用户点 */
.confirm-card {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 9px 11px;
  border-radius: 11px;
  border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border-soft));
  background: color-mix(in srgb, var(--accent) 7%, var(--card));
}
.confirm-head {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.confirm-title {
  font-size: 12.5px;
  font-weight: 650;
  color: var(--text);
}
.confirm-risk {
  font-size: 10px;
  color: var(--danger);
  border: 1px solid var(--danger);
  border-radius: 999px;
  padding: 0 5px;
  line-height: 14px;
}
.confirm-why {
  font-size: 11.5px;
  color: var(--text-soft);
  line-height: 1.6;
}
.confirm-target {
  font-size: 11px;
  color: var(--muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.confirm-actions {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
  padding-top: 2px;
}
/* 流式时正文末尾的闪烁光标 */
.md.typing2::after {
  content: "▍";
  color: var(--accent);
  animation: caret-blink 1s steps(2, start) infinite;
}
@keyframes caret-blink {
  to {
    visibility: hidden;
  }
}
.spacer {
  flex: 1;
}
</style>

<style>
/* Markdown 渲染出来的内容在 scoped 之外，单独给一层 */
.ai-panel .md h1,
.ai-panel .md h2,
.ai-panel .md h3 {
  font-size: 14px;
  margin: 8px 0 4px;
}
.ai-panel .md p {
  margin: 4px 0;
}
.ai-panel .md pre {
  background: var(--card-alt);
  padding: 8px;
  border-radius: 8px;
  overflow-x: auto;
  font-size: 12px;
}
.ai-panel .md code {
  font-family: var(--font-mono);
  font-size: 12px;
}
.ai-panel .md ul,
.ai-panel .md ol {
  margin: 4px 0 4px 18px;
}
</style>
