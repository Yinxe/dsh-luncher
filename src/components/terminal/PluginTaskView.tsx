import { useCallback, useEffect, useRef, useState } from "react";
import { Copy, Download, RotateCcw, ShieldCheck, Square } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { api } from "../../api";
import type { PluginJob } from "../../types";

function fmtDuration(job: PluginJob): string {
  const end = job.finishedAt ?? Date.now();
  const s = Math.max(0, Math.round((end - job.startedAt) / 1000));
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m${String(s % 60).padStart(2, "0")}s`;
}

function statusOf(job: PluginJob): { variant: "info" | "success" | "destructive" | "warning"; text: string } {
  if (job.running) return { variant: "info", text: "运行中" };
  if (job.cancelled) return { variant: "warning", text: "已取消" };
  if (job.ok) return { variant: "success", text: "成功" };
  return { variant: "destructive", text: job.exitCode != null ? `失败 ${job.exitCode}` : "失败" };
}

/** 日志行着色：stdout 正常前景色、stderr 红色、info 次级灰 */
function lineClass(stream: string): string {
  if (stream === "stderr") return "text-destructive";
  if (stream === "info") return "text-muted-foreground";
  return "text-foreground/90";
}

interface Props {
  job: PluginJob;
  /** 当前选中的 profile：跨 profile 的任务在页脚标注归属 */
  profile: string;
  onCancel: (id: number) => void;
  /** 重试失败的任务：原样重放它的全部步骤 */
  onRetry: (jobId: number) => void;
  /** 放行构建脚本并重试 */
  onApproveBuilds: (jobId: number) => void;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

/**
 * 终端面板的「插件任务」视图：安装 / 卸载 / 升级 / clone 的实时输出。
 * 数据来自后端 plugin-log（逐行流式）与 plugin-job（状态）事件，
 * 由 App 层的 usePluginJobs 统一维护，这里只渲染选中的那一个任务
 * （父组件按 job.id 重挂载，滚动跟随状态不跨任务残留）。
 */
export default function PluginTaskView({ job, profile, onCancel, onRetry, onApproveBuilds, onToast }: Props) {
  const [follow, setFollow] = useState(true);
  const bodyRef = useRef<HTMLDivElement>(null);

  // 自动滚到底（除非用户手动上滑）
  useEffect(() => {
    if (!follow) return;
    const el = bodyRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [job.lines.length, follow]);

  const onScroll = useCallback(() => {
    const el = bodyRef.current;
    if (!el) return;
    setFollow(el.scrollHeight - el.scrollTop - el.clientHeight < 24);
  }, []);

  const copyLog = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(job.lines.map((l) => l.text).join("\n"));
      onToast("ok", `已复制 ${job.lines.length} 行输出`);
    } catch {
      onToast("err", "复制失败：剪贴板不可用");
    }
  }, [job, onToast]);

  const exportLog = useCallback(async () => {
    try {
      const path = await api.exportPluginJobLog(job.id);
      onToast("ok", `日志已导出到 ${path}`);
    } catch (e) {
      onToast("err", String(e));
    }
  }, [job.id, onToast]);

  const status = statusOf(job);

  return (
    <>
      <div className="relative mx-4 mt-3 flex min-h-0 flex-1 flex-col">
        <div
          ref={bodyRef}
          onScroll={onScroll}
          className="min-h-0 flex-1 overflow-y-auto rounded-md border border-border bg-background p-3 font-mono text-[11px] leading-relaxed select-text"
        >
          {job.dropped > 0 && (
            <div className="mb-1 text-muted-foreground">… 已省略前 {job.dropped} 行（超出缓冲上限）</div>
          )}
          {job.lines.length === 0 ? (
            <div className="text-muted-foreground">{job.running ? "等待输出…" : "（无输出）"}</div>
          ) : (
            job.lines.map((l, i) => (
              <div key={i} className={`whitespace-pre-wrap break-all ${lineClass(l.stream)}`}>
                {l.text}
              </div>
            ))
          )}
        </div>
        {!follow && (
          <Button
            size="xs"
            variant="secondary"
            className="absolute right-3 bottom-3 shadow-md"
            onClick={() => {
              setFollow(true);
              const el = bodyRef.current;
              if (el) el.scrollTop = el.scrollHeight;
            }}
          >
            ↓ 跳到最新
          </Button>
        )}
      </div>

      {/* 页脚：状态 + 时长 + 命令 + 复制/导出/重试/允许构建/取消 */}
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5 border-t border-border px-4 py-2.5 text-[11px]">
        <Badge variant={status.variant}>{status.text}</Badge>
        <span className="text-muted-foreground">{fmtDuration(job)}</span>
        {job.profile && job.profile !== profile && (
          <Badge variant="outline" className="text-[10px]" title="任务运行的 profile 与当前选中不同">
            {job.profile}
          </Badge>
        )}
        <span
          className="min-w-0 flex-1 basis-full truncate font-mono text-[10.5px] text-muted-foreground"
          title={job.command}
        >
          {job.command || job.label}
        </span>
        <Button size="sm" variant="ghost" disabled={job.lines.length === 0} onClick={copyLog} title="复制该任务全部输出">
          <Copy /> 复制
        </Button>
        <Button size="sm" variant="ghost" onClick={exportLog} title="导出到 ~/.dsh-starter/logs/">
          <Download /> 导出
        </Button>
        {!job.running && !job.ok && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => onRetry(job.id)}
            title="原样重放这个任务的全部步骤（新任务，输出同样在这里）"
          >
            <RotateCcw /> 重试
          </Button>
        )}
        {!job.running && job.pendingBuilds.length > 0 && (
          <Button
            size="sm"
            onClick={() => onApproveBuilds(job.id)}
            title={`写入 allowBuilds 并重跑：${job.pendingBuilds.join("、")}（构建脚本会执行第三方代码，确认可信再放行）`}
          >
            <ShieldCheck /> 允许构建脚本并重试
          </Button>
        )}
        {job.running && (
          <Button size="sm" variant="destructive" onClick={() => onCancel(job.id)}>
            <Square /> 取消
          </Button>
        )}
      </div>
    </>
  );
}
