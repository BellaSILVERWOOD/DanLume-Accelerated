// 看板娘悬浮球小窗的入口（独立 HTML 页面 `mascot.html`，只挂 MascotBall 一个组件）
//
// 为什么不复用主界面 + hash 判断：实测在 Tauri 的 asset 协议下 hash 会被吞掉，
// 结果主界面被塞进 168px 的小窗里。单独一个 entry 最稳。
//
// 这里**故意不 import `./api`**（那个模块 2900 行，主界面那一堆东西都在里面）：
// 小窗只需要三个命令，直接 invoke 就够。api 模块里任何一处模块级问题都会让整个小窗白屏，
// 而小窗一旦白屏，页面上的 × 也点不到 —— 用户就「关不掉看板娘」了（2026-11-14 反馈）。
import { createApp } from "vue";
import { invoke } from "@tauri-apps/api/core";
import MascotBall from "./components/MascotBall.vue";
import "./styles/tokens.css";
import "./styles/base.css";
import { apply, loadFromSettings } from "./theme";

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
    const all = await invoke<{ values: Record<string, string> }>("settings_all");
    loadFromSettings(all?.values || {});
  } catch {
    // 读不到设置（刚启动 / 服务器异常）就先用本机缓存的默认值
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

// Esc 直接关掉小窗（用户 2026-11-14：看板娘关不掉 → 至少给个键盘出口）
window.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  event.preventDefault();
  invoke("mascot_close").catch(() => undefined);
});

// 小窗里也不要浏览器右键菜单
document.addEventListener("contextmenu", (event) => {
  const target = event.target as HTMLElement;
  if (!target.closest("input, textarea")) event.preventDefault();
});
