import { useEffect, useRef, useState } from "react";
import { Download, Eraser, ExternalLink, Square } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { ProcEntry } from "../types";

interface Props {
  procs: ProcEntry[];
  activeId: number | null;
  onSelect: (id: number) => void;
  onStop: (id: number) => void;
  onOpenWeb: (url: string) => void;
  onExport: () => void;
  onClearExited: () => void;
  onToggle: () => void;
  open: boolean;
}

function fmtUptime(startedAt: number): string {
  const s = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(sec)}` : `${pad(m)}:${pad(sec)}`;
}

export default function ProcessDock({
  procs, activeId, onSelect, onStop, onOpenWeb, onExport, onClearExited, onToggle, open,
}: Props) {
  const logRef = useRef<HTMLDivElement>(null);
  const [, tick] = useState(0);

  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, []);

  const active = procs.find((p) => p.id === activeId) ?? null;

  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [active?.lines.length, active?.id]);

  if (procs.length === 0) return null;

  const running = procs.filter((p) => !p.exited).length;

  return (
    <div className="shrink-0 border-t border-border bg-card/60">
      <div className="flex items-center gap-3 px-4 py-2">
        <span className="flex items-center gap-2 text-xs font-bold">
          dsh 进程
          <Badge variant={running > 0 ? "success" : "secondary"}>{running} 运行中</Badge>
        </span>
        <span className="flex-1" />
        <Button size="sm" variant="ghost" onClick={onClearExited} title="从列表移除已退出的进程">
          <Eraser /> 清除已退出
        </Button>
        <Button size="sm" variant="ghost" onClick={onToggle}>
          {open ? "收起 ▾" : "展开 ▸"}
        </Button>
      </div>

      <div className="flex gap-1.5 overflow-x-auto px-4 pb-2">
        {procs.map((p) => (
          <button
            key={p.id}
            onClick={() => onSelect(p.id)}
            title={`PID ${p.id}`}
            className={`flex shrink-0 items-center gap-1.5 rounded-full border px-2.5 py-1 font-mono text-[11px] transition-colors ${
              p.id === activeId
                ? "border-primary/50 bg-primary/10 text-foreground"
                : "border-border bg-background text-muted-foreground hover:border-primary/30"
            } ${p.exited ? "opacity-60" : ""}`}
          >
            <span
              className={`h-1.5 w-1.5 rounded-full ${
                p.exited ? (p.code ? "bg-red-500" : "bg-muted-foreground/50") : "bg-emerald-500"
              }`}
            />
            {p.version}
            {p.profile ? ` · ${p.profile}` : ""}
          </button>
        ))}
      </div>

      {open && active && (
        <div className="px-4 pb-3">
          <div
            ref={logRef}
            className="h-44 overflow-y-auto rounded-md border border-border bg-background p-2.5 font-mono text-[11px] leading-relaxed text-muted-foreground select-text"
          >
            {active.lines.length === 0 ? "（暂无输出，等待 dsh 日志…）" : active.lines.join("\n")}
          </div>
          <div className="flex items-center gap-3 pt-2 text-[11px] text-muted-foreground">
            <span className="font-mono">
              PID {active.id} ·{" "}
              {active.exited
                ? active.code == null
                  ? "已停止"
                  : `已退出，退出码 ${active.code}`
                : `运行中 ${fmtUptime(active.startedAt)}`}
            </span>
            <span className="flex-1" />
            {active.lines.length > 0 && (
              <Button size="sm" variant="ghost" onClick={onExport} title="保存到 ~/.dsh-launcher/logs/">
                <Download /> 导出日志
              </Button>
            )}
            {active.webUrl && !active.exited && (
              <Button size="sm" onClick={() => onOpenWeb(active.webUrl!)} title={active.webUrl}>
                <ExternalLink /> 打开 Web UI
              </Button>
            )}
            {!active.exited && (
              <Button size="sm" variant="destructive" onClick={() => onStop(active.id)}>
                <Square />
                停止
              </Button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
