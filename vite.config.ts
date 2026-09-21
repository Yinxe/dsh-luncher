import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

// Tauri expects a fixed port, fail if that port is not available
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  server: {
    port: 1420,
    strictPort: true,
    host: "localhost",
    // 不要 watch Rust 构建产物：`src-tauri/target` 下光文件就 4 万多个（cargo
    // 展开的 registry 源码占绝大多数），chokidar 会给每个文件建一个 inotify
    // watch，直接把 `fs.inotify.max_user_watches`（默认 65536）顶爆 —— vite 进程
    // 以 ENOSPC 崩掉，beforeDevCommand 随即非零退出，`npm run app` 整条起不来。
    // 前端只依赖 src/、index.html、public/；Rust 侧的文件变更由 tauri 自己的
    // watcher 负责，这里用不着看。
    watch: {
      ignored: ["**/src-tauri/target/**", "**/src-tauri/gen/**", "**/target/**"],
    },
  },
  // Env variables starting with TAURI_ENV_* are exposed to the frontend
  clearScreen: false,
  build: {
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
  },
});
