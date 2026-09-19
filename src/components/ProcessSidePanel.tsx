import { useEffect, useRef, useState } from "react";
import { Download, Eraser, ExternalLink, Square, Terminal, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Sheet, SheetClose, SheetContent, SheetTitle,
} from "@/components/ui/sheet";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { ProcEntry } from "../types";

interface Props {
  procs: ProcEntry[];
  activeId: number | null;
  open: boolean;
  onClose: () => void;
  onSelect: (id: number) => void;
  onStop: (id: number) => void;
  onOpenWeb: (url: string) => void;
  onExport: () => void;
  onClearExited: () => void;
}

function fmtUptime(startedAt: number): string {
  const s = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(sec)}` : `${pad(m)}:${pad(sec)}`;
}

/** 实例终端抽屉：shadcn Sheet（右侧滑入，遮罩点击/ESC 关闭），实时日志 + 启停 + Web UI */
export default function ProcessSidePanel({
  procs, activeId, open, onClose, onSelect, onStop, onOpenWeb, onExport, onClearExited,
}: Props) {
  const logRef = useRef<HTMLDivElement>(null);
  const [, tick] = useState(0);

  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, []);

  const active = procs.find((p) => p.id === activeId) ?? null;
  const running = procs.filter((p) => !p.exited).length;

  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [active?.lines.length, active?.id, open]);

  return (
    <Sheet open={open} onOpenChange={(o) => !o && onClose()}>
      <SheetContent
        side="right"
        showCloseButton={false}
        aria-describedby={undefined}
        className="w-[420px] gap-0 border-l border-border bg-card p-0 shadow-2xl sm:max-w-[88vw]"
      >
        {/* 面板头 */}
        <div className="flex items-center gap-2.5 border-b border-border px-4 py-3">
          <Terminal className="h-3.5 w-3.5 text-muted-foreground" />
          <SheetTitle className="text-xs font-bold">实例终端</SheetTitle>
          <Badge variant={running > 0 ? "success" : "secondary"}>{running} 运行中</Badge>
          <span className="flex-1" />
          <Button size="sm" variant="ghost" onClick={onClearExited} title="移除已退出的实例">
            <Eraser />
          </Button>
          <SheetClose asChild>
            <Button size="sm" variant="ghost" title="收起抽屉">
              <X />
            </Button>
          </SheetClose>
        </div>

        {/* 实例切换 */}
        {procs.length > 0 && (
          <div className="border-b border-border px-4 py-2.5">
            <ToggleGroup
              type="single"
              spacing={6}
              className="flex-wrap"
              value={activeId != null ? String(activeId) : ""}
              onValueChange={(v) => v && onSelect(Number(v))}
            >
              {procs.map((p) => (
                <ToggleGroupItem
                  key={p.id}
                  value={String(p.id)}
                  title={`PID ${p.id}`}
                  variant="outline"
                  size="sm"
                  className={`h-auto gap-1.5 rounded-full px-2.5 py-1 font-mono text-[11px] ${
                    p.exited ? "opacity-60" : ""
                  } data-[state=on]:border-primary/50 data-[state=on]:bg-primary/10 data-[state=on]:text-foreground`}
                >
                  <span
                    className={`h-1.5 w-1.5 rounded-full ${
                      p.exited ? (p.code ? "bg-red-500" : "bg-muted-foreground/50") : "bg-emerald-500"
                    }`}
                  />
                  {p.profile || p.version}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          </div>
        )}

        {/* 日志流 */}
        {active ? (
          <>
            <div
              ref={logRef}
              className="mx-4 mt-3 min-h-0 flex-1 overflow-y-auto rounded-md border border-border bg-background p-3 font-mono text-[11px] leading-relaxed text-muted-foreground select-text"
            >
              {active.lines.length === 0
                ? "（暂无输出，等待 dsh 日志…）"
                : active.lines.join("\n")}
            </div>
            <div className="flex items-center gap-2 border-t border-border px-4 py-3 text-[11px] text-muted-foreground">
              <span className="font-mono">
                PID {active.id} ·{" "}
                {active.exited
                  ? active.code == null
                    ? "已停止"
                    : `退出码 ${active.code}`
                  : `运行中 ${fmtUptime(active.startedAt)}`}
              </span>
              <span className="flex-1" />
              {active.lines.length > 0 && (
                <Button size="sm" variant="ghost" onClick={onExport} title="保存到 ~/.dsh-launcher/logs/">
                  <Download /> 导出
                </Button>
              )}
              {active.webUrl && !active.exited && (
                <Button size="sm" onClick={() => onOpenWeb(active.webUrl!)} title={active.webUrl}>
                  <ExternalLink /> 打开
                </Button>
              )}
              {!active.exited && (
                <Button size="sm" variant="destructive" onClick={() => onStop(active.id)}>
                  <Square /> 停止
                </Button>
              )}
            </div>
          </>
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-2 px-8 text-center text-muted-foreground">
            <Terminal className="h-5 w-5 opacity-40" />
            <div className="text-xs">
              启动一个 Profile 实例后，实时日志会显示在这里
            </div>
          </div>
        )}
      </SheetContent>
    </Sheet>
  );
}
