<script setup lang="ts">
// 看板娘悬浮球（用户 2026-11-12）：把文件或文字拖到球上 → 直接发给 AI；也给一个输入框兜底。
//
// 这个页面跑在独立的置顶小窗里（`mascot.html` 独立入口），所以不套主界面外壳。
// 主题跟随主界面：`mascot.ts` 会把设置里的 theme_* 读进来（不能只看 localStorage，
// 那样系统是深色、软件是浅色时，小窗就跟主界面反着来）。
import { onBeforeUnmount, onMounted, ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { mascotClose, mascotSend } from "../api";

const text = ref("");
const dragging = ref(false);
const note = ref("");
const busy = ref(false);
/** 拖进来还没提交的文件 */
const pending = ref<string[]>([]);

let unlisten: (() => void) | null = null;

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
        // 文件直接发；拖进来的文字（没有路径）用输入框
        void submit("", paths);
      }
    });
  } catch {
    /* 浏览器里没有这个 API */
  }
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

onMounted(setupDrop);
onBeforeUnmount(() => unlisten?.());
</script>

<template>
  <div class="ball" :class="{ dragging }">
    <header class="bar" data-tauri-drag-region>
      <span class="dot" data-tauri-drag-region />
      <span class="title" data-tauri-drag-region>看板娘</span>
      <span class="spacer" data-tauri-drag-region />
      <button class="x" title="关掉看板娘" @click="close">×</button>
    </header>

    <div class="body">
      <div class="orb" :class="{ hot: dragging }">旦</div>
      <div class="hint">
        {{ dragging ? "松手就发给 AI" : "把文件拖到这儿，或说一句" }}
      </div>
      <input
        v-model="text"
        class="say"
        type="text"
        placeholder="跟 AI 说点什么…"
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
  gap: 0;
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
  padding: 0 12px 0 12px;
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
.body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  justify-content: center;
  gap: 7px;
  padding: 2px 12px 12px;
}
.orb {
  align-self: center;
  width: 48px;
  height: 48px;
  margin-top: 2px;
  border-radius: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 21px;
  font-weight: 700;
  color: var(--accent-deep);
  background: linear-gradient(
    145deg,
    color-mix(in srgb, var(--accent) 14%, var(--card)),
    color-mix(in srgb, var(--accent) 34%, var(--card))
  );
  border: 1px solid color-mix(in srgb, var(--accent) 22%, var(--border-soft));
  box-shadow: 0 6px 14px color-mix(in srgb, var(--accent) 20%, transparent);
  transition: transform 0.15s ease, box-shadow 0.15s ease;
  pointer-events: none;
}
.orb.hot {
  transform: scale(1.08);
  box-shadow: 0 8px 20px color-mix(in srgb, var(--accent) 34%, transparent);
}
.hint {
  font-size: 10.5px;
  line-height: 1.35;
  color: var(--muted);
  text-align: center;
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
