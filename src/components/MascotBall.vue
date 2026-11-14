<script setup lang="ts">
// 看板娘「梅子」悬浮球（用户 2026-11-14）：桌面上只有一个人物 + 底下一个聊天气泡。
//
// 用户明确要求：
// * **不要对话框背景**（透明窗口，人物直接浮在桌面上）；
// * 下面是**一个聊天气泡**（输入框 + 发送），不要把整块卡片框起来；
// * 文件拖到人物身上就能发给 AI（`onDragDropEvent` 已经能收到）；
// * 人物是**动图**（精灵表导出、按质心对齐的 8 帧），拖动时看方向换左右走路的图。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";

// 自己 invoke，不 import `../api`：小窗只要三个命令，少一个模块就少一种白屏的可能。
const mascotClose = () => invoke("mascot_close");
const mascotSend = (text: string, paths: string[]) => invoke("mascot_send", { text, paths });

const text = ref("");
const dragging = ref(false);
const note = ref("");
const busy = ref(false);
const pending = ref<string[]>([]);
/** 形象朝向：待机 / 往左走 / 往右走 */
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

/** 拖窗口时看方向换图：位置变小 → 向左走，变大 → 向右走 */
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
      /* 读不到位置就不换图 */
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
    note.value = "拖个文件到梅子身上，或者写一句话";
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
  <div class="ball">
    <!-- 人物：整块都是拖动区域，文件也是拖到这儿 -->
    <div class="stage" data-tauri-drag-region @mousedown="watchDragDirection">
      <img
        class="figure"
        :src="figureSrc"
        :class="{ hot: dragging, look: facing !== 'idle' }"
        alt="梅子"
        draggable="false"
        data-tauri-drag-region
        @error="idleBroken = true"
      />
      <button class="x" title="关掉梅子（也可以按 Esc）" @click="close">×</button>
      <div v-if="dragging" class="drop-hint">松手就发给 AI</div>
    </div>

    <!-- 底下一个聊天气泡（不要卡片背景） -->
    <div class="bubble">
      <input
        v-model="text"
        class="say"
        type="text"
        placeholder="跟梅子说点什么…"
        spellcheck="false"
        @keydown.enter.prevent="submit()"
      />
      <button class="send" :disabled="busy" title="发给 AI" @click="submit()">
        {{ busy ? "…" : "↑" }}
      </button>
    </div>
    <div v-if="note" class="note">{{ note }}</div>
  </div>
</template>

<style scoped>
/* 整块透明：桌面上只看得到人物和气泡 */
.ball {
  width: 100vw;
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: transparent;
  user-select: none;
  overflow: visible;
}
.stage {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  background: transparent;
}
.figure {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
  object-position: bottom center;
  transition: transform 0.12s ease, filter 0.12s ease;
  pointer-events: none;
  filter: drop-shadow(0 8px 16px rgba(0, 0, 0, 0.22));
}
.figure.hot {
  filter: drop-shadow(0 0 2px var(--accent, #731c32)) drop-shadow(0 8px 18px rgba(0, 0, 0, 0.25));
}
.figure.look {
  transform: scale(1.03);
}
.x {
  position: absolute;
  top: 2px;
  right: 2px;
  width: 20px;
  height: 20px;
  border-radius: 999px;
  border: none;
  background: rgba(0, 0, 0, 0.28);
  color: #fff;
  font-size: 12px;
  line-height: 1;
  cursor: pointer;
  opacity: 0.7;
}
.x:hover {
  opacity: 1;
}
.drop-hint {
  position: absolute;
  left: 50%;
  bottom: 12px;
  transform: translateX(-50%);
  padding: 3px 9px;
  border-radius: 999px;
  background: var(--accent, #731c32);
  color: #fff;
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
}

/* 聊天气泡：半透明圆角，输入框融进去 */
.bubble {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0 6px 4px;
  padding: 4px 5px 4px 12px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.92);
  border: 1px solid rgba(0, 0, 0, 0.08);
  box-shadow: 0 6px 16px rgba(0, 0, 0, 0.16);
}
.say {
  flex: 1;
  min-width: 0;
  height: 26px;
  padding: 0;
  border: none;
  background: transparent;
  font-size: 11.5px;
  color: #1d1d1f;
  outline: none;
}
.send {
  flex: 0 0 auto;
  width: 26px;
  height: 26px;
  border-radius: 999px;
  border: none;
  background: var(--accent, #731c32);
  color: #fff;
  font-size: 13px;
  font-weight: 700;
  cursor: pointer;
  text-align: center;
  padding: 0;
}
.send:disabled {
  background: #a1a1a6;
  cursor: default;
}
.note {
  flex: 0 0 auto;
  text-align: center;
  font-size: 10.5px;
  color: #fff;
  text-shadow: 0 1px 3px rgba(0, 0, 0, 0.55);
  padding-bottom: 2px;
}
</style>
