import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ThemeProvider } from "./lib/theme";
import { TooltipProvider } from "@/components/ui/tooltip";
import WindowChrome from "@/components/WindowChrome";
import { api } from "./api";

// 前端异常回流到 logs/ui.log：UI 上的报错此前只停在用户屏幕上，事后无法复盘。
// 这里只做「最后一道兜底」——业务里的报错 toast 由 addToast 统一上报。
const report = (level: "warn" | "error", message: string) => {
  api.logUi(level, message).catch(() => undefined);
};
window.addEventListener("error", (e) => {
  report("error", `window.onerror: ${e.message} @ ${e.filename}:${e.lineno}:${e.colno}`);
});
window.addEventListener("unhandledrejection", (e) => {
  const r = e.reason as unknown;
  report("error", `unhandledrejection: ${r instanceof Error ? `${r.message}\n${r.stack ?? ""}` : String(r)}`);
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      {/* 侧栏收起态用 tooltip 补全导航名，Provider 在根上统一挂一次 */}
      <TooltipProvider>
        {/* 自绘窗口外壳：包裹整个应用（含加载态），替代原生标题栏 */}
        <WindowChrome>
          <App />
        </WindowChrome>
      </TooltipProvider>
    </ThemeProvider>
  </React.StrictMode>
);
