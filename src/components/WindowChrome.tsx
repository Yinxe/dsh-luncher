import { useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/**
 * 自绘窗口外壳：原生标题栏（GNOME/Windows 那条灰带）不好看，这里改成
 * 无边框窗口 + 自绘标题栏，风格跟应用内部完全一致。
 *
 * - 拖动 / 双击最大化：交给 `data-tauri-drag-region`（Tauri 内部处理，
 *   `deep` 表示标题栏子树内任意位置都能拖，button 等可点元素自动豁免）
 * - 窗口按钮：最小化 / 最大化-还原 / 关闭（关闭走 WindowEvent::CloseRequested，
 *   是否「最小化到托盘」由设置里的 close_to_tray 决定）
 * - 拉伸：无边框窗口在 Linux 上丢了原生边缘热区，这里贴边补八向热区
 * - macOS 例外：保留系统装饰（红黄绿灯 + 原生阴影），改用 Overlay 标题栏样式，
 *   所以不画窗口按钮、也不接管拉伸（见 tauri.macos.conf.json）
 */

/** @tauri-apps/api 没导出 ResizeDirection，按 startResizeDragging 的取值手写 */
type ResizeDirection =
  | "North"
  | "South"
  | "East"
  | "West"
  | "NorthEast"
  | "NorthWest"
  | "SouthEast"
  | "SouthWest";

const IS_MAC =
  typeof navigator !== "undefined" && /mac/i.test(navigator.platform || navigator.userAgent);

/** 八向拉伸热区：贴着窗口内缘一圈，尺寸与原生金属边框接近 */
const RESIZE_ZONES: Array<{ dir: ResizeDirection; className: string; cursor: string }> = [
  { dir: "North", className: "top-0 right-3 left-3 h-1", cursor: "cursor-ns-resize" },
  { dir: "South", className: "right-3 bottom-0 left-3 h-1", cursor: "cursor-ns-resize" },
  { dir: "West", className: "top-3 bottom-3 left-0 w-1", cursor: "cursor-ew-resize" },
  { dir: "East", className: "top-3 right-0 bottom-3 w-1", cursor: "cursor-ew-resize" },
  { dir: "NorthWest", className: "top-0 left-0 size-2.5", cursor: "cursor-nwse-resize" },
  { dir: "SouthEast", className: "right-0 bottom-0 size-2.5", cursor: "cursor-nwse-resize" },
  { dir: "NorthEast", className: "top-0 right-0 size-2.5", cursor: "cursor-nesw-resize" },
  { dir: "SouthWest", className: "bottom-0 left-0 size-2.5", cursor: "cursor-nesw-resize" },
];

export default function WindowChrome({ children }: { children: ReactNode }) {
  const [maximized, setMaximized] = useState(false);
  const [focused, setFocused] = useState(true);

  // 窗口状态 → 外壳样式：最大化时贴边（圆角/描边收掉），失焦时标题栏弱化
  useEffect(() => {
    const win = getCurrentWindow();
    let alive = true;
    let unlistenResize: (() => void) | undefined;
    let unlistenFocus: (() => void) | undefined;

    const syncMaximized = () => {
      win
        .isMaximized()
        .then((v) => alive && setMaximized(v))
        .catch(() => undefined);
    };
    syncMaximized();
    win
      .onResized(syncMaximized)
      .then((un) => (alive ? (unlistenResize = un) : un()))
      .catch(() => undefined);
    win
      .onFocusChanged(({ payload }) => alive && setFocused(payload))
      .then((un) => (alive ? (unlistenFocus = un) : un()))
      .catch(() => undefined);

    return () => {
      alive = false;
      unlistenResize?.();
      unlistenFocus?.();
    };
  }, []);

  // 圆角由 --window-radius 统一驱动（弹层遮罩也读它），最大化时置零
  useEffect(() => {
    document.documentElement.classList.toggle("window-maximized", maximized);
  }, [maximized]);

  const run = (fn: () => Promise<unknown>) => () => {
    fn().catch(() => undefined);
  };

  return (
    <div className="app-frame" data-maximized={maximized} data-focused={focused}>
      <div className="window-titlebar" data-tauri-drag-region="deep">
        {/* 品牌标识跟侧栏头部刻意区别开：这里是「窗口标签」，字号更小、色调更弱 */}
        <div className="titlebar-brand flex min-w-0 items-center gap-2">
          <img src="/dsh-logo.svg" alt="DSH" className="h-4 w-4 shrink-0" draggable={false} />
          <span className="truncate text-[12px] font-semibold tracking-tight">DSH Starter</span>
        </div>
        <div className="min-w-2 flex-1" />
        {!IS_MAC && (
          <div className="flex h-full shrink-0 items-stretch">
            <Button
              variant="ghost"
              size="icon"
              className="window-control"
              title="最小化"
              onClick={run(() => getCurrentWindow().minimize())}
            >
              <Minus />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="window-control"
              title={maximized ? "向下还原" : "最大化"}
              onClick={run(() => getCurrentWindow().toggleMaximize())}
            >
              {maximized ? <Copy /> : <Square />}
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="window-control window-control-close"
              title="关闭"
              onClick={run(() => getCurrentWindow().close())}
            >
              <X />
            </Button>
          </div>
        )}
      </div>

      {/* 内容区：撑满剩余高度，保持各自的滚动/溢出行为 */}
      <div className="flex min-h-0 flex-1 flex-col">{children}</div>

      {/* 拉伸热区：无边框窗口的替代方案，压在内容之上、只占贴边几像素 */}
      {!IS_MAC && (
        <div className="pointer-events-none absolute inset-0 z-50">
          {RESIZE_ZONES.map(({ dir, className, cursor }) => (
            <div
              key={dir}
              className={cn("pointer-events-auto absolute", className, cursor)}
              onMouseDown={(e) => {
                if (e.button !== 0) return;
                e.preventDefault();
                getCurrentWindow().startResizeDragging(dir).catch(() => undefined);
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
}
