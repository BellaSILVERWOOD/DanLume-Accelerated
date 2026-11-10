import { createApp } from "vue";
import App from "./App.vue";
import "./styles/tokens.css";
import "./styles/base.css";
import "./theme";

createApp(App).mount("#app");

// 旧版是「关闭收进托盘」，重构版先按真正关闭处理（托盘在后续阶段接）
document.addEventListener("contextmenu", (event) => {
  // 界面上不要弹浏览器右键菜单（旧版也没有）
  const target = event.target as HTMLElement;
  if (!target.closest("input, textarea, [contenteditable='true']")) {
    event.preventDefault();
  }
});
