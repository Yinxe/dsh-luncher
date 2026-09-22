import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import {
  Activity,
  CircleOff,
  Cog,
  Copy,
  Cpu,
  Download,
  Gauge,
  Hourglass,
  Info,
  Loader2,
  MessageSquare,
  RefreshCw,
  Share2,
  Sun,
  Terminal,
  TrendingUp,
  Waves,
  type LucideIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Progress } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { api } from "../api";
import type { SessionStats, ShareIdentity } from "../types";
import { ShareBoard } from "./ShareBoard";
import { copyPng, downloadBlob, nodeToPngBlob } from "@/lib/share-export";
import { ModelUsageBoard, OnlineDayChart, StackModelChart, type StackPoint } from "./stats-charts";
import {
  fmtClock,
  fmtDur,
  fmtDurCompact,
  fmtNum,
  fmtTok,
  Heatmap,
  md,
  ModelLegend,
  SERIES_COLORS,
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

/** 口径徽章：精确=绿、估算=琥珀、下界=灰 */
const BADGES = {
  exact: { text: "精确", cls: "border-[var(--meter-1)]/45 text-[var(--meter-1)]" },
  est: { text: "估算", cls: "border-[var(--meter-3)]/55 text-[var(--meter-3)]" },
  lower: { text: "下界", cls: "border-border text-muted-foreground" },
} as const;

/** 在线时长 KPI 卡：图标 + 标签 + 口径徽章 + 大数字 + 副行，右下同名水印 */
function KpiCard({
  label,
  badge,
  value,
  sub,
  icon: Icon,
}: {
  label: string;
  badge?: keyof typeof BADGES;
  value: string;
  sub?: string;
  icon: LucideIcon;
}) {
  return (
    <Card className="relative overflow-hidden">
      <CardContent className="p-3.5">
        <div className="flex items-center gap-1.5">
          <Icon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
          <span className="truncate text-xs text-muted-foreground">{label}</span>
          {badge && (
            <Badge variant="outline" className={`h-4 shrink-0 px-1 text-[9px] ${BADGES[badge].cls}`}>
              {BADGES[badge].text}
            </Badge>
          )}
        </div>
        <div className="mt-1 truncate font-mono text-xl font-bold leading-tight tabular-nums" title={value}>
          {value}
        </div>
        {sub && (
          <div className="mt-0.5 truncate text-[11px] text-muted-foreground" title={sub}>
            {sub}
          </div>
        )}
      </CardContent>
      <Icon className="pointer-events-none absolute -bottom-3 -right-3 h-14 w-14 opacity-[0.06]" />
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
  const [heatMonths, setHeatMonths] = useState<"6" | "12">("6");

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
        <div className="ml-auto flex flex-wrap items-center gap-2">
          {tab === "sessions" && (
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
          )}
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
                <ChartCard
                  eyebrow="Activity"
                  title="Token 活动热力图（悬浮查看当日明细）"
                  actions={
                    <ToggleGroup
                      type="single"
                      variant="outline"
                      spacing={0}
                      value={heatMonths}
                      onValueChange={(v) => v && setHeatMonths(v as "6" | "12")}
                    >
                      <ToggleGroupItem value="6" className="h-7 px-2 text-[11px]">6 个月</ToggleGroupItem>
                      <ToggleGroupItem value="12" className="h-7 px-2 text-[11px]">12 个月</ToggleGroupItem>
                    </ToggleGroup>
                  }
                >
                  <Heatmap cells={stats.heatmap.slice(-(heatMonths === "6" ? 27 : 53) * 7)} />
                </ChartCard>
              </div>

              {/* 模型用量分布：环形图 + 响应式卡片网格 */}
              <ChartCard
                eyebrow="Models"
                title="模型用量分布（全部 · 悬浮查看构成）"
                desc="总 Token = 输入 + 缓存读 + 缓存写 + 输出（reasoning 已含在输出内）；fork/resume 种子事件已去重。"
              >
                <ModelUsageBoard models={stats.models} />
              </ChartCard>

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
              {/* 空闲阈值：规则 + 五档卡片（显示各档累计与相对当前档的增量）+ 时间范围 */}
              <Card>
                <CardHeader className="pb-2">
                  <CardTitle className="flex items-center gap-1.5 text-sm">
                    <Gauge className="h-4 w-4 text-muted-foreground" /> 空闲阈值
                  </CardTitle>
                </CardHeader>
                <CardContent className="space-y-3 text-xs">
                  <p className="leading-5">
                    在线 = 日志里有事件、且相邻事件间隔不超过阈值的那段墙钟时间。规则就三条：
                  </p>
                  <ul className="list-disc space-y-1 pl-5 leading-5 text-muted-foreground">
                    <li>
                      <b className="text-foreground">间隔 ≤ 阈值 → 整段算在线</b>：10:00 与 10:50 各有一个事件、阈值
                      60 分钟，中间这 50 分钟（哪怕你不在）一并计入，并累加到当天。
                    </li>
                    <li>
                      <b className="text-foreground">间隔 &gt; 阈值 → 断开</b>：从上一个事件处收尾，中间那段一秒都不计，
                      新的一段从下一个事件重新起算。
                    </li>
                    <li>
                      <b className="text-foreground">每段只算到最后一个事件</b>：之后的时间不计（哪怕过了 1
                      分钟就关窗口，或者你接着又跑了 3 小时没产生事件）。所以任何档位算出来都是<b>下界</b>，
                      不是「坐在电脑前」的时长。
                    </li>
                  </ul>
                  <div className="text-muted-foreground">同一份日志下，五档分别是多少（点档位可切换）：</div>
                  <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-5">
                    {on.gaps.map((g) => {
                      const gs = String(g);
                      const t = on.totalMs[gs] ?? 0;
                      const sel = gs === gap;
                      const rec = gs === String(on.defaultGapMin);
                      const delta = t - (on.totalMs[gap] ?? 0);
                      return (
                        <button
                          key={g}
                          type="button"
                          disabled={busy}
                          onClick={() => setGap(gs)}
                          className={`rounded-lg border px-3 py-2 text-left transition-colors hover:bg-accent/50 ${
                            sel ? "border-primary/60 bg-accent/40" : "border-border/70"
                          }`}
                        >
                          <div className="text-[11px] text-muted-foreground">
                            {g} 分钟{rec && <span className="text-[var(--meter-1)]"> · 推荐</span>}
                          </div>
                          <div className={`mt-0.5 font-mono text-base font-bold tabular-nums ${sel ? "text-primary" : ""}`}>
                            {fmtDurCompact(t)}
                          </div>
                          <div className="mt-0.5 h-[14px] text-[10px] leading-none text-muted-foreground">
                            {!sel && delta !== 0 && `${delta > 0 ? "+" : "-"}${fmtDurCompact(Math.abs(delta))}`}
                          </div>
                        </button>
                      );
                    })}
                  </div>
                  <p className="leading-5 text-muted-foreground">
                    <b className="text-[var(--meter-1)]">推荐 15 分钟</b>（插件默认值）：DSH 真正在干活时日志里是有事件的
                    （模型 step、工具 call/result、子代理），不需要靠大阈值来兜；需要兜的是读长回答、想下一个需求这类静默期，
                    通常几分钟量级。5 分钟以下会把「读完回答再想一下」也切断，偏低；60 分钟会把「去开会/吃饭」整段算成在线，
                    只适合回答「今天开着 DSH 多久」。对照上面五档的增量，多出来的小时主要来自哪一档，一眼能看出来。
                  </p>
                  <div className="flex flex-wrap items-center gap-2 border-t border-border/60 pt-3">
                    <span className="text-muted-foreground">时间范围</span>
                    <ToggleGroup
                      type="single"
                      variant="outline"
                      spacing={0}
                      value={dayWindow}
                      onValueChange={(v) => v && setDayWindow(v as DayWindow)}
                    >
                      {DAY_WINDOWS.map((w) => (
                        <ToggleGroupItem key={w} value={w} className="h-7 px-2.5 text-[11px]">
                          {w === "all" ? "全部" : `近 ${w} 天`}
                        </ToggleGroupItem>
                      ))}
                    </ToggleGroup>
                    <span className="text-[11px] text-muted-foreground">
                      「每日在线」图表与下方「在线最多的日子」明细的统计窗口；上面的累计/日均始终按全部活跃日计算。
                    </span>
                  </div>
                  <div className="text-[11px] text-muted-foreground">
                    {on.activeDays} 个活跃日 · 默认按{dayWindow === "all" ? "全部" : `近 ${dayWindow} 天`}显示
                  </div>
                </CardContent>
              </Card>

              {/* 三口径 + 派生指标：带口径徽章与水印图标 */}
              <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-4">
                <KpiCard label="今日在线" badge="lower" icon={Sun} value={fmtDur(todayDay?.byGap[gap] ?? 0)} sub="截至此刻" />
                <KpiCard
                  label="累计在线"
                  badge="lower"
                  icon={Hourglass}
                  value={fmtDur(on.totalMs[gap] ?? 0)}
                  sub={`${on.firstDay ? `${on.firstDay.slice(5).replace("-", "/")} 起 · ` : ""}共 ${on.activeDays} 个活跃日 · ${on.segments[gap] ?? 0} 段`}
                />
                <KpiCard
                  label="活跃日均"
                  icon={Waves}
                  value={fmtDur(on.activeDays > 0 ? (on.totalMs[gap] ?? 0) / on.activeDays : 0)}
                  sub="仅按有活动的日子平均"
                />
                <KpiCard
                  label="对话进行中"
                  badge="est"
                  icon={MessageSquare}
                  value={fmtDur(on.turnMs)}
                  sub={`占在线 ${on.totalMs[gap] ? Math.round((on.turnMs / on.totalMs[gap]!) * 100) : 0}%`}
                />
                <KpiCard label="模型生成" badge="exact" icon={Cpu} value={fmtDur(on.llmMs)} sub="已与官方投影对账" />
                <KpiCard label="工具执行" badge="exact" icon={Terminal} value={fmtDur(on.toolMs)} sub="call→result 按调用配对" />
                <KpiCard
                  label="引擎合计"
                  badge="exact"
                  icon={Cog}
                  value={fmtDur(on.llmMs + on.toolMs)}
                  sub="模型 + 工具相加，可高于墙钟"
                />
                <KpiCard
                  label="连续使用"
                  icon={TrendingUp}
                  value={`${ov!.currentStreak} 天`}
                  sub={`最长 ${ov!.longestStreak} 天`}
                />
              </div>

              {/* 每日在线 */}
              <ChartCard
                eyebrow="Daily"
                title={`每日在线（阈值 ${gap} 分钟 · ${dayWindow === "all" ? "全部" : `近 ${dayWindow} 天`}）`}
                desc="柱=在线时长（阈值口径下界），线=对话进行中；跨零点活动段按本地日切分。"
              >
                <OnlineDayChart days={windowDays} gap={gap} height={260} />
              </ChartCard>

              {/* 每日排行 */}
              <Card>
                <CardHeader>
                  <CardTitle className="text-sm">在线最多的日子（{dayWindow === "all" ? "全部" : `近 ${dayWindow} 天`}，按在线时长）</CardTitle>
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

              {/* 口径与准确性：三条墙钟对比 + 指标性质表 */}
              <Card>
                <CardHeader className="pb-2">
                  <CardTitle className="flex items-center gap-1.5 text-sm">
                    <Info className="h-4 w-4 text-muted-foreground" /> 口径与准确性
                  </CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                  <div className="space-y-2.5">
                    {(
                      [
                        ["在线（你 + DSH）", "含你自己的追问/阅读空档", on.totalMs[gap] ?? 0, "var(--meter-5)"],
                        ["对话进行中（DSH 的钟）", "turn 区间并集，墙钟去重", on.turnMs, "var(--meter-6)"],
                        ["模型 + 工具（DSH 的活）", "并行会话相加，可高于墙钟", on.llmMs + on.toolMs, "var(--meter-3)"],
                      ] as const
                    ).map(([label, desc, ms, color]) => (
                      <div key={label} className="flex flex-wrap items-center gap-x-4 gap-y-1">
                        <div className="w-[190px] shrink-0">
                          <div className="text-xs font-medium">{label}</div>
                          <div className="text-[10px] text-muted-foreground">{desc}</div>
                        </div>
                        <Progress
                          value={(ms / Math.max(1, on.totalMs[gap] ?? 1)) * 100}
                          className="h-1.5 min-w-[120px] flex-1"
                          style={{ "--primary": color } as CSSProperties}
                        />
                        <div className="w-[84px] shrink-0 text-right font-mono text-xs font-semibold tabular-nums">
                          {fmtDurCompact(ms)}
                        </div>
                      </div>
                    ))}
                  </div>
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead className="w-[92px]">指标</TableHead>
                        <TableHead className="w-[56px]">性质</TableHead>
                        <TableHead className="w-[92px] text-right">当前值</TableHead>
                        <TableHead>说明</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {(
                        [
                          ["在线时长", "lower", fmtDurCompact(on.totalMs[gap] ?? 0), `日志只在有事件时打点：窗口开着但没事件的时间不可见，所以是下界；阈值（当前 ${gap} 分钟）直接决定结果`],
                          ["对话进行中", "est", fmtDurCompact(on.turnMs), "turn/start→turn/end 并集（墙钟去重）；含少量等你操作的时间"],
                          ["模型生成", "exact", fmtDurCompact(on.llmMs), "step/start→assistant/message；与 DSH 自带 sessionStats 投影逐会话对账一致"],
                          ["工具执行", "exact", fmtDurCompact(on.toolMs), "tool/call→tool/result 按 callId 配对"],
                          ["引擎合计", "exact", fmtDurCompact(on.llmMs + on.toolMs), "上两者相加；同时开多个会话/子代理会重复计，所以可能大于墙钟"],
                          ["活跃天数", "exact", String(on.activeDays), "有事件或有用水量的自然日数"],
                          ["Token 用量", "exact", fmtTok(ov!.totalTokens), "当前区间的上报值；只统计供应商给了 usage 的步骤，未上报的算不到，因此略低"],
                          ["会话口径", "exact", `${stats.sessionsWithUsage} / ${stats.sessionsTotal}`, `共 ${stats.sessionsTotal} 个会话：有用量 ${stats.sessionsWithUsage} 个；其余为继承空壳、从未发起请求或供应商没上报 usage`],
                        ] as const
                      ).map(([label, badge, value, desc]) => (
                        <TableRow key={label}>
                          <TableCell className="text-xs">{label}</TableCell>
                          <TableCell>
                            <Badge variant="outline" className={`h-4 px-1 text-[9px] ${BADGES[badge].cls}`}>
                              {BADGES[badge].text}
                            </Badge>
                          </TableCell>
                          <TableCell className="text-right font-mono text-xs font-semibold tabular-nums">{value}</TableCell>
                          <TableCell className="text-[11px] leading-4 text-muted-foreground">{desc}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                  <div className="border-t border-border/60 pt-2 text-[11px] text-muted-foreground">
                    数据源 {stats.sessionsTotal}/{stats.sessionsTotal} 个会话已扫描 · 缓存
                    ~/.dsh-starter/session-stats-cache.json · fork/接续的继承前缀不重复计时
                  </div>
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
