import { forwardRef, useMemo } from "react";
import { Card, CardContent } from "@/components/ui/card";
import type { SessionStats, ShareIdentity } from "../types";
import { ModelUsageBoard, OnlineDayChart, StackModelChart, type StackPoint } from "./stats-charts";
import { fmtDur, fmtNum, fmtTok, Heatmap, md, ModelLegend, SERIES_COLORS, StatCard } from "./stats-parts";

interface Props {
  stats: SessionStats;
  identity: ShareIdentity | null;
  appVersion: string;
  /** 分享卡在线口径跟随面板当前选中的阈值 */
  gapMin: number;
}

const REPO = "github.com/Yinxe/dsh-starter";

/** 分享卡的图表标题排印（与页面内 ChartCard 同款，静态无交互） */
function BoardTitle({ eyebrow, title }: { eyebrow: string; title: string }) {
  return (
    <div className="mb-2">
      <div className="text-[10px] font-medium uppercase tracking-[0.16em] text-[var(--eyebrow)]">{eyebrow}</div>
      <div className="text-sm font-semibold">{title}</div>
    </div>
  );
}

/**
 * 分享卡（流式板：宽屏封顶 1280px，窄屏随容器降列）。导出时对这块 DOM 做 2× 光栅化，
 * 版式：页头（署名/来源条）→ 三大主数 → KPI 阵列 → 趋势 → 在线 → 热力图 → 模型用量分布。
 * 所有图表关闭动画（disableAnimation），保证逐帧光栅化的确定性。
 */
export const ShareBoard = forwardRef<HTMLDivElement, Props>(function ShareBoard(
  { stats, identity, appVersion, gapMin },
  ref,
) {
  const ov = stats.overview;
  const on = stats.online;
  const gap = String(gapMin);

  const modelNames = useMemo(() => stats.models.map((m) => m.model), [stats]);
  const legend = stats.models.slice(0, SERIES_COLORS.length);
  const otherTokens = stats.models.slice(SERIES_COLORS.length).reduce((a, b) => a + b.tokens, 0);

  const trendPoints = useMemo<StackPoint[]>(
    () => stats.trend.map((p) => ({ label: md(p.day), full: p.day, parts: p.byModel, total: p.total, calls: p.calls })),
    [stats],
  );

  const totalOnline = on.totalMs[gap] ?? 0;

  return (
    <div ref={ref} className="mx-auto w-full min-w-[720px] max-w-[1280px] space-y-4 bg-background p-6 text-foreground sm:p-10">
      <header className="space-y-2">
        <div className="flex items-end justify-between border-b border-border pb-3">
          <div>
            <div className="text-2xl font-bold tracking-tight">DSH Starter · 用量总览</div>
            <div className="mt-0.5 text-xs text-muted-foreground">dsh 会话 Token 用量与在线时长统计</div>
          </div>
          {(identity?.name || identity?.email) && (
            <div className="text-right">
              {identity?.name && <div className="text-sm font-semibold">{identity.name}</div>}
              {identity?.email && <div className="text-xs text-muted-foreground">{identity.email}</div>}
            </div>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-x-2 text-[11px] text-muted-foreground">
          <span className="font-mono font-medium text-foreground">dsh-starter v{appVersion}</span>
          <span>·</span>
          <span className="font-mono">{REPO}</span>
          <span className="ml-auto">
            会话 {stats.sessionsTotal} 个 · 记录 {ov.activeDays} 天 · 阈值 {gapMin} 分钟 · 生成于{" "}
            {new Date().toLocaleString("zh-CN", { hour12: false })}
          </span>
        </div>
      </header>

      {/* 三大主数 */}
      <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
        <Card className="border-primary/25 bg-gradient-to-br from-secondary/70 via-card to-card">
          <CardContent className="p-4">
            <div className="text-[10px] font-medium uppercase tracking-[0.16em] text-[var(--eyebrow)]">总 Token · 全历史</div>
            <div className="mt-1 font-mono text-3xl font-bold tabular-nums">{fmtTok(ov.totalTokens)}</div>
            <div className="mt-1 text-[11px] text-muted-foreground">
              {fmtNum(ov.calls)} 次请求 · 输入 {fmtTok(ov.totalInput)} / 输出 {fmtTok(ov.totalOutput)}
            </div>
          </CardContent>
        </Card>
        <Card className="border-primary/25 bg-gradient-to-br from-secondary/70 via-card to-card">
          <CardContent className="p-4">
            <div className="text-[10px] font-medium uppercase tracking-[0.16em] text-[var(--eyebrow)]">累计在线 · {gapMin} 分档</div>
            <div className="mt-1 font-mono text-3xl font-bold tabular-nums">{fmtDur(totalOnline)}</div>
            <div className="mt-1 text-[11px] text-muted-foreground">
              {fmtNum(on.segments[gap] ?? 0)} 段 · {on.activeDays} 个活跃日
            </div>
          </CardContent>
        </Card>
        <Card className="border-primary/25 bg-gradient-to-br from-secondary/70 via-card to-card">
          <CardContent className="p-4">
            <div className="text-[10px] font-medium uppercase tracking-[0.16em] text-[var(--eyebrow)]">连续使用</div>
            <div className="mt-1 font-mono text-3xl font-bold tabular-nums">{ov.currentStreak} 天</div>
            <div className="mt-1 text-[11px] text-muted-foreground">
              最长 {ov.longestStreak} 天 · 日均 {fmtTok(ov.avgDay)}
            </div>
          </CardContent>
        </Card>
      </div>

      {/* KPI 阵列 */}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-3">
        <StatCard label="今日" value={fmtTok(stats.today.total)} sub={`${fmtNum(stats.today.calls)} 次请求 · 昨日 ${fmtTok(stats.today.yesterdayTotal)}`} />
        <StatCard label="峰值日" value={ov.peakDay ? fmtTok(ov.peakDay.tokens) : "—"} sub={ov.peakDay?.day} />
        <StatCard label="最大单请求" value={ov.peakStep ? fmtTok(ov.peakStep.tokens) : "—"} sub={ov.peakStep ? `${ov.peakStep.day} · ${ov.peakStep.model}` : undefined} />
        <StatCard label="有用量 / 全部会话" value={`${stats.sessionsWithUsage} / ${stats.sessionsTotal}`} sub={`阈值口径 ${gapMin} 分钟`} />
        <StatCard label="对话进行中" value={fmtDur(on.turnMs)} sub="turn 区间并集" />
        <StatCard label="模型生成" value={fmtDur(on.llmMs)} sub={`工具执行 ${fmtDur(on.toolMs)} · 合计 ${fmtDur(on.llmMs + on.toolMs)}`} />
      </div>

      {/* 趋势 */}
      <Card>
        <CardContent className="p-4">
          <BoardTitle eyebrow="Trend" title={`近 ${stats.rangeDays} 天按日 Token（堆叠=模型）`} />
          <StackModelChart points={trendPoints} models={modelNames} height={230} disableAnimation emptyHint="没有记录" />
          <ModelLegend models={legend} otherTokens={otherTokens} />
        </CardContent>
      </Card>

      {/* 每日在线整行 */}
      <Card>
        <CardContent className="p-4">
          <BoardTitle eyebrow="Daily Online" title={`近 30 天每日在线（阈值 ${gapMin} 分钟）`} />
          <OnlineDayChart
            days={on.days.slice(-30)}
            gap={gap}
            height={232}
            disableAnimation
            emptyHint="没有在线记录"
          />
        </CardContent>
      </Card>

      {/* 热力图整行（53 周需要全宽） */}
      <Card>
        <CardContent className="p-4">
          <BoardTitle eyebrow="Activity" title="近 53 周 Token 活动热力图" />
          <Heatmap cells={stats.heatmap} />
        </CardContent>
      </Card>

      {/* 模型用量分布：环形图 + 卡片网格（导出时静态呈现，悬浮明细不出现） */}
      <Card>
        <CardContent className="p-4">
          <BoardTitle eyebrow="Models" title={`模型用量分布（全历史 ${stats.models.length} 个模型）`} />
          <ModelUsageBoard models={stats.models} disableAnimation />
        </CardContent>
      </Card>

      <footer className="flex items-center justify-between pt-1 text-[11px] text-muted-foreground">
        <span>口径与 dsh-token-stats 插件一致：fork/resume 去重；在线为阈值口径下的墙钟下界。</span>
        <span className="font-mono">{REPO}</span>
      </footer>
    </div>
  );
});
