import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

const repoRoot = path.resolve(__dirname, "..");

/*
 * 下载页（GitHub Pages）构建配置。
 *
 * base 用相对路径 `./`：项目站点（yinxe.github.io/dsh-starter/）与将来绑自定义域
 * （根路径）都能直接跑，不需要改配置 —— 这个页面没有前端路由，相对资源路径是安全的。
 *
 * 页面要用仓库里的品牌资源（public/dsh-logo.svg）与截图（images/preview*.png），
 * 它们都在站点目录之外，所以 dev server 的 fs 白名单要放开到仓库根目录。
 */
export default defineConfig({
  base: "./",
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    fs: {
      allow: [repoRoot],
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "es2020",
    sourcemap: false,
  },
});
