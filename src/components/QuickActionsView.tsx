import {
  ChevronRight, Cpu, ExternalLink, Gauge, Loader2, Monitor, Package, Play, Rocket,
  RotateCw, SlidersHorizontal, Square,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { WhaleMark } from "@/components/icons/whale";
import type { EnvironmentInfo, View } from "../types";

/** App 的 instanceRows 行结构（只取快捷页需要的字段） */
export interface QuickInstanceRow {
  profile: string;
  phase: "stopped" | "starting" | "ready" | "failed" | "external";
  pid: number | null;
  /** 上次真正把这个 profile 跑起来的 dsh 版本；null = 从未记录（首次启动） */
  boundVersion: string | null;
  webUrl: string | null;
  code: number | null;
}

interface Props {
  /** 内置 web profile 的实例行；null = dsh 还没初始化（没有任何 profile） */
  row: QuickInstanceRow | null;
  /** 环境探测结果（OS / Node / npm），用于底部「环境信息」卡 */
  env: EnvironmentInfo;
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
  onOpenDeepSeek: () => void;
  onNavigate: (v: View) => void;
  onInitDsh: () => void;
  /** 打开该 profile 的配置工作台（跳「Profiles」页并选中） */
  onConfigure: (profile: string) => void;
}

const PHASE_TEXT: Record<QuickInstanceRow["phase"], string> = {
  stopped: "未运行",
  starting: "启动中…",
  ready: "运行中",
  failed: "启动失败",
  external: "运行中",
};

/** 首页的 DeepSeek 官方对话入口：应用内独立窗口打开，支持同时开多个窗口 */
function DeepSeekCard({ onOpen }: { onOpen: () => void }) {
  return (
    <Card className="group relative overflow-hidden p-5 animate-in fade-in slide-in-from-bottom-2 fill-mode-backwards duration-300 [animation-delay:70ms]">
      {/* 装饰：主题蓝渐变洗底 + 蓝色鲸鱼水印 + 对话气泡点阵（低透明度，深浅主题都不抢内容） */}
      <div aria-hidden className="pointer-events-none absolute inset-0 bg-gradient-to-tr from-blue-500/10 via-transparent to-transparent" />
      <WhaleMark className="pointer-events-none absolute -bottom-9 -right-8 h-40 w-40 scale-100 text-blue-500/[0.14] transition-transform duration-300 group-hover:scale-105" />
      <svg
        aria-hidden
        viewBox="0 0 120 40"
        className="pointer-events-none absolute top-4 right-10 hidden h-10 w-28 text-blue-500/[0.20] sm:block"
        fill="currentColor"
      >
        <rect x="0" y="0" width="46" height="26" rx="9" />
        <path d="M10 26 L8 34 L18 26 Z" />
        <rect x="56" y="8" width="46" height="26" rx="9" opacity=".55" />
        <path d="M94 34 L92 26 L84 34 Z" opacity=".55" transform="rotate(180 90 31)" />
      </svg>
      <div className="relative z-10 flex flex-wrap items-center gap-x-3.5 gap-y-3">
        <span className="grid h-11 w-11 shrink-0 place-items-center rounded-xl bg-blue-500/10 text-blue-500">
          <WhaleMark className="h-6 w-6" />
        </span>
        <div className="min-w-0 grow basis-[calc(100%-3.5rem)] xl:basis-0">
          <div className="text-[15px] font-semibold">
            DeepSeek{" "}
            <span className="bg-gradient-to-r from-sky-500 to-blue-600 bg-clip-text italic text-transparent">
              Chat
            </span>
          </div>
          <div className="truncate text-xs text-muted-foreground">
            在应用内独立窗口打开官方 chat.deepseek.com，可开多个窗口并行会话
          </div>
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-2">
          <Button variant="outline" onClick={onOpen} title="打开 https://chat.deepseek.com/（每次新开一个窗口）">
            <ExternalLink /> <span className="hidden sm:inline">打开 DeepSeek</span>
          </Button>
        </div>
      </div>
    </Card>
  );
}

/** 环境信息卡：OS / Node / npm / dsh 版本 Bento 小格一卡看全，整卡可点跳「版本与安装」 */
function EnvInfoCard({ env, version, onOpen }: { env: EnvironmentInfo; version: string; onOpen: () => void }) {
  const tiles: Array<{
    icon: typeof Monitor; label: string; value: string;
    tone: "ok" | "warn" | "bad"; title: string;
  }> = [
    { icon: Monitor, label: "系统", value: `${env.os} / ${env.arch}`, tone: "ok", title: "操作系统与 CPU 架构" },
    {
      icon: Rocket, label: "dsh", value: version || "未选择",
      tone: version ? "ok" : "warn", title: `当前生效的 dsh 版本：${version || "尚未选择"}，点击管理版本`,
    },
    {
      icon: Cpu, label: "Node", value: env.node ? `v${env.node}` : "未安装",
      tone: env.node ? "ok" : "bad",
      title: env.node ? `Node 路径：${env.nodePath ?? "未知"}` : "未检测到 Node：可在「版本与安装」页安装内置 Node",
    },
    {
      icon: Package, label: "npm", value: env.npm ?? "未安装",
      tone: env.npm ? "ok" : env.npmPath ? "warn" : "bad",
      title: env.npm ? `npm 路径：${env.npmPath ?? "未知"}` : "未检测到 npm：安装 dsh 版本需要它",
    },
  ];
  return (
    <Card
      role="button"
      tabIndex={0}
      aria-label="环境信息：前往版本与安装"
      title="前往「版本与安装」页"
      onClick={onOpen}
      onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onOpen(); } }}
      className="group relative cursor-pointer overflow-hidden p-4 transition-all duration-200 hover:border-primary/40 hover:shadow-md active:scale-[0.995] focus-visible:border-primary/60 focus-visible:ring-2 focus-visible:ring-primary/20"
    >
      {/* 装饰：主题色渐变洗底 + 鲸鱼水印（悬停时水印轻微放大） */}
      <div aria-hidden className="pointer-events-none absolute inset-0 bg-gradient-to-br from-primary/10 via-transparent to-transparent" />
      <WhaleMark className="pointer-events-none absolute -bottom-8 -right-6 h-28 w-28 scale-100 text-primary/[0.08] transition-transform duration-300 group-hover:scale-110" />
      <div className="relative z-10">
        <div className="flex items-center gap-2.5">
          <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg bg-primary/10 text-primary">
            <Gauge className="h-4 w-4" />
          </span>
          <div className="min-w-0 grow">
            <div className="text-[13px] font-semibold">环境信息</div>
            <div className="text-[11px] text-muted-foreground">点击管理 dsh 版本与运行环境</div>
          </div>
          <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground transition-transform duration-200 group-hover:translate-x-1" />
        </div>
        <div className="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
          {tiles.map((t) => (
            <div
              key={t.label}
              title={t.title}
              className="flex items-center gap-2.5 rounded-lg border border-border/60 bg-background/60 px-3 py-2"
            >
              <t.icon className="h-4 w-4 shrink-0 text-muted-foreground" aria-hidden />
              <div className="min-w-0">
                <div className="text-[10.5px] font-medium uppercase tracking-wide text-muted-foreground">{t.label}</div>
                <div className="truncate font-mono text-[12.5px] font-medium">{t.value}</div>
              </div>
              <span
                className={`ml-auto h-2 w-2 shrink-0 rounded-full ${
                  t.tone === "ok" ? "bg-emerald-500" : t.tone === "warn" ? "bg-amber-500" : "bg-red-500"
                }`}
                aria-hidden
              />
            </div>
          ))}
        </div>
      </div>
    </Card>
  );
}

/**
 * 「首页」：内置 web profile 的一键起停 + 配置入口，外加 DeepSeek 官方对话快捷卡。
 * 状态、地址、动作全部收在一张卡里，其余功能各页自管。
 */
export default function QuickActionsView(props: Props) {
  const {
    row, env, activeVersion, hasNode, hasInstalled, starting, restarting, initBusy,
    onStart, onStop, onRestart, onOpenWeb, onOpenDeepSeek, onNavigate, onInitDsh, onConfigure,
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
        <DeepSeekCard onOpen={onOpenDeepSeek} />
        <EnvInfoCard env={env} version={activeVersion} onOpen={() => onNavigate("versions")} />
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
        (row.webUrl
          ? ` · ${row.webUrl}`
          : !running && row.boundVersion
            ? ` · 上次 dsh ${row.boundVersion}`
            : ` · 基于 ${activeVersion || "（未选择版本）"}`);

  return (
    <div className="mx-auto w-full max-w-2xl space-y-4">
      <h2 className="text-base font-semibold">首页</h2>

      <Card className="group relative overflow-hidden p-5 animate-in fade-in slide-in-from-bottom-2 fill-mode-backwards duration-300">
        {/* 装饰：主题色渐变洗底 + dsh（harness）鲸鱼水印 + 底部波浪线，低透明度不抢内容 */}
        <div aria-hidden className="pointer-events-none absolute inset-0 bg-gradient-to-tr from-primary/10 via-transparent to-transparent" />
        <WhaleMark className="pointer-events-none absolute -bottom-9 -right-8 h-40 w-40 scale-100 text-foreground/[0.10] transition-transform duration-300 group-hover:scale-105" />
        <svg
          aria-hidden
          viewBox="0 0 400 56"
          preserveAspectRatio="none"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          className="pointer-events-none absolute inset-x-0 bottom-0 h-12 w-full text-foreground/[0.15]"
        >
          <path d="M0 30 C 40 18, 60 42, 100 30 S 160 18, 200 30 S 260 42, 300 30 S 360 18, 400 30" />
          <path d="M0 44 C 40 32, 60 56, 100 44 S 160 32, 200 44 S 260 56, 300 44 S 360 32, 400 44" opacity=".5" />
        </svg>
        {/* 窄窗口：按钮排整行换到第二行（右对齐），不再与标题/徽章叠在一起 */}
        <div className="relative z-10 flex flex-wrap items-center gap-x-3.5 gap-y-3">
          {/* 图标：dsh 官方黑鲸 logo；右下角状态点一眼看出跑没跑 */}
          <span className="relative grid h-11 w-11 shrink-0 place-items-center rounded-xl bg-muted">
            <img src="/dsh-logo.svg" alt="dsh" className="h-7 w-7" />
            <span
              className={`absolute -right-0.5 -bottom-0.5 h-3.5 w-3.5 rounded-full ring-2 ring-card ${
                row.phase === "starting" || starting
                  ? "animate-pulse bg-amber-500"
                  : row.phase === "ready" || row.phase === "external"
                    ? "bg-emerald-500"
                    : row.phase === "failed"
                      ? "bg-red-500"
                      : "bg-muted-foreground/40"
              }`}
            />
          </span>
          <div className="min-w-0 grow basis-[calc(100%-3.5rem)] xl:basis-0">
            <div className="bg-gradient-to-r from-foreground via-primary/70 to-primary bg-clip-text text-[15px] font-semibold text-transparent">
              DeepSeek Harness
            </div>
            <div className="truncate text-xs text-muted-foreground" title={statusLine}>
              {statusLine}
            </div>
          </div>
          {/* 超窄时只剩图标（文字进 tooltip），换行后整排右对齐 */}
          <div className="ml-auto flex shrink-0 items-center gap-2">
            <Button variant="outline" onClick={() => onConfigure(row.profile)} title="打开 web 的配置工作台">
              <SlidersHorizontal /> <span className="hidden sm:inline">配置</span>
            </Button>
            {running ? (
              <>
                {row.webUrl && (
                  <Button onClick={() => onOpenWeb(row.webUrl!)} title={`打开 DSH 界面：${row.webUrl}`}>
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
              <Button size="lg" disabled={disabled || busy} title={disabled ? disabledTitle : row.boundVersion && row.boundVersion !== activeVersion
                ? `上次用 dsh ${row.boundVersion} 跑起来，本次将用 ${activeVersion}；版本变化可能导致该 profile 起不来，会先让你确认风险`
                : `基于当前版本（${activeVersion}）启动 web`} onClick={() => onStart(row.profile)}>
                {busy ? <Loader2 className="animate-spin" /> : <Play />} {row.phase === "failed" ? "重试启动" : "启动"}
              </Button>
            )}
          </div>
        </div>
        {row.phase === "failed" && (
          <p className="relative z-10 mt-3 border-t border-border pt-3 text-[11.5px] leading-relaxed text-muted-foreground">
            启动失败：可在「终端面板」查看日志尾部；若是插件导致，到「Profiles」页选中 web 工作台，在「插件」Tab 停用可疑插件后重试。
          </p>
        )}
      </Card>
      <DeepSeekCard onOpen={onOpenDeepSeek} />
      <div className="animate-in fade-in slide-in-from-bottom-2 fill-mode-backwards duration-300 [animation-delay:140ms]">
        <EnvInfoCard env={env} version={activeVersion} onOpen={() => onNavigate("versions")} />
      </div>
    </div>
  );
}
