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
  },
});
