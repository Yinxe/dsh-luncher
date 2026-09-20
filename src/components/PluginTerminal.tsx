import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronDown, Copy, Download, Eraser, Loader2, Square, Terminal as TerminalIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { api } from "../api";
import type { PluginJob } from "../types";

interface Props {
  jobs: PluginJob[];
  activeId: number | null;
  onSelect: (id: number) => void;
  onCancel: (id: number) => void;
  onClear: () => void;
  /** 当前选中的 profile（用于标注「本实例」任务） */
  profile: string;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

function fmtDuration(job: PluginJob): string {
  const end = job.finishedAt ?? Date.now();
  const s = Math.max(0, Math.round((end - job.startedAt) / 1000));
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m${String(s % 60).padStart(2, "0")}s`;
}

function statusOf(job: PluginJob): { variant: "info" | "success" | "destructive" | "warning" | "outline"; text: string } {
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

/**
 * 内置终端：插件安装 / 卸载 / 升级 / clone 的实时输出面板。
 *
 * 数据来自后端 plugin-log（逐行流式）与 plugin-job（状态）事件，
 * 挂载时用 list_plugin_jobs 快照恢复历史；每个任务一个标签页。
 */
export default function PluginTerminal({
  jobs, activeId, onSelect, onCancel, onClear, profile, onToast,
}: Props) {
  const [open, setOpen] = useState(true);
  const [follow, setFollow] = useState(true);
  const bodyRef = useRef<HTMLDivElement>(null);

  const active = useMemo(
    () => jobs.find((j) => j.id === activeId) ?? jobs[0] ?? null,
    [jobs, activeId],
  );
  const running = jobs.filter((j) => j.running).length;
  const lineCount = active?.lines.length ?? 0;

  // 自动滚到底（除非用户手动上滑）
  useEffect(() => {
    if (!follow || !open) return;
    const el = bodyRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lineCount, active?.id, follow, open]);

  const onScroll = useCallback(() => {
    const el = bodyRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    setFollow(atBottom);
  }, []);

  const copyLog = useCallback(async () => {
    if (!active) return;
    const text = active.lines.map((l) => l.text).join("\n");
    try {
      await navigator.clipboard.writeText(text);
      onToast("ok", `已复制 ${active.lines.length} 行输出`);
    } catch {
      onToast("err", "复制失败：剪贴板不可用");
    }
  }, [active, onToast]);

  const exportLog = useCallback(async () => {
    if (!active) return;
    try {
      const path = await api.exportPluginJobLog(active.id);
      onToast("ok", `日志已导出到 ${path}`);
    } catch (e) {
      onToast("err", String(e));
    }
  }, [active, onToast]);

  const status = active ? statusOf(active) : null;

  return (
    // py-0：Card 默认带纵向 --card-spacing 内边距，终端要贴边
    <Card className="gap-0 overflow-hidden py-0">
      <Collapsible open={open} onOpenChange={setOpen}>
        {/* 头部：标题 + 运行状态 + 折叠 */}
        <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2.5">
          <TerminalIcon className="h-3.5 w-3.5 text-muted-foreground" />
          <div className="text-[13px] font-semibold">内置终端</div>
          <span className="text-[10.5px] text-muted-foreground">
            安装 / 卸载 / 升级输出（官方 dsh plugin 命令）
          </span>
          {running > 0 && (
            <Badge variant="info">
              <Loader2 className="animate-spin" /> {running} 个任务运行中
            </Badge>
          )}
          <span className="flex-1" />
          <Button size="sm" variant="ghost" disabled={!active || lineCount === 0} onClick={copyLog} title="复制当前任务输出">
            <Copy /> 复制
          </Button>
          <Button size="sm" variant="ghost" disabled={!active} onClick={exportLog} title="导出到 ~/.dsh-launcher/logs/">
            <Download /> 导出
          </Button>
          <Button
            size="sm"
            variant="ghost"
            disabled={jobs.every((j) => j.running)}
            onClick={onClear}
            title="清理已结束的任务"
          >
            <Eraser /> 清空
          </Button>
          <CollapsibleTrigger asChild>
            <Button size="sm" variant="ghost" title={open ? "收起终端" : "展开终端"}>
              <ChevronDown className={`transition-transform ${open ? "" : "-rotate-90"}`} />
            </Button>
          </CollapsibleTrigger>
        </div>

        <CollapsibleContent>
          {jobs.length === 0 ? (
            <div className="flex flex-col items-center justify-center gap-1.5 px-6 py-8 text-center text-[11.5px] text-muted-foreground">
              <TerminalIcon className="h-4 w-4 opacity-40" />
              安装、升级或卸载插件后，命令输出会实时显示在这里
            </div>
          ) : (
            <>
              {/* 任务标签 */}
              <div className="border-b border-border px-4 py-2">
                <ToggleGroup
                  type="single"
                  spacing={6}
                  className="w-full flex-wrap justify-start"
                  value={active ? String(active.id) : ""}
                  onValueChange={(v) => v && onSelect(Number(v))}
                >
                  {jobs.map((j) => (
                    <ToggleGroupItem
                      key={j.id}
                      value={String(j.id)}
                      variant="outline"
                      size="sm"
                      title={`${j.label}${j.command ? ` · ${j.command}` : ""}`}
                      className={`h-auto! max-w-[260px] gap-1.5 rounded-full! px-2.5 py-1 text-[11px] ${
                        j.running ? "" : "opacity-80"
                      } data-[state=on]:border-primary/50 data-[state=on]:bg-primary/10 data-[state=on]:text-foreground`}
                    >
                      {j.running ? (
                        <Loader2 className="animate-spin text-sky-500" />
                      ) : (
                        <span
                          className={`h-1.5 w-1.5 rounded-full ${
                            j.cancelled
                              ? "bg-amber-500"
                              : j.ok
                              ? "bg-emerald-500"
                              : "bg-red-500"
                          }`}
                        />
                      )}
                      <span className="truncate">{j.label}</span>
                      {j.profile && j.profile !== profile && (
                        <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                          {j.profile}
                        </span>
                      )}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
              </div>

              {/* 输出区 */}
              {active && (
                <>
                  <div className="relative">
                    <div
                      ref={bodyRef}
                      onScroll={onScroll}
                      className="h-[260px] overflow-y-auto bg-background px-4 py-3 font-mono text-[11px] leading-relaxed select-text"
                    >
                      {active.dropped > 0 && (
                        <div className="mb-1 text-muted-foreground">
                          … 已省略前 {active.dropped} 行（超出缓冲上限）
                        </div>
                      )}
                      {active.lines.length === 0 ? (
                        <div className="text-muted-foreground">
                          {active.running ? "等待输出…" : "（无输出）"}
                        </div>
                      ) : (
                        active.lines.map((l, i) => (
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

                  {/* 脚注：命令 + 状态 + 取消 */}
                  <div className="flex flex-wrap items-center gap-2 border-t border-border px-4 py-2 text-[11px]">
                    {status && <Badge variant={status.variant}>{status.text}</Badge>}
                    <span className="text-muted-foreground">{fmtDuration(active)}</span>
                    <span className="min-w-0 flex-1 truncate font-mono text-[10.5px] text-muted-foreground" title={active.command}>
                      {active.command || active.label}
                    </span>
                    {active.running && (
                      <Button size="sm" variant="destructive" onClick={() => onCancel(active.id)}>
                        <Square /> 取消
                      </Button>
                    )}
                  </div>
                </>
              )}
            </>
          )}
        </CollapsibleContent>
      </Collapsible>
    </Card>
  );
}
