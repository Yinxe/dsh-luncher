import { useEffect, useRef, useState } from "react";
import { Download, Eraser, ExternalLink, Square, Terminal, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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

/** 实例终端抽屉：从右侧滑入，实时日志 + 启停 + Web UI */
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
    <aside
      className={`fixed inset-y-0 right-0 z-40 flex w-[420px] max-w-[88vw] flex-col border-l border-border bg-card shadow-2xl transition-transform duration-200 ease-out ${
        open ? "translate-x-0" : "translate-x-full"
      }`}
    >
      {/* 面板头 */}
      <div className="flex items-center gap-2.5 border-b border-border px-4 py-3">
        <Terminal className="h-3.5 w-3.5 text-muted-foreground" />
        <span className="text-xs font-bold">实例终端</span>
        <Badge variant={running > 0 ? "success" : "secondary"}>{running} 运行中</Badge>
        <span className="flex-1" />
        <Button size="sm" variant="ghost" onClick={onClearExited} title="移除已退出的实例">
          <Eraser />
        </Button>
        <Button size="sm" variant="ghost" onClick={onClose} title="收起抽屉">
          <X />
        </Button>
      </div>

      {/* 实例切换 */}
      {procs.length > 0 && (
        <div className="flex flex-wrap gap-1.5 border-b border-border px-4 py-2.5">
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
              {p.profile || p.version}
            </button>
          ))}
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
    </aside>
  );
}
