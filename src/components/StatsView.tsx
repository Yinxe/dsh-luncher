import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Activity, CircleOff, Copy, Download, Loader2, RefreshCw, Share2, TrendingUp } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { api } from "../api";
import type { SessionStats, ShareIdentity } from "../types";
import { ShareBoard } from "./ShareBoard";
import { copyPng, downloadBlob, nodeToPngBlob } from "@/lib/share-export";
import { ModelRankChart, OnlineDayChart, StackModelChart, type StackPoint } from "./stats-charts";
import {
  fmtClock,
  fmtDur,
  fmtNum,
  fmtTok,
  Heatmap,
  md,
  ModelLegend,
  SERIES_COLORS,
  ShareBar,
  StatCard,
} from "./stats-parts";

interface Props {
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
  appVersion: string;
}

/** 在线时长表的取数范围（天）；"all" = 全部记录日 */
const DAY_WINDOWS = ["14", "30", "90", "all"] as const;
type DayWindow = (typeof DAY_WINDOWS)[number];

/** 图表卡：统一标题排印（eyebrow 小标 + 大标题），右上放窗口切换等操作 */
function ChartCard({
  eyebrow,
  title,
  desc,
  actions,
  children,
}: {
  eyebrow: string;
  title: string;
  desc?: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <Card>
      <CardHeader className="flex-row items-start justify-between space-y-0 pb-2">
        <div className="min-w-0">
          <div className="text-[10px] font-medium uppercase tracking-[0.14em] text-[var(--eyebrow)]">{eyebrow}</div>
          <CardTitle className="mt-0.5 text-sm">{title}</CardTitle>
          {desc && <CardDescription className="mt-1 text-[11px] leading-4">{desc}</CardDescription>}
        </div>
        {actions && <div className="shrink-0">{actions}</div>}
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  );
}

/** 会话统计 + 在线时长统计：读 ~/.dsh 会话日志（只读）聚合，缓存落在启动器自己的目录 */
export default function StatsView({ onToast, appVersion }: Props) {
  const [tab, setTab] = useState<"sessions" | "online">("sessions");
  const [range, setRange] = useState<string>("30");
  const [gap, setGap] = useState<string>("15");
  const [stats, setStats] = useState<SessionStats | null>(null);
  const [busy, setBusy] = useState(false);
  const [dayWindow, setDayWindow] = useState<DayWindow>("30");

  const load = useCallback(
    async (rangeDays: number, gapMin: number, silent: boolean) => {
      setBusy(true);
      try {
        const s = await api.getSessionStats(rangeDays, gapMin);
        setStats(s);
        // 首次加载后用后端归一好的默认阈值回填选择器
        if (!silent) setGap(String(s.online.defaultGapMin));
      } catch (e) {
        if (!silent) onToast("err", `读取会话统计失败: ${e}`);
      } finally {
        setBusy(false);
      }
    },
    [onToast],
  );

  useEffect(() => {
    load(Number(range), Number(gap), false);
    // gap 变化重算是刻意的：阈值只影响快照合并（缓存不动），秒级返回
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [range, gap]);

  // 日志在长会话中持续增长：停留在页面上时每 60s 静默刷一轮
  useEffect(() => {
    const id = window.setInterval(() => {
      if (stats !== null && Date.now() - stats.generatedAt > 60_000) void load(Number(range), Number(gap), true);
    }, 15_000);
    return () => window.clearInterval(id);
  }, [stats, range, gap, load]);

  // ── 派生 ──
  const modelNames = useMemo(() => (stats?.models ?? []).map((m) => m.model), [stats]);
  const colorOf = useMemo(() => {
    const m = new Map<string, string>();
    (stats?.models ?? []).slice(0, SERIES_COLORS.length).forEach((x, i) => m.set(x.model, SERIES_COLORS[i]));
    return m;
  }, [stats]);
  const legend = useMemo(() => (stats?.models ?? []).slice(0, SERIES_COLORS.length), [stats]);
  const otherTokens = useMemo(
    () => (stats?.models ?? []).slice(SERIES_COLORS.length).reduce((a, b) => a + b.tokens, 0),
    [stats],
  );

  const trendPoints = useMemo<StackPoint[]>(
    () => (stats?.trend ?? []).map((p) => ({ label: md(p.day), full: p.day, parts: p.byModel, total: p.total, calls: p.calls })),
    [stats],
  );
  const todayPoints = useMemo<StackPoint[]>(
    () =>
      (stats?.today.hours ?? []).map((h) => ({
        label: String(h.hour).padStart(2, "0"),
        full: `${String(h.hour).padStart(2, "0")}:00 起一小时`,
        parts: h.byModel,
        total: h.total,
      })),
    [stats],
  );

  // ── 在线时长派生 ──
  const on = stats?.online;
  const gapNum = Number(gap);
  const todayDay = useMemo(() => {
    if (!on) return null;
    return on.days.find((d) => d.d === stats!.today.day) ?? null;
  }, [on, stats]);
  /** 表/柱图按所选窗口截取（days 已按日期升序） */
  const windowDays = useMemo(() => {
    if (!on) return [];
    return dayWindow === "all" ? on.days : on.days.slice(-Number(dayWindow));
  }, [on, dayWindow]);
  const rankDays = useMemo(() => [...windowDays].sort((a, b) => (b.byGap[gap] ?? 0) - (a.byGap[gap] ?? 0)), [windowDays, gap]);

  // ── 分享面板 ──
  const [shareOpen, setShareOpen] = useState(false);
  const [identity, setIdentity] = useState<ShareIdentity | null>(null);
  const [exporting, setExporting] = useState(false);
  const boardRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (shareOpen && identity === null) {
      api.getShareIdentity().then(setIdentity).catch(() => setIdentity({ name: "", email: "", via: "none" }));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shareOpen]);

  const doExport = useCallback(
    async (kind: "download" | "copy") => {
      if (!boardRef.current || !stats) return;
      setExporting(true);
      try {
        const blob = await nodeToPngBlob(boardRef.current);
        if (kind === "download") {
          downloadBlob(blob, `dsh-stats-${stats.today.day}.png`);
          onToast("ok", "已导出 PNG（2× 分辨率），可直接发图");
        } else if (await copyPng(blob)) {
          onToast("ok", "图片已复制到剪贴板");
        } else {
          onToast("info", "浏览器不允许直接复制图片，已改为下载，请把文件手动转发");
          downloadBlob(blob, `dsh-stats-${stats.today.day}.png`);
        }
      } catch (e) {
        onToast("err", `导出失败: ${e}`);
      } finally {
        setExporting(false);
      }
    },
    [stats, onToast],
  );

  const ov = stats?.overview;

  return (
    <div className="mx-auto max-w-5xl space-y-4">
      {/* 吸顶工具条：切页/阈值/时间窗随时可达，长页面不再滚丢 */}
      <div className="sticky top-0 z-20 -mx-1 flex flex-wrap items-center gap-2 rounded-xl border border-border/60 bg-background/85 px-3 py-2 shadow-[var(--card-shadow)] backdrop-blur supports-[backdrop-filter]:bg-background/70">
        <ToggleGroup
          type="single"
          variant="outline"
          spacing={0}
          value={tab}
          onValueChange={(v) => v && setTab(v as "sessions" | "online")}
        >
          <ToggleGroupItem value="sessions" className="gap-1.5 text-xs">
            <TrendingUp className="h-3.5 w-3.5" /> 会话统计
          </ToggleGroupItem>
          <ToggleGroupItem value="online" className="gap-1.5 text-xs">
            <Activity className="h-3.5 w-3.5" /> 在线时长
          </ToggleGroupItem>
        </ToggleGroup>
        {tab === "online" && on && (
          <ToggleGroup
            type="single"
            variant="outline"
            spacing={0}
            value={gap}
            onValueChange={(v) => v && setGap(v)}
          >
            {on.gaps.map((g) => (
              <ToggleGroupItem key={g} value={String(g)} className="h-8 px-2 text-xs">{g} 分</ToggleGroupItem>
            ))}
          </ToggleGroup>
        )}
        <div className="ml-auto flex flex-wrap items-center gap-2">
          <ToggleGroup
            type="single"
            variant="outline"
            spacing={0}
            value={range}
            onValueChange={(v) => v && setRange(v)}
          >
            {["7", "30", "90", "365"].map((r) => (
              <ToggleGroupItem key={r} value={r} className="h-8 px-2.5 text-xs">{r} 天</ToggleGroupItem>
            ))}
          </ToggleGroup>
          {stats && (
            <Button variant="outline" size="sm" onClick={() => setShareOpen(true)}>
              <Share2 /> 分享
            </Button>
          )}
          <Button variant="outline" size="sm" disabled={busy} onClick={() => void load(Number(range), Number(gap), false)}>
            {busy ? <Loader2 className="animate-spin" /> : <RefreshCw />} 刷新
          </Button>
        </div>
      </div>

      {tab === "sessions" && (
        <>
          {stats === null && busy ? (
            <div className="space-y-4">
              <div className="text-xs text-muted-foreground">
                正在扫描本机 dsh 会话日志（首次全量扫描可能需要几十秒，之后按文件指纹增量秒开）…
              </div>
              <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
                {Array.from({ length: 8 }, (_, i) => <Skeleton key={i} className="h-[76px]" />)}
              </div>
              <Skeleton className="h-40" />
            </div>
          ) : stats === null ? (
            <Card>
              <CardContent className="flex flex-col items-center gap-3 py-10 text-sm text-muted-foreground">
                <CircleOff className="h-6 w-6" />
                暂时读不到统计数据 —— 如果本机还没有运行过 dsh 会话，这里会是空的；
                否则点右上角「刷新」重试。
              </CardContent>
            </Card>
          ) : (
            <>
              {/* 头图：总量 + 今日，一眼看到主数字 */}
              <Card className="border-primary/25 bg-gradient-to-br from-secondary/70 via-card to-card">
                <CardContent className="flex flex-wrap items-end justify-between gap-x-8 gap-y-3 p-5">
                  <div className="min-w-0">
                    <div className="text-[10px] font-medium uppercase tracking-[0.16em] text-[var(--eyebrow)]">
                      总 Token · 全历史
                    </div>
                    <div className="mt-1 truncate font-mono text-4xl font-bold leading-tight tabular-nums">
                      {fmtTok(ov!.totalTokens)}
                    </div>
                    <div className="mt-1 text-[11px] text-muted-foreground">
                      {fmtNum(ov!.calls)} 次请求 · 输入 {fmtTok(ov!.totalInput)} / 输出 {fmtTok(ov!.totalOutput)} · 缓存读写{" "}
                      {fmtTok(ov!.totalCacheRead)} / {fmtTok(ov!.totalCacheWrite)}
                    </div>
                  </div>
                  <div className="text-right">
                    <div className="text-[10px] font-medium uppercase tracking-[0.16em] text-[var(--eyebrow)]">今日</div>
                    <div className="mt-1 font-mono text-2xl font-bold tabular-nums">{fmtTok(stats.today.total)}</div>
                    <div className="mt-1 text-[11px] text-muted-foreground">
                      {stats.today.calls} 次请求 · 昨日 {fmtTok(stats.today.yesterdayTotal)}
                    </div>
                  </div>
                </CardContent>
              </Card>

              {stats.errors > 0 && (
                <Badge variant="destructive">{stats.errors} 个日志读取失败（不计入统计，仅提示）</Badge>
              )}

              {/* KPI 阵列 */}
              <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
                <StatCard
                  label="活跃天数"
                  value={`${ov!.activeDays} 天`}
                  sub={ov!.firstDay ? `${ov!.firstDay} 起记录在案` : "暂无记录"}
                />
                <StatCard
                  label="连续使用"
                  value={`当前 ${ov!.currentStreak} 天`}
                  sub={`最长 ${ov!.longestStreak} 天 · 日均 ${fmtTok(ov!.avgDay)}`}
                />
                <StatCard
                  label="峰值日"
                  value={ov!.peakDay ? fmtTok(ov!.peakDay.tokens) : "—"}
                  sub={ov!.peakDay ? ov!.peakDay.day : undefined}
                />
                <StatCard
                  label="最大单请求"
                  value={ov!.peakStep ? fmtTok(ov!.peakStep.tokens) : "—"}
                  sub={ov!.peakStep ? `${ov!.peakStep.day} · ${ov!.peakStep.model}` : undefined}
                />
                <StatCard
                  label="会话数"
                  value={`${stats.sessionsWithUsage} / ${stats.sessionsTotal}`}
                  sub="有 Token 用量 / 全部会话"
                />
                <StatCard
                  label="数据生成"
                  value={fmtClock(stats.generatedAt)}
                  sub={`近 ${stats.rangeDays} 天窗口`}
                />
              </div>

              {/* 近 N 天趋势 */}
              <ChartCard
                eyebrow="Trend"
                title={`近 ${stats.rangeDays} 天按日 Token（堆叠=模型）`}
                desc="口径：input + output + cacheRead + cacheWrite，与 dsh-token-stats 插件一致；fork/resume 会话已去重。"
              >
                <StackModelChart points={trendPoints} models={modelNames} height={300} emptyHint="该时间段内没有 Token 记录" />
                <ModelLegend models={legend} otherTokens={otherTokens} />
              </ChartCard>

              {/* 今日 24h + 热力图 */}
              <div className="grid gap-4 xl:grid-cols-[1fr_1fr]">
                <ChartCard eyebrow="Today" title={`今日 24 小时分布（${stats.today.day}）`}>
                  <StackModelChart points={todayPoints} models={modelNames} height={220} emptyHint="今天还没有 Token 记录" />
                </ChartCard>
                <ChartCard eyebrow="Activity" title="近 53 周活跃热力图" desc="悬停查看单日 Token 与活跃会话数。">
                  <Heatmap cells={stats.heatmap} />
                </ChartCard>
              </div>

              {/* 模型排行 + 明细 */}
              <ChartCard eyebrow="Models" title="模型用量（全历史 Top 8）">
                <ModelRankChart models={stats.models} />
              </ChartCard>
              <Card>
                <CardHeader>
                  <CardTitle className="text-sm">模型用量明细（全历史）</CardTitle>
                </CardHeader>
                <CardContent className="overflow-x-auto px-0">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>模型</TableHead>
                        <TableHead className="w-[160px]">占比</TableHead>
                        <TableHead className="text-right">Token</TableHead>
                        <TableHead className="text-right">输入</TableHead>
                        <TableHead className="text-right">输出</TableHead>
                        <TableHead className="text-right">缓存读</TableHead>
                        <TableHead className="text-right">缓存写</TableHead>
                        <TableHead className="text-right">请求</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {stats.models.map((m) => (
                        <TableRow key={m.model}>
                          <TableCell className="max-w-[220px] truncate font-mono text-xs" title={m.model}>{m.model}</TableCell>
                          <TableCell><ShareBar share={m.share} color={colorOf.get(m.model)} /></TableCell>
                          <TableCell className="text-right font-mono tabular-nums">{fmtTok(m.tokens)}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{fmtTok(m.input)}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{fmtTok(m.output)}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{fmtTok(m.cacheRead)}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{fmtTok(m.cacheWrite)}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{fmtNum(m.calls)}</TableCell>
                        </TableRow>
                      ))}
                      {stats.models.length === 0 && (
                        <TableRow>
                          <TableCell colSpan={8} className="py-8 text-center text-sm text-muted-foreground">
                            还没有任何模型用量记录
                          </TableCell>
                        </TableRow>
                      )}
                    </TableBody>
                  </Table>
                </CardContent>
              </Card>

              <div className="pb-2 text-center text-[11px] text-muted-foreground">
                统计缓存位于 ~/.dsh-starter/session-stats-cache.json（可随时删除重建）
              </div>
            </>
          )}
        </>
      )}

      {tab === "online" && (
        <>
          {stats === null || !on ? (
            <Card>
              <CardContent className="flex flex-col items-center gap-3 py-10 text-sm text-muted-foreground">
                <CircleOff className="h-6 w-6" />
                正在准备在线时长数据 —— 首次全量扫描可能需要几十秒，请稍候或到「会话统计」页触发扫描。
              </CardContent>
            </Card>
          ) : (
            <>
              {/* 头图：当前阈值下的累计在线 */}
              <Card className="border-primary/25 bg-gradient-to-br from-secondary/70 via-card to-card">
                <CardContent className="flex flex-wrap items-end justify-between gap-x-8 gap-y-3 p-5">
                  <div className="min-w-0">
                    <div className="text-[10px] font-medium uppercase tracking-[0.16em] text-[var(--eyebrow)]">
                      累计在线 · {gap} 分钟阈值
                    </div>
                    <div className="mt-1 truncate font-mono text-4xl font-bold leading-tight tabular-nums">
                      {fmtDur(on.totalMs[gap] ?? 0)}
                    </div>
                    <div className="mt-1 text-[11px] text-muted-foreground">
                      {on.segments[gap] ?? 0} 段 · {on.activeDays} 个活跃日
                      {on.firstDay ? ` · ${on.firstDay} → ${on.lastDay ?? ""}` : ""}
                    </div>
                  </div>
                  <div className="text-right">
                    <div className="text-[10px] font-medium uppercase tracking-[0.16em] text-[var(--eyebrow)]">今日在线</div>
                    <div className="mt-1 font-mono text-2xl font-bold tabular-nums">
                      {fmtDur(todayDay?.byGap[gap] ?? 0)}
                    </div>
                    <div className="mt-1 text-[11px] text-muted-foreground">
                      {todayDay ? `${todayDay.segByGap[gap] ?? 0} 段 · 对话 ${fmtDur(todayDay.turnMs)}` : stats.today.day}
                    </div>
                  </div>
                </CardContent>
              </Card>

              {/* 口径说明 */}
              <Card>
                <CardContent className="py-3 text-[11px] leading-5 text-muted-foreground">
                  在线 = 日志里有事件、且相邻事件间隔不超过阈值的墙钟时间：间隔 ≤ 阈值 → 整段算在线；
                  间隔 &gt; 阈值 → 断开；每段以最后一个事件收尾，所以任何时刻的「在线」都是<b>下界</b>；
                  多个会话的重叠时间取并集，不重复计（阈值切换在上方工具条）。
                </CardContent>
              </Card>

              {/* 三口径 + 派生指标 */}
              <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
                <StatCard
                  label="活跃日均在线"
                  value={fmtDur(on.activeDays > 0 ? (on.totalMs[gap] ?? 0) / on.activeDays : 0)}
                  sub={`阈值 ${gap} 分钟`}
                />
                <StatCard label="连续使用" value={`${ov!.currentStreak} 天`} sub={`最长 ${ov!.longestStreak} 天`} />
                <StatCard label="对话进行中" value={fmtDur(on.turnMs)} sub="turn/start→end 并集（墙钟去重）" />
                <StatCard label="模型生成" value={fmtDur(on.llmMs)} sub="step/start→响应落地的墙钟" />
                <StatCard label="工具执行" value={fmtDur(on.toolMs)} sub="tool/call→result 按调用配对" />
                <StatCard label="引擎合计" value={fmtDur(on.llmMs + on.toolMs)} sub="模型 + 工具墙钟相加" />
              </div>

              {/* 每日在线 */}
              <ChartCard
                eyebrow="Daily"
                title={`每日在线（阈值 ${gap} 分钟）`}
                desc="柱=在线时长（阈值口径下界），线=对话进行中；跨零点活动段按本地日切分。"
                actions={
                  <ToggleGroup
                    type="single"
                    variant="outline"
                    spacing={0}
                    value={dayWindow}
                    onValueChange={(v) => v && setDayWindow(v as DayWindow)}
                  >
                    {DAY_WINDOWS.map((w) => (
                      <ToggleGroupItem key={w} value={w} className="h-7 px-2 text-[11px]">
                        {w === "all" ? "全部" : `${w} 天`}
                      </ToggleGroupItem>
                    ))}
                  </ToggleGroup>
                }
              >
                <OnlineDayChart days={windowDays} gap={gap} height={260} />
              </ChartCard>

              {/* 每日排行 */}
              <Card>
                <CardHeader>
                  <CardTitle className="text-sm">每日排行（{dayWindow === "all" ? "全部" : `近 ${dayWindow} 天`}，按在线时长）</CardTitle>
                </CardHeader>
                <CardContent className="max-h-[420px] overflow-auto px-0">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>日期</TableHead>
                        <TableHead className="text-right">在线</TableHead>
                        <TableHead className="text-right">段数</TableHead>
                        <TableHead className="text-right">对话</TableHead>
                        <TableHead className="text-right">模型生成</TableHead>
                        <TableHead className="text-right">工具执行</TableHead>
                        <TableHead className="text-right">会话</TableHead>
                        <TableHead className="text-right">Token</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {rankDays.filter((d) => (d.byGap[gap] ?? 0) > 0).slice(0, 30).map((d) => (
                        <TableRow key={d.d}>
                          <TableCell className="font-mono text-xs">{d.d}</TableCell>
                          <TableCell className="text-right font-mono font-semibold tabular-nums">{fmtDur(d.byGap[gap] ?? 0)}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{d.segByGap[gap] ?? 0}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{fmtDur(d.turnMs)}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{fmtDur(d.llmMs)}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{fmtDur(d.toolMs)}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{d.sessions}</TableCell>
                          <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{fmtTok(d.tokens)}</TableCell>
                        </TableRow>
                      ))}
                      {rankDays.every((d) => (d.byGap[gap] ?? 0) === 0) && (
                        <TableRow>
                          <TableCell colSpan={8} className="py-8 text-center text-sm text-muted-foreground">
                            该窗口内没有达到阈值的在线记录
                          </TableCell>
                        </TableRow>
                      )}
                    </TableBody>
                  </Table>
                </CardContent>
              </Card>

              {/* 口径对比 */}
              <Card>
                <CardHeader>
                  <CardTitle className="text-sm">五档阈值对比</CardTitle>
                  <CardDescription className="text-[11px]">
                    同一份日志，阈值只改变「静默期算不算在线」的口径：档位越大累计越多，但段数越少（碎片被并进大段）。
                  </CardDescription>
                </CardHeader>
                <CardContent className="overflow-x-auto px-0">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>空闲阈值</TableHead>
                        <TableHead className="text-right">累计在线</TableHead>
                        <TableHead className="text-right">段数</TableHead>
                        <TableHead className="text-right">活跃日均</TableHead>
                        <TableHead className="text-right">边际增量</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {on.gaps.map((g, i) => {
                        const t = on.totalMs[String(g)] ?? 0;
                        const prev = i > 0 ? on.totalMs[String(on.gaps[i - 1])] ?? 0 : 0;
                        return (
                          <TableRow key={g} className={String(g) === gap ? "bg-accent/60" : undefined}>
                            <TableCell className="text-xs">{g} 分钟{i > 0 ? `（+${g - on.gaps[i - 1]}）` : ""}</TableCell>
                            <TableCell className="text-right font-mono font-semibold tabular-nums">{fmtDur(t)}</TableCell>
                            <TableCell className="text-right font-mono tabular-nums text-muted-foreground">{on.segments[String(g)] ?? 0}</TableCell>
                            <TableCell className="text-right font-mono tabular-nums text-muted-foreground">
                              {fmtDur(on.activeDays > 0 ? t / on.activeDays : 0)}
                            </TableCell>
                            <TableCell className="text-right font-mono tabular-nums text-muted-foreground">
                              {i > 0 ? `+${fmtDur(t - prev)}` : "—"}
                            </TableCell>
                          </TableRow>
                        );
                      })}
                    </TableBody>
                  </Table>
                </CardContent>
              </Card>

              <div className="pb-2 text-center text-[11px] text-muted-foreground">
                数据更新于 {fmtClock(stats.generatedAt)} · 在线为估算下界，对话/模型/工具三口径为墙钟精确值
              </div>
            </>
          )}
        </>
      )}

      {/* 分享面板 */}
      <Dialog open={shareOpen} onOpenChange={setShareOpen}>
        <DialogContent className="flex max-h-[92vh] w-[min(1360px,96vw)] max-w-none sm:max-w-none flex-col gap-0 p-0">
          <DialogHeader className="border-b border-border px-4 py-3 text-left">
            <DialogTitle className="text-sm">分享卡预览</DialogTitle>
            <DialogDescription className="text-[11px]">
              整块板子按 2× 光栅化为 PNG；导出图与预览一致（含当前阈值 {gap} 分钟口径）。
            </DialogDescription>
          </DialogHeader>
          {stats && (
            <div className="min-h-0 flex-1 overflow-auto bg-muted/40 p-4">
              <ShareBoard ref={boardRef} stats={stats} identity={identity} appVersion={appVersion} gapMin={gapNum} />
            </div>
          )}
          <div className="flex items-center justify-end gap-2 border-t border-border px-4 py-3">
            <Button variant="outline" size="sm" disabled={exporting} onClick={() => void doExport("copy")}>
              {exporting ? <Loader2 className="animate-spin" /> : <Copy />} 复制图片
            </Button>
            <Button size="sm" disabled={exporting} onClick={() => void doExport("download")}>
              <Download /> 下载 PNG
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
