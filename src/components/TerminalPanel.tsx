import { useEffect, useState } from "react";
import { Eraser, Layers, Loader2, Package, Terminal, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Sheet, SheetContent, SheetTitle,
} from "@/components/ui/sheet";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import InstanceTaskView, { labelOf } from "./terminal/InstanceTaskView";
import PluginTaskView from "./terminal/PluginTaskView";
import SystemTaskView from "./terminal/SystemTaskView";
import { cn } from "@/lib/utils";
import type { InstanceLog, PluginJob, ProcEntry, SystemTask, TerminalTaskRef } from "../types";

/** 面板固定宽度（行内右栏与 Sheet 共用；二期换 resizable 后由用户拖） */
export const TERMINAL_PANEL_W = 420;

interface Props {
  open: boolean;
  /** true = 与内容区并排的行内右栏；false = 窄屏 Sheet 覆盖式 */
  inline: boolean;
  onOpenChange: (v: boolean) => void;
  task: TerminalTaskRef | null;
  onSelectTask: (ref: TerminalTaskRef | null) => void;
  procs: ProcEntry[];
  pluginJobs: PluginJob[];
  /** 工作台当前选中的 profile：插件任务列表默认只显示它的（跨 profile 需显式展开） */
  pluginProfile: string;
  onCancelPluginJob: (id: number) => void;
  onRetryPluginJob: (jobId: number) => void;
  onApprovePluginBuilds: (jobId: number) => void;
  sysTasks: SystemTask[];
  onStop: (id: number) => void;
  onOpenWeb: (url: string) => void;
  onExport: () => void;
  onReadLog: (pid: number) => Promise<InstanceLog | null>;
  onReveal: (path: string) => void;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
  /** 取消在途的 dsh 版本安装（面板内系统任务视图的取消按钮） */
  onCancelInstall: () => void;
  /** 清理全部已结束任务（退出的实例 + 终态插件任务 + 终态系统任务） */
  onClearFinished: () => void;
}

const refKey = (r: TerminalTaskRef) => `${r.kind}:${r.id}`;

/**
 * 通用终端面板：实例日志 / 插件任务 / dsh 与 Node 安装任务统一在此展示。
 * 宽屏作为右侧常驻栏（与内容并排、width 过渡滑入），窄屏降级为 Sheet 抽屉；
 * 两种宿主共用同一个 body。
 */
export default function TerminalPanel(props: Props) {
  const { open, inline, onOpenChange, task, onSelectTask, procs, pluginJobs, pluginProfile, sysTasks } = props;
  const running = procs.filter((p) => !p.exited).length;
  const hasFinished =
    procs.some((p) => p.exited) || pluginJobs.some((j) => !j.running) || sysTasks.some((t) => !t.running);

  // 隔离（沿用旧内嵌终端的行为）：插件任务默认只显示当前 profile 的，
  // 跨 profile 混排是「以为在给 A 装、其实是在 B 上操作」的来源；要看得显式展开。
  const [showAllPlugins, setShowAllPlugins] = useState(false);
  // 切 profile 时收起「全部」：展开按钮只挂在「存在其他 profile 任务」上，
  // 切到一个没有外部任务的 profile 后按钮消失，视图会卡在跨 profile 模式回不来
  useEffect(() => setShowAllPlugins(false), [pluginProfile]);
  const foreignPluginCount = pluginJobs.filter((j) => j.profile !== pluginProfile).length;
  const visiblePluginJobs = showAllPlugins
    ? pluginJobs
    : pluginJobs.filter((j) => j.profile === pluginProfile);

  const sectionLabel = (t: string) => (
    <div className="px-2 pb-0.5 pt-2 text-[10px] font-semibold text-muted-foreground">{t}</div>
  );

  const taskRows = (
    <ToggleGroup
      type="single"
      orientation="vertical"
      className="flex-col items-stretch gap-1"
      value={task ? refKey(task) : ""}
      onValueChange={(v) => {
        if (!v) return;
        const [kind, ...rest] = v.split(":");
        onSelectTask({ kind: kind as TerminalTaskRef["kind"], id: rest.join(":") });
      }}
    >
      {procs.length > 0 && sectionLabel("实例")}
      {procs.map((p) => (
        <ToggleGroupItem
          key={`i:${p.id}`}
          value={`instance:${p.id}`}
          variant="outline"
          size="sm"
          title={`PID ${p.id}${p.port != null ? ` · 端口 ${p.port}` : ""}${p.external ? " · 非内嵌实例" : ""}`}
          className={cn(
            "h-auto w-full justify-start gap-2 rounded-md px-2.5 py-1.5 font-mono text-[11px]",
            "data-[state=on]:border-primary/50 data-[state=on]:bg-primary/10 data-[state=on]:text-foreground",
            p.exited && "opacity-60"
          )}
        >
          <span
            className={cn(
              "h-1.5 w-1.5 shrink-0 rounded-full",
              p.exited ? (p.code ? "bg-red-500" : "bg-muted-foreground/50") : "bg-emerald-500"
            )}
          />
          <span className="min-w-0 flex-1 truncate text-left">{labelOf(p)}</span>
          <span className="shrink-0 text-[10px] text-muted-foreground">
            {p.exited ? (p.code == null ? "已停止" : `码 ${p.code}`) : "运行中"}
          </span>
        </ToggleGroupItem>
      ))}
      {(visiblePluginJobs.length > 0 || foreignPluginCount > 0) && sectionLabel("插件任务")}
      {foreignPluginCount > 0 && (
        <Button
          size="xs"
          variant={showAllPlugins ? "secondary" : "outline"}
          className="mb-1 self-start"
          title={
            showAllPlugins
              ? "只看当前 profile 的插件任务"
              : `其他 profile 还有 ${foreignPluginCount} 个插件任务`
          }
          onClick={() => setShowAllPlugins((v) => !v)}
        >
          <Layers /> {showAllPlugins ? `只看 ${pluginProfile || "当前"}` : `+${foreignPluginCount} 其他 profile`}
        </Button>
      )}
      {visiblePluginJobs.map((j) => (
        <ToggleGroupItem
          key={`p:${j.id}`}
          value={`plugin:${j.id}`}
          variant="outline"
          size="sm"
          title={`${j.label}${j.command ? ` · ${j.command}` : ""}`}
          className={cn(
            "h-auto w-full justify-start gap-2 rounded-md px-2.5 py-1.5 font-mono text-[11px]",
            "data-[state=on]:border-primary/50 data-[state=on]:bg-primary/10 data-[state=on]:text-foreground",
            !j.running && "opacity-60"
          )}
        >
          {j.running ? (
            <Loader2 className="h-3 w-3 shrink-0 animate-spin text-teal-500" />
          ) : (
            <span
              className={cn(
                "h-1.5 w-1.5 shrink-0 rounded-full",
                j.cancelled ? "bg-amber-500" : j.ok ? "bg-emerald-500" : "bg-red-500"
              )}
            />
          )}
          <span className="min-w-0 flex-1 truncate text-left">{j.label}</span>
          {j.profile && j.profile !== pluginProfile && (
            <span className="shrink-0 text-[10px] text-muted-foreground">{j.profile}</span>
          )}
          <span className="shrink-0 text-[10px] text-muted-foreground">
            {j.running ? "进行中" : j.cancelled ? "已取消" : j.ok ? "成功" : "失败"}
          </span>
        </ToggleGroupItem>
      ))}
      {sysTasks.length > 0 && sectionLabel("系统任务")}
      {sysTasks.map((t) => (
        <ToggleGroupItem
          key={`s:${t.id}`}
          value={`${t.kind}:${t.id}`}
          variant="outline"
          size="sm"
          title={t.message ?? t.label}
          className={cn(
            "h-auto w-full justify-start gap-2 rounded-md px-2.5 py-1.5 font-mono text-[11px]",
            "data-[state=on]:border-primary/50 data-[state=on]:bg-primary/10 data-[state=on]:text-foreground",
            !t.running && "opacity-60"
          )}
        >
          {t.running ? (
            <Loader2 className="h-3 w-3 shrink-0 animate-spin text-muted-foreground" />
          ) : t.ok ? (
            <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-emerald-500" />
          ) : (
            <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-red-500" />
          )}
          <span className="min-w-0 flex-1 truncate text-left">{t.label}</span>
          <span className="shrink-0 text-[10px] text-muted-foreground">
            {t.running ? (t.total > 0 ? `${Math.round((t.received / t.total) * 100)}%` : "进行中") : t.ok ? "完成" : "失败"}
          </span>
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );

  const activeProc = task?.kind === "instance" ? procs.find((p) => String(p.id) === task.id) ?? null : null;
  // 视图路由用全量 jobs：自动聚焦的跨 profile 任务不在折叠列表里也要能显示
  const activePluginJob =
    task?.kind === "plugin" ? pluginJobs.find((j) => String(j.id) === task.id) ?? null : null;
  const activeSys =
    task && (task.kind === "dshInstall" || task.kind === "nodeInstall")
      ? sysTasks.find((t) => t.kind === task.kind && t.id === task.id) ?? null
      : null;
  const emptyList = procs.length + visiblePluginJobs.length + sysTasks.length === 0;

  const body = (
    /* 与左侧栏/标题栏同一套玻璃材质：半透明卡面 + 背后主背景模糊透出 */
    <div className="flex h-full min-h-0 flex-col bg-card/75 backdrop-blur-[10px] backdrop-saturate-[1.3]">
      {/* 面板头 */}
      <div className="flex shrink-0 items-center gap-2.5 border-b border-border px-4 py-3">
        <Terminal className="h-3.5 w-3.5 text-muted-foreground" />
        {/* SheetTitle 底层是 DialogTitle：只在 Sheet 宿主里合法，行内右栏用普通标题 */}
        {inline ? (
          <div className="text-xs font-bold">终端</div>
        ) : (
          <SheetTitle className="text-xs font-bold">终端</SheetTitle>
        )}
        <Badge variant={running > 0 ? "success" : "secondary"}>{running} 运行中</Badge>
        <span className="flex-1" />
        <Button size="sm" variant="ghost" onClick={props.onClearFinished} disabled={!hasFinished} title="移除已退出的实例与已结束的安装任务">
          <Eraser />
        </Button>
        <Button size="sm" variant="ghost" title="收起终端面板" onClick={() => onOpenChange(false)}>
          <X />
        </Button>
      </div>

      {/* 任务列表 */}
      <div className="max-h-[35%] shrink-0 overflow-y-auto border-b border-border px-3 py-2">
        {emptyList ? (
          <div className="px-1 py-1.5 text-[11px] text-muted-foreground">
            暂无终端任务：启动实例、安装插件 / dsh / Node 时会出现在这里
          </div>
        ) : taskRows}
      </div>

      {/* 当前任务视图 */}
      {activeProc && (
        <InstanceTaskView
          proc={activeProc}
          open={open}
          onStop={props.onStop}
          onOpenWeb={props.onOpenWeb}
          onExport={props.onExport}
          onReadLog={props.onReadLog}
          onReveal={props.onReveal}
        />
      )}
      {activePluginJob && (
        <PluginTaskView
          key={activePluginJob.id}
          job={activePluginJob}
          profile={pluginProfile}
          onCancel={props.onCancelPluginJob}
          onRetry={props.onRetryPluginJob}
          onApproveBuilds={props.onApprovePluginBuilds}
          onToast={props.onToast}
        />
      )}
      {activeSys && <SystemTaskView task={activeSys} onCancelInstall={props.onCancelInstall} />}
      {!task && !emptyList && (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 px-8 text-center text-muted-foreground">
          <Package className="h-5 w-5 opacity-40" />
          <div className="text-xs">从上方选择一个终端任务查看输出</div>
        </div>
      )}
      {!task && emptyList && (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 px-8 text-center text-muted-foreground">
          <Terminal className="h-5 w-5 opacity-40" />
          <div className="text-xs">启动一个 Profile 实例后，实时日志会显示在这里</div>
        </div>
      )}
    </div>
  );

  if (inline) {
    return (
      <div
        aria-hidden={!open}
        className={cn(
          "shrink-0 overflow-hidden transition-[width] duration-300 ease-out motion-reduce:transition-none",
          open ? "w-[var(--tw-terminal-w)] border-l border-border" : "w-0 border-l border-transparent"
        )}
        style={{ "--tw-terminal-w": `${TERMINAL_PANEL_W}px` } as React.CSSProperties}
      >
        <div className="h-full" style={{ width: TERMINAL_PANEL_W }}>
          {body}
        </div>
      </div>
    );
  }
  return (
    <Sheet open={open} onOpenChange={(o) => onOpenChange(o)}>
      <SheetContent
        side="right"
        showCloseButton={false}
        aria-describedby={undefined}
        className="w-full max-w-[420px] gap-0 border-l border-border bg-card p-0 shadow-2xl"
      >
        {body}
      </SheetContent>
    </Sheet>
  );
}
