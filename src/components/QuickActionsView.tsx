import {
  CheckCircle2, ExternalLink, Loader2, Play, Rocket, RotateCw, Square, XCircle,
} from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import type { ProfileTarget, View } from "../types";

/** App 的 instanceRows 行结构（只取快捷页需要的字段） */
export interface QuickInstanceRow {
  key: string;
  profile: string;
  phase: "stopped" | "starting" | "ready" | "failed" | "external";
  pid: number | null;
  source: string | null;
  version: string | null;
  webUrl: string | null;
  code: number | null;
  target: ProfileTarget;
  port: number | null;
  logFile: string | null;
  reserved: boolean;
}

interface Props {
  /** 内置 web profile 的实例行；null = dsh 还没初始化（没有任何 profile） */
  row: QuickInstanceRow | null;
  activeVersion: string;
  hasNode: boolean;
  hasInstalled: boolean;
  starting: boolean;
  restarting: boolean;
  initBusy: boolean;
  onStart: (profile: string) => void;
  onStop: (row: { profile: string; pid: number | null }) => void;
  onRestart: (profile: string) => void;
  onOpenWeb: (url: string) => void;
  onNavigate: (v: View) => void;
  onInitDsh: () => void;
}

const PHASE_TEXT: Record<QuickInstanceRow["phase"], string> = {
  stopped: "未运行",
  starting: "启动中…",
  ready: "运行中",
  failed: "启动失败",
  external: "运行中",
};

/**
 * 「首页」：只做一件事 —— 内置 web profile 的一键起停。
 * 状态、地址、动作全部收在一张卡里，其余功能各页自管。
 */
export default function QuickActionsView(props: Props) {
  const {
    row, activeVersion, hasNode, hasInstalled, starting, restarting, initBusy,
    onStart, onStop, onRestart, onOpenWeb, onNavigate, onInitDsh,
  } = props;

  // dsh 还没初始化：先给引导，否则这里没有任何可操作的对象
  if (!row) {
    return (
      <div className="mx-auto w-full max-w-2xl space-y-4">
        <h2 className="text-base font-semibold">首页</h2>
        <Card className="p-5">
          <div className="flex items-start gap-3.5">
            <span className="grid h-10 w-10 shrink-0 place-items-center rounded-lg bg-primary/10 text-primary">
              <Rocket className="h-5 w-5" />
            </span>
            <div className="min-w-0 flex-1 space-y-2">
              <h3 className="text-sm font-semibold">先初始化 dsh</h3>
              <p className="text-[12px] leading-relaxed text-muted-foreground">
                快捷起停建立在 dsh 初始化之后：第一次运行 <span className="font-mono">dsh web</span>
                会生成内置 <span className="font-mono">web</span> profile，之后回到本页即可一键启动。
                {!hasInstalled && "（还没安装 dsh 版本）"}
                {hasInstalled && !hasNode && "（未检测到 Node，dsh 无法运行）"}
              </p>
              <div className="flex flex-wrap items-center gap-2 pt-0.5">
                {!hasInstalled || !hasNode ? (
                  <Button size="sm" onClick={() => onNavigate("versions")}>
                    去准备环境
                  </Button>
                ) : (
                  <Button size="sm" disabled={initBusy} onClick={onInitDsh}>
                    {initBusy ? <Loader2 className="animate-spin" /> : <Rocket />}
                    {initBusy ? "初始化中…" : "初始化 dsh（首次启动 web）"}
                  </Button>
                )}
              </div>
            </div>
          </div>
        </Card>
      </div>
    );
  }

  const running = row.phase === "starting" || row.phase === "ready" || row.phase === "external";
  const busy = starting || restarting;
  const disabled = !hasInstalled || !hasNode;
  const disabledTitle = !hasInstalled
    ? "请先在「版本与安装」页安装 dsh"
    : !hasNode
      ? "未检测到 Node，dsh 无法运行：可在「版本与安装」页安装内置 Node"
      : undefined;

  const statusLine =
    row.phase === "failed"
      ? `启动失败${row.code != null ? `（退出码 ${row.code}）` : ""}`
      : PHASE_TEXT[row.phase] +
        (row.pid != null ? ` · PID ${row.pid}` : "") +
        (row.webUrl ? ` · ${row.webUrl}` : ` · 基于 ${activeVersion || "（未选择版本）"}`);

  return (
    <div className="mx-auto w-full max-w-2xl space-y-4">
      <h2 className="text-base font-semibold">首页</h2>

      <Card className="p-5">
        {/* 窄窗口：按钮排整行换到第二行（右对齐），不再与标题/徽章叠在一起 */}
        <div className="flex flex-wrap items-center gap-x-3.5 gap-y-3">
          {/* 状态灯：一眼看出跑没跑 */}
          <span
            className={`grid h-10 w-10 shrink-0 place-items-center rounded-full ${
              running ? "bg-emerald-500/10 text-emerald-500"
              : row.phase === "failed" ? "bg-red-500/10 text-red-500"
              : "bg-muted text-muted-foreground"
            }`}
          >
            {row.phase === "starting" || starting ? (
              <Loader2 className="h-5 w-5 animate-spin" />
            ) : row.phase === "ready" || row.phase === "external" ? (
              <CheckCircle2 className="h-5 w-5" />
            ) : row.phase === "failed" ? (
              <XCircle className="h-5 w-5" />
            ) : (
              <span className="h-3 w-3 rounded-full bg-muted-foreground/40" />
            )}
          </span>
          <div className="min-w-0 grow basis-[calc(100%-3.5rem)] xl:basis-0">
            <div className="flex items-center gap-1.5">
              <span className="font-mono text-[15px] font-semibold">web</span>
              <Badge variant="info">Web</Badge>
              {row.reserved && <Badge variant="outline">内置</Badge>}
            </div>
            <div className="truncate text-xs text-muted-foreground" title={statusLine}>
              {statusLine}
            </div>
          </div>
          {/* 超窄时只剩图标（文字进 tooltip），换行后整排右对齐 */}
          <div className="ml-auto flex shrink-0 items-center gap-2">
            {running ? (
              <>
                {row.webUrl && (
                  <Button onClick={() => onOpenWeb(row.webUrl!)} title={`在浏览器打开 ${row.webUrl}`}>
                    <ExternalLink /> <span className="hidden sm:inline">打开界面</span>
                  </Button>
                )}
                <Button variant="outline" disabled={restarting} onClick={() => onRestart(row.profile)} title="停止并重新启动">
                  {restarting ? <Loader2 className="animate-spin" /> : <RotateCw />} <span className="hidden sm:inline">重启</span>
                </Button>
                <Button variant="destructive" disabled={restarting} onClick={() => onStop({ profile: row.profile, pid: row.pid })} title="停止 web">
                  <Square /> <span className="hidden sm:inline">停止</span>
                </Button>
              </>
            ) : (
              <Button size="lg" disabled={disabled || busy} title={disabled ? disabledTitle : `基于当前版本（${activeVersion}）启动 web`} onClick={() => onStart(row.profile)}>
                {busy ? <Loader2 className="animate-spin" /> : <Play />} {row.phase === "failed" ? "重试启动" : "启动"}
              </Button>
            )}
          </div>
        </div>
        {row.phase === "failed" && (
          <p className="mt-3 border-t border-border pt-3 text-[11.5px] leading-relaxed text-muted-foreground">
            启动失败：可在「实例终端」查看日志尾部；若是插件导致，到「插件管理」停用可疑插件后重试。
          </p>
        )}
      </Card>
    </div>
  );
}
