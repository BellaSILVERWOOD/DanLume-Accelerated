<script setup lang="ts">
// 看板娘「梅子」（用户 2026-11-14 命名）：把文件或文字拖到小窗上 → 直接发给 AI；也给输入框兜底。
//
// 这个页面跑在独立的置顶小窗里（`mascot.html` 独立入口），所以不套主界面外壳。
// 主题跟随主界面：`mascot.ts` 会把设置里的 theme_* 读进来（不能只看 localStorage，
// 那样系统是深色、软件是浅色时，小窗就跟主界面反着来）。
//
// 形象（用户 2026-11-14 指定）：
// * 待机用精灵表导出的 `meizi-idle.webp`（8 帧动画）；
// * 拖窗口时看方向：**向左拖显示向左走的图、向右拖显示向右走的图**（用户 2026-11-14 指定的两张）；
// * 正面立绘 `meizi-front.png` 是静态示意图，动画没出来时当兜底。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";

// 自己 invoke，不 import `../api`：小窗只要三个命令，少一个模块就少一种白屏的可能
// （白屏时 × 也按不到，用户就关不掉看板娘了 —— 2026-11-14 反馈）。
const mascotClose = () => invoke("mascot_close");
const mascotSend = (text: string, paths: string[]) => invoke("mascot_send", { text, paths });

const text = ref("");
const dragging = ref(false);
const note = ref("");
const busy = ref(false);
/** 拖进来还没提交的文件 */
const pending = ref<string[]>([]);
/** 形象朝向：待机 / 往左看 / 往右看 */
const facing = ref<"idle" | "left" | "right">("idle");
const idleBroken = ref(false);

const figureSrc = computed(() => {
  if (facing.value === "left") return "/mascot/meizi-walk-left.webp";
  if (facing.value === "right") return "/mascot/meizi-walk-right.webp";
  return idleBroken.value ? "/mascot/meizi-front.png" : "/mascot/meizi-idle.webp";
});

let unlisten: (() => void) | null = null;
let dragTimer = 0;
let lastX: number | null = null;
let idleTimer = 0;

async function setupDrop() {
  try {
    unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload as { type: string; paths?: string[] };
      if (payload.type === "enter") dragging.value = true;
      else if (payload.type === "leave") dragging.value = false;
      else if (payload.type === "drop") {
        dragging.value = false;
        const paths = payload.paths || [];
        if (!paths.length) return;
        void submit("", paths);
      }
    });
  } catch {
    /* 浏览器里没有这个 API */
  }
}

/**
 * 拖动时看方向换脸：鼠标按下开始每 90ms 读一次窗口位置，
 * 位置变小 → 向左拖（显示左侧），变大 → 向右拖（显示右侧）。
 */
async function watchDragDirection() {
  if (dragTimer) return;
  try {
    const pos = await getCurrentWindow().outerPosition();
    lastX = pos.x;
  } catch {
    return;
  }
  dragTimer = window.setInterval(async () => {
    try {
      const pos = await getCurrentWindow().outerPosition();
      if (lastX !== null) {
        if (pos.x < lastX) facing.value = "left";
        else if (pos.x > lastX) facing.value = "right";
      }
      lastX = pos.x;
    } catch {
      /* 读不到位置就不换脸 */
    }
  }, 90);
}

function stopDragDirection() {
  if (dragTimer) {
    window.clearInterval(dragTimer);
    dragTimer = 0;
  }
  lastX = null;
  window.clearTimeout(idleTimer);
  idleTimer = window.setTimeout(() => {
    facing.value = "idle";
  }, 260);
}

async function submit(withText = text.value, paths = pending.value) {
  if (busy.value) return;
  const content = withText.trim();
  if (!content && !paths.length) {
    note.value = "拖个文件进来，或者写一句话";
    return;
  }
  busy.value = true;
  note.value = "已交给 AI…";
  try {
    await mascotSend(content, paths);
    text.value = "";
    pending.value = [];
    note.value = "已发到 AI 面板";
  } catch (err) {
    note.value = `发送失败：${String(err)}`;
  } finally {
    busy.value = false;
  }
}

function close() {
  void mascotClose();
}

onMounted(async () => {
  await setupDrop();
  window.addEventListener("mouseup", stopDragDirection);
});
onBeforeUnmount(() => {
  unlisten?.();
  window.removeEventListener("mouseup", stopDragDirection);
  if (dragTimer) window.clearInterval(dragTimer);
  window.clearTimeout(idleTimer);
});
</script>

<template>
  <div class="ball" :class="{ dragging }">
    <header class="bar" data-tauri-drag-region @mousedown="watchDragDirection">
      <span class="dot" data-tauri-drag-region />
      <span class="title" data-tauri-drag-region>梅子</span>
      <span class="spacer" data-tauri-drag-region />
      <button class="x" title="关掉梅子（或按 Esc）" @click="close">×</button>
    </header>

    <div class="stage" @mousedown="watchDragDirection">
      <img
        class="figure"
        :src="figureSrc"
        :class="{ hot: dragging, look: facing !== 'idle' }"
        alt="梅子"
        draggable="false"
        @error="idleBroken = true"
      />
      <div v-if="dragging" class="drop-hint">松手就发给 AI</div>
    </div>

    <div class="body">
      <input
        v-model="text"
        class="say"
        type="text"
        placeholder="跟梅子说点什么…"
        spellcheck="false"
        @keydown.enter.prevent="submit()"
      />
      <button class="send" :disabled="busy" @click="submit()">
        {{ busy ? "发送中…" : "发给 AI" }}
      </button>
      <div v-if="note" class="note">{{ note }}</div>
    </div>
  </div>
</template>

<style scoped>
.ball {
  width: 100vw;
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: linear-gradient(180deg, var(--card) 0%, var(--card-alt) 100%);
  border: 1px solid var(--border-soft);
  overflow: hidden;
  user-select: none;
}
.ball.dragging {
  border-color: var(--accent);
}
.bar {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 26px;
  padding: 0 12px;
  flex: 0 0 auto;
}
.dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  background: var(--accent);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 16%, transparent);
}
.title {
  font-size: 11px;
  font-weight: 600;
  color: var(--muted);
  letter-spacing: 0.2px;
}
.spacer {
  flex: 1;
}
.x {
  border: none;
  background: transparent;
  color: var(--faint);
  font-size: 13px;
  line-height: 1;
  padding: 2px 3px;
  border-radius: 6px;
  cursor: pointer;
}
.x:hover {
  color: var(--text-soft);
  background: var(--card-hover);
}

/* 形象区：占满中间，图片按比例缩放（不裁脸） */
.stage {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  overflow: hidden;
}
.figure {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
  object-position: bottom center;
  transition: transform 0.12s ease;
  pointer-events: none;
}
.figure.hot {
  transform: translateY(-2px) scale(1.02);
}
.figure.look {
  /* 侧脸图稍窄，稍微放大一点填满同一视觉高度 */
  transform: scale(1.04);
}
.drop-hint {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 8px;
  text-align: center;
  font-size: 11px;
  color: var(--accent);
  font-weight: 600;
}

.body {
  flex: 0 0 auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 6px 12px 10px;
}
.say {
  height: 28px;
  padding: 0 9px;
  font-size: 11.5px;
  border-radius: 9px;
}
.send {
  height: 28px;
  border-radius: 9px;
  border: 1px solid transparent;
  background: var(--accent);
  color: #fff;
  font-family: inherit;
  font-size: 11.5px;
  font-weight: 600;
  text-align: center;
  cursor: pointer;
}
.send:hover:not(:disabled) {
  background: var(--accent-deep);
}
.send:disabled {
  background: var(--card-alt);
  border-color: var(--border-soft);
  color: var(--faint);
  cursor: default;
}
.note {
  font-size: 10.5px;
  line-height: 1.4;
  color: var(--muted);
  text-align: center;
}
</style>
