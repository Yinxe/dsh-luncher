import { useEffect, useRef, useState } from "react";
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
  const mm = String(m).padStart(2, "0");
  const ss = String(sec).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

export default function ProcessDock({
  procs,
  activeId,
  onSelect,
  onStop,
  onOpenWeb,
  onExport,
  onClearExited,
  onToggle,
  open,
}: Props) {
  const logRef = useRef<HTMLDivElement>(null);
  const [, tick] = useState(0);

  // 每秒刷新一次运行时长
  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, []);

  const active = procs.find((p) => p.id === activeId) ?? null;

  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [active?.lines.length, active?.id]);

  if (procs.length === 0) return null;

  return (
    <div className="dock">
      <div className="dock-head">
        <span className="dock-title">
          dsh 进程
          <span className="dock-count">{procs.filter((p) => !p.exited).length} 运行中</span>
        </span>
        <span style={{ flex: 1 }} />
        <button className="sm ghost" onClick={onClearExited} title="从列表移除已退出的进程">
          清除已退出
        </button>
        <button className="sm ghost" onClick={onToggle}>
          {open ? "收起 ▾" : "展开 ▸"}
        </button>
      </div>

      <div className="dock-tabs">
        {procs.map((p) => (
          <button
            key={p.id}
            className={`dock-tab${p.id === activeId ? " active" : ""}${p.exited ? " exited" : ""}`}
            onClick={() => onSelect(p.id)}
            title={`PID ${p.id}`}
          >
            <span className={`dot ${p.exited ? (p.code ? "err" : "off") : "ok"}`} />
            {p.version}
            {p.profile ? ` · ${p.profile}` : ""}
          </button>
        ))}
      </div>

      {open && active && (
        <div className="dock-body">
          <div className="dock-log" ref={logRef}>
            {active.lines.length === 0
              ? "（暂无输出，等待 dsh 日志…）"
              : active.lines.join("\n")}
          </div>
          <div className="dock-foot">
            <span className="mono">
              PID {active.id} · {active.exited
                ? active.code == null
                  ? "已停止"
                  : `已退出，退出码 ${active.code}`
                : `运行中 ${fmtUptime(active.startedAt)}`}
            </span>
            <span style={{ flex: 1 }} />
            {active.lines.length > 0 && (
              <button className="sm ghost" onClick={onExport} title="保存到 ~/.dsh-launcher/logs/">
                导出日志
              </button>
            )}
            {active.webUrl && !active.exited && (
              <button
                className="sm primary"
                onClick={() => onOpenWeb(active.webUrl!)}
                title={active.webUrl}
              >
                打开 Web UI ↗
              </button>
            )}
            {!active.exited && (
              <button className="sm danger" onClick={() => onStop(active.id)}>
                停止
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
