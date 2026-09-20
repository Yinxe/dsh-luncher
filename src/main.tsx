import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ThemeProvider } from "./lib/theme";
import { TooltipProvider } from "@/components/ui/tooltip";
import WindowChrome from "@/components/WindowChrome";

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
