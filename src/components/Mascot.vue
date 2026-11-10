<script setup lang="ts">
// 吉祥物（对应旧版 mascot.py）：可以在窗口里拖动的小角色，点一下说句话。
// 设置键：mascot_on（开关）、mascot_style（cat/blob）、mascot_image（自定义图片）、mascot_pos（位置 JSON）
import { onMounted, ref } from "vue";

const props = withDefaults(
  defineProps<{ style?: string; image?: string; pos?: string }>(),
  { style: "cat", image: "", pos: "" },
);
const emit = defineEmits<{ (e: "move", pos: string): void; (e: "hide"): void }>();

const x = ref(24);
const y = ref(24);
const talking = ref("");
const tipIndex = ref(0);
const tips = [
  "今天先做最重要的那一件。",
  "写不出来就先列提纲。",
  "专注 25 分钟，休息 5 分钟。",
  "记得喝水。",
  "卡住的时候，把问题讲给别人听（或者讲给我）。",
];

function loadPos() {
  if (!props.pos) return;
  try {
    const parsed = JSON.parse(props.pos);
    if (typeof parsed.x === "number" && typeof parsed.y === "number") {
      x.value = parsed.x;
      y.value = parsed.y;
    }
  } catch {
    // 位置坏了就回默认
  }
}

function startDrag(event: MouseEvent) {
  const startX = event.clientX;
  const startY = event.clientY;
  const baseX = x.value;
  const baseY = y.value;
  const onMove = (move: MouseEvent) => {
    x.value = Math.max(0, baseX + (move.clientX - startX));
    y.value = Math.max(0, baseY + (move.clientY - startY));
  };
  const onUp = () => {
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
    emit("move", JSON.stringify({ x: Math.round(x.value), y: Math.round(y.value) }));
  };
  window.addEventListener("mousemove", onMove);
  window.addEventListener("mouseup", onUp);
}

function speak() {
  talking.value = tips[tipIndex.value % tips.length];
  tipIndex.value += 1;
  window.setTimeout(() => {
    if (talking.value === tips[(tipIndex.value - 1) % tips.length]) talking.value = "";
  }, 4000);
}

onMounted(loadPos);
</script>

<template>
  <div
    class="mascot"
    :style="{ left: x + 'px', top: y + 'px' }"
    title="点我说话，拖我换位置"
    @mousedown.prevent="startDrag"
    @click="speak"
  >
    <img v-if="image" class="pic" :src="image" alt="mascot" />
    <svg v-else-if="style === 'blob'" class="pic" viewBox="0 0 64 64">
      <ellipse cx="32" cy="34" rx="26" ry="24" fill="var(--accent)" opacity="0.85" />
      <circle cx="24" cy="30" r="4" fill="#fff" />
      <circle cx="40" cy="30" r="4" fill="#fff" />
      <circle cx="25" cy="31" r="2" fill="#1c1c1e" />
      <circle cx="41" cy="31" r="2" fill="#1c1c1e" />
    </svg>
    <svg v-else class="pic" viewBox="0 0 64 64">
      <ellipse cx="32" cy="40" rx="22" ry="18" fill="var(--accent)" opacity="0.9" />
      <path d="M14 26 L20 12 L28 24 Z" fill="var(--accent)" opacity="0.9" />
      <path d="M50 26 L44 12 L36 24 Z" fill="var(--accent)" opacity="0.9" />
      <circle cx="25" cy="38" r="3.4" fill="#fff" />
      <circle cx="39" cy="38" r="3.4" fill="#fff" />
      <circle cx="26" cy="39" r="1.8" fill="#1c1c1e" />
      <circle cx="40" cy="39" r="1.8" fill="#1c1c1e" />
      <path d="M28 46 Q32 50 36 46" stroke="#fff" stroke-width="2" fill="none" />
    </svg>
    <div v-if="talking" class="bubble">{{ talking }}</div>
    <button class="close" title="收起吉祥物" @click.stop="emit('hide')">×</button>
  </div>
</template>

<style scoped>
.mascot {
  position: fixed;
  z-index: 55;
  width: 64px;
  height: 64px;
  cursor: grab;
  user-select: none;
  filter: drop-shadow(0 6px 14px rgba(0, 0, 0, 0.18));
  animation: bob 3.2s ease-in-out infinite;
}
.mascot:active {
  cursor: grabbing;
}
.pic {
  width: 64px;
  height: 64px;
  display: block;
  border-radius: 18px;
}
.bubble {
  position: absolute;
  left: 72px;
  top: 4px;
  width: 200px;
  padding: 8px 10px;
  border-radius: 12px;
  background: var(--material-card, var(--card));
  border: 1px solid var(--hairline, var(--border-soft));
  backdrop-filter: var(--material-blur, none);
  box-shadow: var(--shadow-pop, none);
  font-size: 12px;
  line-height: 1.5;
  color: var(--text);
  text-align: left;
}
.close {
  position: absolute;
  right: -6px;
  top: -6px;
  width: 18px;
  height: 18px;
  border-radius: 999px;
  border: 1px solid var(--hairline, var(--border-soft));
  background: var(--card);
  color: var(--muted);
  font-size: 12px;
  line-height: 1;
  padding: 0;
  cursor: pointer;
  text-align: center;
  opacity: 0;
  transition: opacity 120ms ease;
}
.mascot:hover .close {
  opacity: 1;
}
@keyframes bob {
  0%,
  100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-4px);
  }
}
</style>
