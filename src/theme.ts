// 主题 + 外观：深浅模式、强调色、背景色调、不透明度、模糊度、磨砂、浏览器风格
// 键名与旧版 theme.py 一致（theme_mode / theme_accent / theme_bg / theme_alpha /
// theme_blur / theme_glass / theme_browser），两个版本共用同一份设置。
import { reactive, watch } from "vue";

export type Mode = "light" | "dark" | "auto";
export type BrowserStyle = "follow" | "light" | "dark";

export const ACCENTS: { name: string; value: string }[] = [
  { name: "系统蓝", value: "#007AFF" },
  { name: "靛蓝", value: "#5856D6" },
  { name: "青", value: "#32ADE6" },
  { name: "绿", value: "#34C759" },
  { name: "粉", value: "#FF2D55" },
  { name: "橙", value: "#FF9500" },
  { name: "紫", value: "#AF52DE" },
  { name: "石墨", value: "#8E8E93" },
];

/** 6 套浅色背景色调（旧版「背景色调」那一排） */
export const BG_TONES: { key: string; name: string; top: string; mid: string; bottom: string }[] = [
  { key: "macos", name: "系统灰", top: "#FBFBFD", mid: "#F5F5F7", bottom: "#EDEDF0" },
  { key: "fog", name: "雾白", top: "#FCFCFD", mid: "#F6F7F9", bottom: "#EFF1F4" },
  { key: "sand", name: "米白", top: "#FDFCF8", mid: "#F8F5EC", bottom: "#F1EDE2" },
  { key: "mint", name: "薄雾绿", top: "#FBFDFB", mid: "#F2F8F3", bottom: "#E8F1EA" },
  { key: "rose", name: "微粉白", top: "#FEFBFC", mid: "#FBF3F5", bottom: "#F5E9ED" },
  { key: "sky", name: "天光蓝", top: "#FBFCFE", mid: "#F2F6FB", bottom: "#E8EFF8" },
];

export const state = reactive({
  mode: (localStorage.getItem("danlume.mode") as Mode) || "auto",
  accent: localStorage.getItem("danlume.accent") || ACCENTS[0].value,
  /** 第二强调色（旧版 theme_accent2），空串表示由主色推导 */
  accent2: "",
  bg: localStorage.getItem("danlume.bg") || "macos",
  alpha: Number(localStorage.getItem("danlume.alpha") || "98"),
  blur: Number(localStorage.getItem("danlume.blur") || "24"),
  glass: (localStorage.getItem("danlume.glass") || "1") === "1",
  browser: (localStorage.getItem("danlume.browser") as BrowserStyle) || "follow",
  browserTabs: (localStorage.getItem("danlume.browsertabs") || "1") === "1",
  translucent: (localStorage.getItem("danlume.translucent") || "0") === "1",
  dark: false,
});

const media = window.matchMedia("(prefers-color-scheme: dark)");

function resolve(mode: Mode): boolean {
  return mode === "dark" || (mode === "auto" && media.matches);
}

export function apply(): void {
  state.dark = resolve(state.mode);
  const root = document.documentElement;
  root.dataset.mode = state.dark ? "dark" : "light";
  root.style.setProperty("--accent", state.accent);
  root.style.setProperty("--accent-deep", shade(state.accent, -0.16));
  // 旧版 theme_accent2：第二强调色（渐变/图表用），没设就由主色推一个
  root.style.setProperty(
    "--accent-2",
    state.accent2 || shade(state.accent, 0.18),
  );

  const tone = BG_TONES.find((item) => item.key === state.bg) || BG_TONES[0];
  root.style.setProperty("--window-top", state.dark ? "#26262A" : tone.top);
  root.style.setProperty("--window-mid", state.dark ? "#212124" : tone.mid);
  root.style.setProperty("--window-bottom", state.dark ? "#1A1A1C" : tone.bottom);

  root.style.setProperty("--win-alpha", String(Math.min(100, Math.max(55, state.alpha)) / 100));
  root.style.setProperty("--win-blur", `${Math.min(60, Math.max(0, state.blur))}px`);
  root.dataset.glass = state.glass ? "on" : "off";
  root.dataset.browser = state.browser;
  root.dataset.browsertabs = state.browserTabs ? "on" : "off";
  root.dataset.translucent = state.translucent ? "on" : "off";
}

/** 把颜色调深 / 调浅一点（旧版 theme.mix 的等价物） */
export function shade(hex: string, amount: number): string {
  const value = hex.replace("#", "");
  const full = value.length === 3 ? value.split("").map((c) => c + c).join("") : value;
  const num = parseInt(full, 16);
  const r = (num >> 16) & 255;
  const g = (num >> 8) & 255;
  const b = num & 255;
  const mix = (c: number) =>
    Math.max(0, Math.min(255, Math.round(c + (amount < 0 ? c : 255 - c) * amount)));
  return `#${[mix(r), mix(g), mix(b)].map((c) => c.toString(16).padStart(2, "0")).join("")}`;
}

function remember(key: string, value: string) {
  localStorage.setItem(`danlume.${key}`, value);
}

export function setMode(mode: Mode): void {
  state.mode = mode;
  remember("mode", mode);
  apply();
}
export function setAccent(value: string): void {
  state.accent = value;
  remember("accent", value);
  apply();
}
export function setBg(key: string): void {
  state.bg = key;
  remember("bg", key);
  apply();
}
export function setAlpha(value: number): void {
  state.alpha = value;
  remember("alpha", String(value));
  apply();
}
export function setBlur(value: number): void {
  state.blur = value;
  remember("blur", String(value));
  apply();
}
export function setGlass(on: boolean): void {
  state.glass = on;
  remember("glass", on ? "1" : "0");
  apply();
}
export function setBrowserStyle(value: BrowserStyle): void {
  state.browser = value;
  remember("browser", value);
  apply();
}
export function setBrowserTabs(on: boolean): void {
  state.browserTabs = on;
  remember("browsertabs", on ? "1" : "0");
  apply();
}
export function setTranslucent(on: boolean): void {
  state.translucent = on;
  remember("translucent", on ? "1" : "0");
  apply();
}
export function toggleMode(): void {
  setMode(resolve(state.mode) ? "light" : "dark");
}

/** 用数据库里的设置覆盖本地默认值（键名与旧版一致） */
export function loadFromSettings(values: Record<string, string>): void {
  const mode = values["theme_mode"];
  if (mode === "light" || mode === "dark" || mode === "auto") state.mode = mode;
  if (values["theme_accent"]) state.accent = values["theme_accent"];
  state.accent2 = (values["theme_accent2"] || "").trim();
  if (values["theme_bg"]) state.bg = values["theme_bg"];
  const alpha = Number(values["theme_alpha"] || "0");
  if (alpha > 0 && alpha <= 1) state.alpha = Math.round(alpha * 100);
  const blur = Number(values["theme_blur"] || "0");
  if (blur > 0) state.blur = blur;
  if (values["theme_glass"] !== undefined && values["theme_glass"] !== "") {
    state.glass = values["theme_glass"] !== "0";
  }
  if (values["theme_browser"]) state.browser = values["theme_browser"] as BrowserStyle;
  apply();
}

media.addEventListener("change", () => {
  if (state.mode === "auto") apply();
});

watch(() => state.mode, apply);
apply();
