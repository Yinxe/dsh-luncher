import type { ReactNode } from "react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  ComposedChart,
  Line,
  XAxis,
  YAxis,
} from "recharts";
import { ChartContainer, ChartTooltip } from "@/components/ui/chart";
import type { ModelTokens, ModelUsage, OnlineDay } from "../types";
import { fmtDur, fmtHourAxis, fmtTok, fmtTokAxis, md, OTHER_COLOR, SERIES_COLORS } from "./stats-parts";

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

function RankTip({ active, payload }: { active?: boolean; payload?: TipEntry[] }) {
  if (!active || !payload || payload.length === 0) return null;
  const row = payload[0].payload as unknown as { full: string; v: number; share: number; calls: number } | undefined;
  if (!row) return null;
  return (
    <TipBox
      title={<span className="font-mono">{row.full}</span>}
      rows={[
        { k: "Token", v: fmtTok(row.v) },
        { k: "占比", v: `${(row.share * 100).toFixed(1)}%` },
        { k: "请求", v: row.calls.toLocaleString() },
      ]}
    />
  );
}

/** 模型排行横向条（Top N，取色与堆叠图一致） */
export function ModelRankChart({ models, topN = 8, disableAnimation }: { models: ModelUsage[]; topN?: number; disableAnimation?: boolean }) {
  const top = models.slice(0, topN);
  if (top.length === 0) return null;
  // 分类轴自下而上渲染，反转让第一名落在顶部
  const data = top
    .map((m, i) => ({
      y: m.model.length > 17 ? `${m.model.slice(0, 16)}…` : m.model,
      full: m.model,
      v: m.tokens,
      share: m.share,
      calls: m.calls,
      c: SERIES_COLORS[i] ?? OTHER_COLOR,
    }))
    .reverse();
  return (
    <ChartContainer config={{}} className="aspect-auto w-full" style={{ height: top.length * 30 + 12 }}>
      <BarChart layout="vertical" data={data} margin={{ top: 2, right: 12, bottom: 2, left: 0 }}>
        <XAxis type="number" hide />
        <YAxis
          type="category"
          dataKey="y"
          width={128}
          tickLine={false}
          axisLine={false}
          tick={{ ...AXIS_TICK, fontFamily: "var(--font-mono)" }}
        />
        <ChartTooltip content={<RankTip />} />
        <Bar dataKey="v" radius={[0, 4, 4, 0]} maxBarSize={18} isAnimationActive={!disableAnimation}>
          {data.map((d, i) => (
            <Cell key={i} fill={d.c} />
          ))}
        </Bar>
      </BarChart>
    </ChartContainer>
  );
}
