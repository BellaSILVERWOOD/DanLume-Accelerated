import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri 开发时前端跑在固定端口，Rust 端 tauri.conf.json 里 devUrl 指的就是它
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5273,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "chrome110",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    rollupOptions: {
      // 两个页面：主界面 + 看板娘悬浮球小窗（单独 entry，避免 hash 被 asset 协议吞掉）
      input: {
        main: "index.html",
        mascot: "mascot.html",
      },
    },
  },
});
