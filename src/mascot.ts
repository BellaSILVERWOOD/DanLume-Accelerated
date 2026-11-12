// 看板娘悬浮球小窗的入口（独立 HTML 页面 `mascot.html`，只挂 MascotBall 一个组件）
//
// 为什么不复用主界面 + hash 判断：实测在 Tauri 的 asset 协议下 hash 会被吞掉，
// 结果主界面被塞进 168px 的小窗里。单独一个 entry 最稳。
import { createApp } from "vue";
import MascotBall from "./components/MascotBall.vue";
import "./styles/tokens.css";
import "./styles/base.css";
import { apply, loadFromSettings } from "./theme";
import { settingsAll } from "./api";

/** 把错误写进 mascot.html 里那行「启动中」，别让 168px 的窗口只剩白屏 */
function fail(why: string): void {
  const box = document.getElementById("boot");
  if (!box) return;
  box.textContent = `看板娘启动失败：${why}`;
  box.style.color = "#C0392B";
  box.style.background = "#FFF";
}

/**
 * 主题要跟**主界面**一致：主界面的深浅色 / 强调色存在数据库设置里（theme_mode 等），
 * localStorage 只是缓存。只读 localStorage 的话，系统是深色而软件设了浅色时，
 * 小窗会是深色（用户 2026-11-12 报的就是这个）。
 */
async function syncTheme(): Promise<void> {
  try {
    const all = await settingsAll();
    loadFromSettings(all.values || {});
  } catch {
    // 读不到设置（服务器没起来 / 首次启动）就先用本机缓存的默认值
    apply();
  }
}

try {
  const app = createApp(MascotBall);
  app.config.errorHandler = (err) => {
    fail(String((err as Error)?.message || err));
    console.error("[mascot]", err);
  };
  app.mount("#app");
  // 挂上了就把「启动中」那行撤掉（组件自己占满 100vw/100vh）
  document.getElementById("boot")?.remove();
  void syncTheme();
  // 在主界面改完主题再回到小窗时，重新对一次色
  window.addEventListener("focus", () => void syncTheme());
} catch (err) {
  fail(String((err as Error)?.message || err));
  console.error("[mascot] mount failed", err);
}

// 小窗里也不要浏览器右键菜单
document.addEventListener("contextmenu", (event) => {
  const target = event.target as HTMLElement;
  if (!target.closest("input, textarea")) event.preventDefault();
});
