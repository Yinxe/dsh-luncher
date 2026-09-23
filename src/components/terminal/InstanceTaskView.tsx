import { useCallback, useEffect, useRef, useState } from "react";
import { Download, ExternalLink, FileText, FolderOpen, Square, Terminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { InstanceLog, ProcEntry } from "../../types";

/** 实例标签：有 profile 名用名字；没有（终端 `dsh web` 不带 --profile）用端口兜底 */
export function labelOf(p: ProcEntry): string {
  return p.profile || (p.port != null ? `:${p.port}` : p.version) || `PID ${p.id}`;
}

function fmtUptime(startedAt: number): string {
  const s = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(sec)}` : `${pad(m)}:${pad(sec)}`;
}

const dirOf = (path: string): string => path.replace(/[\\/][^\\/]*$/, "");

interface Props {
  proc: ProcEntry | null;
  /** 面板是否可见（行内栏打开 / Sheet 打开）：关闭时停掉 tail 轮询与秒级 tick */
  open: boolean;
  onStop: (id: number) => void;
  onOpenWeb: (url: string) => void;
  onExport: () => void;
  /** 拉取独立进程实例的日志尾部（内嵌实例走实时管道，不用这个） */
  onReadLog: (pid: number) => Promise<InstanceLog | null>;
  /** 在文件管理器里定位日志文件 */
  onReveal: (path: string) => void;
}

/**
 * 终端面板的「实例任务」视图：实时日志 + 启停 + Web UI。
 * 内嵌子进程走 stdout/stderr 管道实时推送；独立进程/外部实例没有管道，
 * 独立进程按需 tail 它的日志文件，外部实例只能提示「日志在启动它的终端里」。
 */
export default function InstanceTaskView({ proc, open, onStop, onOpenWeb, onExport, onReadLog, onReveal }: Props) {
  const logRef = useRef<HTMLDivElement>(null);
  const [, tick] = useState(0);
  const [fileLog, setFileLog] = useState<InstanceLog | null>(null);

  useEffect(() => {
    // 只在面板打开时驱动「运行时长」每秒刷新；关闭后无需每秒重渲染整个面板
    if (!open) return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [open]);

  const external = !!proc?.external;
  const externalId = external && proc ? proc.id : null;

  // 独立进程：按需 tail 日志文件（外部实例没有文件日志，直接不轮询）
  const pull = useCallback(async () => {
    if (externalId == null) return;
    try {
      const r = await onReadLog(externalId);
      setFileLog(r);
    } catch { /* 日志尚未生成/已删除，保持上一次内容 */ }
  }, [externalId, onReadLog]);

  useEffect(() => {
    setFileLog(null);
    if (externalId == null || !open) return; // 面板关闭后不再后台 tail 日志文件
    pull();
    const t = setInterval(pull, 1500);
    return () => clearInterval(t);
  }, [externalId, pull, open]);

  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [proc?.lines.length, proc?.id, open, fileLog?.content]);

  if (!proc) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-2 px-8 text-center text-muted-foreground">
        <Terminal className="h-5 w-5 opacity-40" />
        <div className="text-xs">
          启动一个 Profile 实例后，实时日志会显示在这里
        </div>
      </div>
    );
  }

  const body = external ? (fileLog?.content ?? "") : proc.lines.join("\n");
  const emptyText = external
    ? proc.logFile
      ? "（日志为空，等待 dsh 输出…）"
      : "该实例不是启动器拉起的：日志在启动它的终端里，启动器抓不到。可以在这里停止它。"
    : "（暂无输出，等待 dsh 日志…）";

  return (
    <>
      <div
        ref={logRef}
        className="mx-4 mt-3 min-h-0 flex-1 overflow-y-auto rounded-md border border-border bg-background p-3 font-mono text-[11px] leading-relaxed text-muted-foreground select-text"
      >
        {body.trim() === "" ? emptyText : body}
      </div>
      <div className="flex flex-wrap items-center gap-2 border-t border-border px-4 py-3 text-[11px] text-muted-foreground">
        <span className="font-mono">
          PID {proc.id}
          {proc.port != null ? ` · :${proc.port}` : ""}
          {" · "}
          {proc.exited
            ? proc.code == null
              ? "已停止"
              : `退出码 ${proc.code}`
            : proc.startedAt > 0
            ? `运行中 ${fmtUptime(proc.startedAt)}`
            : "运行中"}
          {external && " · 非内嵌"}
        </span>
        <span className="flex-1" />
        {external && proc.logFile && (
          <Button
            size="sm"
            variant="ghost"
            onClick={() => onReveal(dirOf(proc.logFile!))}
            title={proc.logFile}
          >
            <FolderOpen /> 打开日志
          </Button>
        )}
        {!external && proc.lines.length > 0 && (
          <Button size="sm" variant="ghost" onClick={onExport} title="保存到 ~/.dsh-starter/logs/">
            <Download /> 导出
          </Button>
        )}
        {external && proc.logFile && fileLog?.truncated && (
          <span className="inline-flex items-center gap-1" title="日志较长，仅显示末尾部分">
            <FileText className="h-3 w-3" /> 已截断
          </span>
        )}
        {proc.webUrl && !proc.exited && (
          <Button size="sm" onClick={() => onOpenWeb(proc.webUrl!)} title={proc.webUrl}>
            <ExternalLink /> 打开
          </Button>
        )}
        {!proc.exited && (
          <Button size="sm" variant="destructive" onClick={() => onStop(proc.id)}>
            <Square /> 停止
          </Button>
        )}
      </div>
    </>
  );
}
