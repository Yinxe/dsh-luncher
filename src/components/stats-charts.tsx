import { useMemo, useRef, useState, type ReactNode } from "react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  ComposedChart,
  Line,
  Pie,
  PieChart,
  Sector,
  XAxis,
  YAxis,
} from "recharts";
import { ChartContainer, ChartTooltip } from "@/components/ui/chart";
import type { ModelTokens, ModelUsage, OnlineDay } from "../types";
import {
  fmtDur,
  fmtHourAxis,
  fmtNum,
  fmtTok,
  fmtTokAxis,
  FloatTip,
  md,
  OTHER_COLOR,
  SERIES_COLORS,
  tipAnchorRect,
} from "./stats-parts";

/**
 * 统计图表族：全部基于 recharts（经 shadcn ChartContainer 接入主题），
 * 序列色走 CSS 变量（--meter-*），明暗主题与分享卡导出共用同一套色板。
 * 分享卡光栅化时传 disableAnimation，保证逐帧确定性。
 */

const AXIS_TICK = { fontSize: 10, fill: "var(--muted-foreground)" } as const;
const GRID = { vertical: false, strokeDasharray: "2 6" } as const;

/** 按模型堆叠的柱图数据点（趋势 / 今日 24h / 分享卡共用） */
export interface StackPoint {
  /** X 轴短标签（MM-DD 或小时） */
  label: string;
  /** 提示框标题（完整日期 / 时段描述） */
  full: string;
  parts: ModelTokens[];
  total: number;
  calls?: number;
}

interface StackRow {
  label: string;
  full: string;
  total: number;
  calls: number;
  [key: string]: string | number | undefined;
}

interface TipEntry {
  dataKey?: string | number;
  name?: ReactNode;
  value?: number | string;
  color?: string;
  payload?: StackRow;
}

function TipBox({ title, rows, foot }: { title: ReactNode; rows: Array<{ c?: string; k: ReactNode; v: ReactNode }>; foot?: ReactNode }) {
  return (
    <div className="min-w-[170px] rounded-lg border border-border bg-popover px-3 py-2 text-xs shadow-md">
      <div className="mb-1 font-medium">{title}</div>
      {rows.map((r, i) => (
        <div key={i} className="flex items-center justify-between gap-4">
          <span className="flex min-w-0 items-center gap-1.5">
            {r.c !== undefined && <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: r.c }} />}
            <span className="max-w-[180px] truncate text-muted-foreground" title={typeof r.k === "string" ? r.k : undefined}>
              {r.k}
            </span>
          </span>
          <span className="font-mono tabular-nums">{r.v}</span>
        </div>
      ))}
      {foot && <div className="mt-1 border-t border-border/60 pt-1 text-[10px] text-muted-foreground">{foot}</div>}
    </div>
  );
}

function StackTip({ active, payload }: { active?: boolean; payload?: TipEntry[] }) {
  if (!active || !payload || payload.length === 0) return null;
  const row = payload[0].payload;
  if (!row) return null;
  return (
    <TipBox
      title={row.full}
      rows={[
        { k: "合计", v: `${fmtTok(row.total)} tokens${row.calls ? ` · ${row.calls} 次请求` : ""}` },
        ...payload
          .filter((e) => Number(e.value) > 0)
          .map((e) => ({ c: e.color, k: e.name ?? String(e.dataKey), v: fmtTok(Number(e.value)) })),
      ]}
    />
  );
}

/** 按日/按小时堆叠 Token（堆叠段=模型，Top6 之外并入「其他」） */
export function StackModelChart({
  points,
  models,
  className,
  height = 280,
  disableAnimation,
  emptyHint = "该时间段内没有记录",
}: {
  points: StackPoint[];
  /** 全历史 Token 排名前 6 的模型名（决定取色顺序），其余归「其他」 */
  models: string[];
  className?: string;
  height?: number;
  disableAnimation?: boolean;
  emptyHint?: string;
}) {
  const series = models.slice(0, SERIES_COLORS.length);
  const idx = new Map(series.map((m, i) => [m, i]));
  const data: StackRow[] = points.map((p) => {
    const row: StackRow = { label: p.label, full: p.full, total: p.total, calls: p.calls ?? 0 };
    const per: number[] = [];
    let other = 0;
    for (const t of p.parts) {
      const i = idx.get(t.model);
      if (i === undefined) other += t.tokens;
      else per[i] = (per[i] ?? 0) + t.tokens;
    }
    series.forEach((_m, i) => {
      const v = per[i];
      if (v) row[`s${i}`] = v;
    });
    if (other) row.other = other;
    return row;
  });
  const hasOther = data.some((r) => r.other !== undefined);
  // 顶段才有圆角：按声明顺序最后一个是顶段
  const topKey = hasOther ? "other" : series.length > 0 ? `s${series.length - 1}` : null;
  const anim = !disableAnimation;
  const config = {};
  const bar = (key: string, name: string, color: string, isTop: boolean) => (
    <Bar
      key={key}
      dataKey={key}
      name={name}
      stackId="tok"
      fill={color}
      radius={isTop ? [4, 4, 0, 0] : 0}
      stroke="var(--card)"
      strokeWidth={1}
      maxBarSize={46}
      isAnimationActive={anim}
    />
  );

  return (
    <div className="relative">
      <ChartContainer config={config} className={`aspect-auto w-full ${className ?? ""}`} style={{ height }}>
        <BarChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: -6 }}>
          <CartesianGrid {...GRID} className="stroke-border/60" />
          <XAxis
            dataKey="label"
            tickLine={false}
            axisLine={false}
            tick={AXIS_TICK}
            interval="preserveStartEnd"
            minTickGap={24}
          />
          <YAxis tickLine={false} axisLine={false} tick={AXIS_TICK} tickFormatter={(v) => fmtTokAxis(Number(v))} width={44} />
          <ChartTooltip content={<StackTip />} />
          {series.map((m, i) => bar(`s${i}`, m, SERIES_COLORS[i], !hasOther && i === series.length - 1 && series.length > 0))}
          {hasOther && bar("other", "其他", OTHER_COLOR, topKey === "other")}
        </BarChart>
      </ChartContainer>
      {points.every((p) => p.total === 0) && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center text-xs text-muted-foreground">
          {emptyHint}
        </div>
      )}
    </div>
  );
}

function OnlineTip({ active, payload, gap }: { active?: boolean; payload?: TipEntry[]; gap: string }) {
  if (!active || !payload || payload.length === 0) return null;
  const row = payload[0].payload as unknown as { full: string; online: number; turn: number; segs: number; sessions: number } | undefined;
  if (!row) return null;
  return (
    <TipBox
      title={row.full}
      rows={[
        { c: SERIES_COLORS[1], k: `在线（${gap} 分档）`, v: fmtDur(row.online) },
        { c: SERIES_COLORS[2], k: "对话进行中", v: fmtDur(row.turn) },
      ]}
      foot={`${row.segs} 段 · ${row.sessions} 个会话`}
    />
  );
}

/** 每日在线（阈值口径）柱 + 对话进行中折线 */
export function OnlineDayChart({
  days,
  gap,
  height = 260,
  disableAnimation,
  emptyHint = "该窗口内没有在线记录",
}: {
  days: OnlineDay[];
  gap: string;
  height?: number;
  disableAnimation?: boolean;
  emptyHint?: string;
}) {
  const data = days.map((d) => ({
    label: md(d.d),
    full: d.d,
    online: d.byGap[gap] ?? 0,
    turn: d.turnMs,
    segs: d.segByGap[gap] ?? 0,
    sessions: d.sessions,
  }));
  const anim = !disableAnimation;
  return (
    <div className="relative">
      <ChartContainer config={{}} className="aspect-auto w-full" style={{ height }}>
        <ComposedChart data={data} margin={{ top: 8, right: 8, bottom: 0, left: -6 }}>
          <CartesianGrid {...GRID} className="stroke-border/60" />
          <XAxis dataKey="label" tickLine={false} axisLine={false} tick={AXIS_TICK} interval="preserveStartEnd" minTickGap={24} />
          <YAxis tickLine={false} axisLine={false} tick={AXIS_TICK} tickFormatter={(v) => fmtHourAxis(Number(v))} width={40} />
          <ChartTooltip content={<OnlineTip gap={gap} />} />
          <Bar dataKey="online" name="在线" fill={SERIES_COLORS[1]} radius={[3, 3, 0, 0]} maxBarSize={26} isAnimationActive={anim} />
          <Line type="monotone" dataKey="turn" name="对话" stroke={SERIES_COLORS[2]} strokeWidth={1.5} dot={false} isAnimationActive={anim} />
        </ComposedChart>
      </ChartContainer>
      {data.every((d) => d.online === 0) && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center text-xs text-muted-foreground">
          {emptyHint}
        </div>
      )}
    </div>
  );
}

interface ModelTip {
  m: ModelUsage;
  x: number;
  y: number;
}

/** "provider/model" → provider / model（无前缀时 provider 显示为 —） */
const provOf = (model: string) => {
  const i = model.indexOf("/");
  return i > 0 ? model.slice(0, i) : "—";
};
const shortOf = (model: string) => {
  const i = model.indexOf("/");
  return i > 0 ? model.slice(i + 1) : model;
};

/** 环图切片（top6 + 其他）；full 保留完整模型 id 给 tooltip 的 title */
interface DonutDatum {
  name: string;
  full: string;
  value: number;
  color: string;
  pct: number;
}

/**
 * 模型用量分布：环形图（中心=累计 Token）+ 响应式卡片网格，悬浮卡片看用量构成。
 * 环图悬浮用跟随鼠标的 FloatTip：recharts 自带 Tooltip 会被 150px 的容器夹住位置、
 * 压在中心数字上，既不动也读不清。
 */
export function ModelUsageBoard({ models, disableAnimation }: { models: ModelUsage[]; disableAnimation?: boolean }) {
  const [tip, setTip] = useState<ModelTip | null>(null);
  const [dtip, setDtip] = useState<{ d: DonutDatum; x: number; y: number } | null>(null);
  // mousemove 每个事件都 setState 会让整块面板（含卡片网格与全部 Sector）逐像素重渲染；限流到 ~20fps
  const dtipTs = useRef(0);
  const total = models.reduce((a, m) => a + m.tokens, 0);
  /** 同名模型常来自不同供应商（列表里会重复出现），重名时给显示名补上供应商标识 */
  const labelOf = useMemo(() => {
    const cnt = new Map<string, number>();
    for (const m of models) {
      const s = shortOf(m.model);
      cnt.set(s, (cnt.get(s) ?? 0) + 1);
    }
    return (model: string) => {
      const s = shortOf(model);
      return (cnt.get(s) ?? 0) > 1 ? `${s} · ${provOf(model)}` : s;
    };
  }, [models]);
  if (models.length === 0) return <div className="py-8 text-center text-sm text-muted-foreground">还没有任何模型用量记录</div>;
  const top = models.slice(0, SERIES_COLORS.length);
  const otherTokens = models.slice(top.length).reduce((a, m) => a + m.tokens, 0);
  const pieData: DonutDatum[] = [
    ...top.map((m, i) => ({ name: labelOf(m.model), full: m.model, value: m.tokens, color: SERIES_COLORS[i], pct: m.share * 100 })),
    ...(otherTokens > 0
      ? [{ name: "其他", full: `其余 ${models.length - top.length} 个模型合计`, value: otherTokens, color: OTHER_COLOR, pct: (otherTokens / Math.max(1, total)) * 100 }]
      : []),
  ];
  const colorOf = (i: number) => (i < SERIES_COLORS.length ? SERIES_COLORS[i] : OTHER_COLOR);
  return (
    <div className="flex flex-col gap-5 sm:flex-row sm:items-start">
      <div className="relative mx-auto w-[150px] shrink-0 sm:mx-0">
        <ChartContainer config={{}} className="aspect-auto w-full" style={{ height: 150 }}>
          <PieChart>
            <Pie
              data={pieData}
              dataKey="value"
              nameKey="name"
              innerRadius="64%"
              outerRadius="88%"
              paddingAngle={pieData.length > 1 ? 2 : 0}
              stroke="var(--card)"
              startAngle={90}
              endAngle={-270}
              isAnimationActive={!disableAnimation}
              // 悬浮段外扩 4px：环图唯一的即时触感反馈（v3 用 shape + isActive，activeShape 已废弃）
              shape={(p) => (
                <Sector
                  cx={p.cx}
                  cy={p.cy}
                  innerRadius={p.innerRadius}
                  outerRadius={p.outerRadius + (p.isActive ? 4 : 0)}
                  startAngle={p.startAngle}
                  endAngle={p.endAngle}
                  cornerRadius={p.cornerRadius}
                  fill={p.fill ?? p.payload?.color}
                  stroke="var(--card)"
                  strokeWidth={1}
                />
              )}
              onMouseMove={(_d, i, e) => {
                const now = performance.now();
                if (now - dtipTs.current < 50) return;
                dtipTs.current = now;
                setDtip({ d: pieData[i], x: e.clientX, y: e.clientY });
              }}
              onMouseLeave={() => setDtip(null)}
            >
              {pieData.map((d, i) => (
                <Cell key={i} fill={d.color} />
              ))}
            </Pie>
          </PieChart>
        </ChartContainer>
        <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center">
          <div className="font-mono text-lg font-bold leading-tight tabular-nums">{fmtTok(total)}</div>
          <div className="text-[10px] text-muted-foreground">累计 Token</div>
        </div>
      </div>
      <div
        className="grid min-w-0 flex-1 content-start gap-x-5 gap-y-2.5 sm:grid-cols-2 xl:grid-cols-3"
        onMouseLeave={() => setTip(null)}
      >
        {models.map((m, i) => {
          const c = colorOf(i);
          return (
            <div
              key={m.model}
              className="min-w-0"
              onMouseEnter={(e) => {
                const a = tipAnchorRect(e.currentTarget.getBoundingClientRect(), 170);
                setTip({ m, ...a });
              }}
            >
              <div className="flex items-center gap-1.5 text-xs">
                <span className="h-2 w-2 shrink-0 rounded-[3px]" style={{ background: c }} />
                <span className="min-w-0 truncate font-medium" title={m.model}>{labelOf(m.model)}</span>
                <span className="ml-auto shrink-0 font-mono text-[11px] tabular-nums text-muted-foreground">
                  {fmtTok(m.tokens)} · {(m.share * 100).toFixed(1)}%
                </span>
              </div>
              <div className="mt-1 h-1 w-full overflow-hidden rounded-full bg-muted">
                <div className="h-full rounded-full" style={{ width: `${Math.max(1.5, m.share * 100)}%`, background: c }} />
              </div>
              <div className="mt-1 truncate text-[10px] text-muted-foreground" title={`${provOf(m.model)} · ${m.calls} 次请求`}>
                {provOf(m.model)} · 输入 {fmtTok(m.input)} · 输出 {fmtTok(m.output)} · 缓存 {fmtTok(m.cacheRead + m.cacheWrite)} ·{" "}
                {fmtNum(m.calls)} 次
              </div>
            </div>
          );
        })}
      </div>
      {dtip && (
        <FloatTip
          x={Math.min(dtip.x + 14, Math.max(8, window.innerWidth - 268))}
          y={Math.min(dtip.y + 12, Math.max(8, window.innerHeight - 104))}
        >
          <div className="flex items-center gap-1.5 font-medium">
            <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: dtip.d.color }} />
            <span className="min-w-0 truncate" title={dtip.d.full}>
              {dtip.d.name}
            </span>
          </div>
          <div className="mt-1 flex items-baseline justify-between gap-4">
            <span className="text-muted-foreground">Token</span>
            <span className="font-mono tabular-nums">{fmtTok(dtip.d.value)}</span>
          </div>
          <div className="flex items-baseline justify-between gap-4">
            <span className="text-muted-foreground">占比</span>
            <span className="font-mono tabular-nums">{dtip.d.pct.toFixed(1)}%</span>
          </div>
        </FloatTip>
      )}
      {tip && (
        <FloatTip x={tip.x} y={tip.y}>
          <div className="truncate font-medium" title={tip.m.model}>
            {tip.m.model} 用量构成
          </div>
          {([
            ["输入", tip.m.input],
            ["输出", tip.m.output],
            ["缓存读", tip.m.cacheRead],
            ["缓存写", tip.m.cacheWrite],
          ] as const).map(([k, v]) => (
            <div key={k} className="flex items-baseline justify-between gap-4 text-[11px]">
              <span className="text-muted-foreground">{k}</span>
              <span className="font-mono tabular-nums">
                {fmtNum(v)} · {tip.m.tokens ? ((v / tip.m.tokens) * 100).toFixed(1) : "0.0"}%
              </span>
            </div>
          ))}
          <div className="mt-1 border-t border-border/60 pt-1 text-[10px] text-muted-foreground">
            共 {fmtNum(tip.m.tokens)} tokens · {fmtNum(tip.m.calls)} 次请求
          </div>
        </FloatTip>
      )}
    </div>
  );
}
