// 看板娘悬浮球 → AI 面板的「信箱」
//
// 为什么不让 AiPanel 自己 `listen("mascot:send")`：右栏停在「日程 / 文献」标签时
// AiPanel 根本没挂载（`v-else-if`），事件会被丢掉——用户拖进去的东西就没了。
// 所以统一由 App.vue 收事件（它一直在），先放进这个 ref，AiPanel 挂载/显示时取走。
import { ref } from "vue";

export type MascotPayload = { text: string; paths: string[] };

/** 待处理的载荷（null = 没有） */
export const pendingMascot = ref<MascotPayload | null>(null);

/** 取走并清空（同一个载荷不会被执行两次） */
export function takeMascot(): MascotPayload | null {
  const value = pendingMascot.value;
  pendingMascot.value = null;
  return value;
}
