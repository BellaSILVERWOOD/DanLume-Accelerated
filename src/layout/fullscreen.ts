// 面板全屏（对应旧版 main_window.set_panel_fullscreen）：
// 文档阅读器 / 作业内嵌网页要把自己铺满整个窗口时，收起标题栏、左导航、右栏。
// 用一个共享的 ref 而不是事件总线：App.vue 读它决定 chrome 显示，页面读/写它请求进出。
import { onBeforeUnmount, ref, watch } from "vue";

/** 是否有页面请求了「面板全屏」 */
export const panelFullscreen = ref(false);

/** 进出全屏；退出时要保证 chrome 回来 */
export function setPanelFullscreen(on: boolean) {
  panelFullscreen.value = !!on;
}

export function togglePanelFullscreen() {
  setPanelFullscreen(!panelFullscreen.value);
}

/**
 * 页面里这样用：
 *   const { active, toggle, exit } = usePanelFullscreen();
 *   toggle()  进出；离开页面时会自动退出（不然切页后 chrome 还藏着）。
 */
export function usePanelFullscreen() {
  const active = ref(false);

  function set(on: boolean) {
    active.value = !!on;
    setPanelFullscreen(on);
  }

  function toggle() {
    set(!active.value);
  }

  function exit() {
    if (active.value) set(false);
  }

  // 别的页面把全屏关掉时，本页状态也跟着复位
  watch(panelFullscreen, (on) => {
    if (!on && active.value) active.value = false;
  });

  onBeforeUnmount(() => {
    if (active.value) set(false);
  });

  return { active, toggle, exit, set };
}
