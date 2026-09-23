import { useEffect, useRef } from "react";
import { Loader2, Square, XCircle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import type { SystemTask } from "../../types";

interface Props {
  task: SystemTask;
  /** 取消 dsh 版本安装（Node 安装无取消通道，传了也不会显示按钮） */
  onCancelInstall?: () => void;
}

/** 进度小字：有字节数就显示 % 与 MB，否则显示状态词 */
function progressText(t: SystemTask): string {
  if (t.total > 0) {
    return `${Math.round((t.received / t.total) * 100)}% (${(t.received / 1048576).toFixed(1)}/${(t.total / 1048576).toFixed(1)} MB)`;
  }
  return t.running ? "进行中…" : t.ok ? "完成" : "失败";
}

/**
 * 终端面板的「系统任务」视图：dsh 版本安装 / 内置 Node 安装的进度与日志。
 * 数据由 App 层 useTerminalJobs 从 install-log / runtime-log 等事件流聚合。
 */
export default function SystemTaskView({ task, onCancelInstall }: Props) {
  const logRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
  }, [task.lines.length]);

  return (
    <>
      {task.running && task.total > 0 && (
        <div className="mx-4 mt-3 shrink-0 space-y-1">
          <Progress value={(task.received / task.total) * 100} className="h-1.5" />
          <div className="text-right font-mono text-[10.5px] text-muted-foreground">
            {progressText(task)}
          </div>
        </div>
      )}
      <div
        ref={logRef}
        className="mx-4 mt-3 min-h-0 flex-1 overflow-y-auto rounded-md border border-border bg-background p-3 font-mono text-[11px] leading-relaxed text-muted-foreground select-text whitespace-pre-wrap break-words"
      >
        {task.lines.length === 0
          ? task.running
            ? "等待输出…"
            : "（无输出）"
          : task.lines.join("\n")}
      </div>
      <div className="flex flex-wrap items-center gap-2 border-t border-border px-4 py-2.5 text-[11px]">
        {task.running ? (
          <Badge variant="info">
            <Loader2 className="animate-spin" /> 运行中
          </Badge>
        ) : task.ok ? (
          <Badge variant="success">完成</Badge>
        ) : (
          <Badge variant="destructive">失败</Badge>
        )}
        {!task.running && !task.ok && (
          <span className="inline-flex min-w-0 items-center gap-1 text-destructive">
            <XCircle className="h-3 w-3 shrink-0" />
            <span className="truncate" title={task.message ?? ""}>{task.message ?? "安装失败"}</span>
          </span>
        )}
        {task.running && task.total > 0 && (
          <span className="font-mono text-[10.5px] text-muted-foreground">{progressText(task)}</span>
        )}
        <span className="flex-1" />
        {task.running && task.kind === "dshInstall" && onCancelInstall && (
          <Button size="sm" variant="destructive" onClick={onCancelInstall}>
            <Square /> 取消
          </Button>
        )}
      </div>
    </>
  );
}
