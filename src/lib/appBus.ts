// 应用内的「跳页 + 打开网址」信箱
//
// 场景（用户 2026-11-12）：作业提醒卡片上的「打开」应该用**应用内置**的浏览器打开
// （作业页里贴着 eLearning 的那个子 Webview），而不是弹到系统浏览器。
// 但那个 Webview 只在「作业」页挂载，所以要能：① 切到作业页；② 把网址塞给它。
//
// App.vue 监听 `pendingPage` 切页；HomeworkPage 挂载/刷新时取走 `pendingEmbedUrl`。
import { ref } from "vue";

/** 请求切到哪一页（消费后置空）；类型跟着左导航走，别在这里抄一份 */
export const pendingPage = ref<string | null>(null);
/** 请求用内置浏览器打开哪个地址（消费后置空） */
export const pendingEmbedUrl = ref("");

/** 切到「作业」页，并用内置浏览器打开这个地址 */
export function openInAppBrowser(url: string): void {
  const text = (url || "").trim();
  if (!text) return;
  pendingEmbedUrl.value = text;
  pendingPage.value = "homework";
}
