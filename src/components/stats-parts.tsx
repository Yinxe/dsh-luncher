import { Fragment, useMemo, useState, type CSSProperties, type ReactNode } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import type { DayCell } from "../types";

/** 统计图序列色：CSS 变量随明暗主题切换（定义见 src/index.css） */
export const SERIES_COLORS = [
  "var(--meter-1)",
  "var(--meter-2)",
  "var(--meter-3)",
  "var(--meter-4)",
  "var(--meter-5)",
  "var(--meter-6)",
];
export const OTHER_COLOR = "var(--meter-other)";
export const WEEKDAYS = ["一", "二", "三", "四", "五", "六", "日"];

export function fmtTok(n: number): string {
  if (n >= 1e8) return `${(n / 1e8).toFixed(2)} 亿`;
  if (n >= 1e4) return `${(n / 1e4).toFixed(1)} 万`;
  return n.toLocaleString();
}

export function fmtNum(n: number): string {
  return n.toLocaleString();
}

/** YYYY-MM-DD → MM-DD（图表轴标签） */
export function md(day: string): string {
  return day.length >= 10 ? day.slice(5) : day;
}

export function fmtClock(ms: number): string {
  return new Date(ms).toLocaleTimeString("zh-CN", { hour12: false });
}

/** 毫秒 → 人话时长：8 小时 12 分 / 45 分 / 32 秒（一律用小时计，不折算成天） */
export function fmtDur(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s} 秒`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} 分`;
  const h = Math.floor(m / 60);
  return `${h} 小时 ${m % 60} 分`;
}

/** 毫秒 → 紧凑时长（阈值档位/口径表这类窄空间）：234h12m / 7m / 45s */
export function fmtDurCompact(ms: number): string {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  return `${h}h${String(m % 60).padStart(2, "0")}m`;
}

/** 图表 Y 轴的空间敏感短格式：1.2万 / 3.4亿 / 6h */
export function fmtTokAxis(n: number): string {
  if (n >= 1e8) return `${(n / 1e8).toFixed(1)}亿`;
  if (n >= 1e4) return `${Math.round(n / 1e4)}万`;
  if (n >= 1e3) return `${Math.round(n / 1e3)}k`;
  return String(n);
}

export function fmtHourAxis(ms: number): string {
  const h = ms / 3_600_000;
  return h >= 10 ? `${Math.round(h)}h` : `${h.toFixed(h < 1 ? 1 : 0)}h`;
}

/** 顶栏统计卡：eyebrow 标签 + 等宽数字，明暗两档都保持“数据终端”的排印感 */
export function StatCard({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <Card className="group/card relative overflow-hidden transition-shadow hover:shadow-[var(--card-shadow)]">
      <CardContent className="p-3.5">
        <div className="text-[10px] font-medium uppercase tracking-[0.14em] text-[var(--eyebrow)]">{label}</div>
        <div className="mt-1 truncate font-mono text-xl font-bold leading-tight tabular-nums" title={value}>
          {value}
        </div>
        {sub && (
          <div className="mt-0.5 truncate text-[11px] text-muted-foreground" title={sub}>
            {sub}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

/** 模型分布表里的小占比条（指示条颜色借 --primary 变量覆盖 bg-primary） */
export function ShareBar({ share, color }: { share: number; color?: string }) {
  return (
    <div className="flex items-center gap-2">
      <Progress
        value={Math.min(100, share * 100)}
        className="h-1.5"
        style={color ? ({ "--primary": color } as CSSProperties) : undefined}
      />
      <span className="w-12 shrink-0 text-right font-mono text-[11px] tabular-nums text-muted-foreground">
        {(share * 100).toFixed(1)}%
      </span>
    </div>
  );
}

export function ModelLegend({ models, otherTokens }: { models: Array<{ model: string }>; otherTokens: number }) {
  if (models.length === 0) return null;
  return (
    <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
      {models.map((m, i) => (
        <span key={m.model} className="flex items-center gap-1.5" title={m.model}>
          <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: SERIES_COLORS[i] }} />
          <span className="max-w-[160px] truncate font-mono">{m.model}</span>
        </span>
      ))}
      {otherTokens > 0 && (
        <span className="flex items-center gap-1.5">
          <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: OTHER_COLOR }} /> 其他
        </span>
      )}
    </div>
  );
}

/** 悬浮明细浮层：固定在鼠标锚点旁（右列放不下时翻到左侧），不随滚动漂移 */
export function FloatTip({ x, y, children }: { x: number; y: number; children: ReactNode }) {
  return (
    <div
      className="pointer-events-none fixed z-50 w-[260px] rounded-lg border border-border bg-popover px-3 py-2 text-xs shadow-lg"
      style={{ left: x, top: y }}
    >
      {children}
    </div>
  );
}

/** 把锚点矩形换算成浮层坐标：默认贴在元素右侧，放不下就翻到左侧 */
export function tipAnchorRect(rect: DOMRect, estH = 190): { x: number; y: number } {
  const W = 260;
  let x = rect.right + 8;
  if (x + W > window.innerWidth - 8) x = Math.max(8, rect.left - W - 8);
  const y = Math.min(Math.max(8, rect.top - 8), Math.max(8, window.innerHeight - estH - 8));
  return { x, y };
}

/** 明细里的迷你进度条（缓存读/写、Top 模型占比） */
export function MiniBar({ pct, color }: { pct: number; color: string }) {
  return (
    <div className="h-1 min-w-[40px] flex-1 overflow-hidden rounded-full bg-muted">
      <div className="h-full rounded-full" style={{ width: `${Math.min(100, Math.max(2, pct * 100))}%`, background: color }} />
    </div>
  );
}

/**
 * Token 活动热力图：列=周、行=星期，色阶按四分位分 4 档（--heat-* 绿阶，有用量的日子最低也是 1 档）。
 * 纯 CSS 网格自适应容器宽度（不横向溢出）；悬浮出当日明细浮层；底部缓存汇总行。
 */
export function Heatmap({ cells }: { cells: DayCell[] }) {
  const [tip, setTip] = useState<{ cell: DayCell; x: number; y: number } | null>(null);
  const { weeks, levels, monthMarks } = useMemo(() => {
    const nz = cells.filter((c) => c.tokens > 0).map((c) => c.tokens).sort((a, b) => a - b);
    const q = (f: number) => (nz.length === 0 ? Infinity : nz[Math.min(nz.length - 1, Math.floor(nz.length * f))]);
    const levels = [q(0.25), q(0.5), q(0.75)];
    // 首日向前补空格，让每列对齐星期一→星期日（getDay 0=周日，换成 0=周一）
    const lead = cells.length === 0 ? 0 : (new Date(`${cells[0].day}T00:00:00`).getDay() + 6) % 7;
    const pad: Array<(typeof cells)[number] | null> = Array.from({ length: lead }, () => null);
    const flat = [...pad, ...cells];
    const weeks: Array<Array<(typeof cells)[number] | null>> = [];
    for (let i = 0; i < flat.length; i += 7) weeks.push(flat.slice(i, i + 7));
    // 月首所在列打刻度（跨月即打标，重复月名允许出现）
    const monthMarks: Array<{ col: number; label: string }> = [];
    let prevMon = "";
    weeks.forEach((wk, i) => {
      const first = wk.find((c): c is DayCell => c != null);
      if (!first) return;
      const mon = first.day.slice(5, 7);
      if (mon !== prevMon) {
        monthMarks.push({ col: i, label: `${Number(mon)}月` });
        prevMon = mon;
      }
    });
    return { weeks, levels, monthMarks };
  }, [cells]);

  const cellColor = (tokens: number): string => {
    if (tokens <= 0) return "var(--muted)";
    let lv = 1;
    for (let i = 0; i < 3; i++) if (tokens >= levels[i]) lv = i + 2;
    return `var(--heat-${lv})`;
  };

  const sum = useMemo(() => {
    const acc = { tokens: 0, input: 0, output: 0, cr: 0, cw: 0 };
    for (const c of cells) {
      acc.tokens += c.tokens; acc.input += c.input; acc.output += c.output; acc.cr += c.cacheRead; acc.cw += c.cacheWrite;
    }
    return acc;
  }, [cells]);

  return (
    <div>
      <div
        className="grid"
        style={{ gridTemplateColumns: `16px repeat(${weeks.length}, minmax(0, 1fr))`, gap: 3 }}
        onMouseLeave={() => setTip(null)}
      >
        {/* 月份刻度行：每列一个可溢出标签位 */}
        <div />
        {weeks.map((_, i) => {
          const mark = monthMarks.find((m) => m.col === i);
          return (
            <div key={i} className="relative h-[11px] text-[9px] leading-none text-muted-foreground">
              {mark && <span className="absolute left-0 top-0 whitespace-nowrap">{mark.label}</span>}
            </div>
          );
        })}
        {WEEKDAYS.map((w, row) => (
          <Fragment key={w}>
            <div className="flex items-center text-[9px] leading-none text-muted-foreground">
              {row % 2 === 0 && row < 5 ? w : ""}
            </div>
            {weeks.map((wk, i) => {
              const c = wk[row];
              return (
                <div
                  key={i}
                  className={`aspect-square w-full rounded-[2px] ${c ? "transition-transform hover:scale-125" : ""}`}
                  style={{ background: c ? cellColor(c.tokens) : "transparent" }}
                  onMouseEnter={(e) => {
                    if (!c) return;
                    const a = tipAnchorRect(e.currentTarget.getBoundingClientRect());
                    setTip({ cell: c, ...a });
                  }}
                />
              );
            })}
          </Fragment>
        ))}
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-muted-foreground">
        <span className="flex items-center gap-1">
          少
          {["var(--muted)", "var(--heat-1)", "var(--heat-2)", "var(--heat-3)", "var(--heat-4)"].map((c) => (
            <span key={c} className="h-[10px] w-[10px] rounded-[2px]" style={{ background: c }} />
          ))}
          多
        </span>
        <span className="ml-auto flex items-center gap-1">
          <span className="h-2 w-2 rounded-full" style={{ background: "var(--meter-3)" }} /> 缓存读 {fmtTok(sum.cr)} ·
          <span className="ml-1 h-2 w-2 rounded-full" style={{ background: "var(--meter-6)" }} /> 缓存写 {fmtTok(sum.cw)}
          {sum.tokens > 0 && <> · 缓存占比 {(((sum.cr + sum.cw) / sum.tokens) * 100).toFixed(1)}%</>}
        </span>
      </div>
      {tip && tip.cell.tokens > 0 && (
        <FloatTip x={tip.x} y={tip.y}>
          <div className="font-medium">{tip.cell.day}</div>
          <div className="mt-1 flex items-baseline justify-between gap-3">
            <span>
              <span className="text-muted-foreground">总消耗 </span>
              <span className="font-mono font-semibold tabular-nums">{fmtTok(tip.cell.tokens)}</span>
            </span>
            <span className="text-muted-foreground">
              会话 <span className="font-mono text-foreground tabular-nums">{tip.cell.sessions}</span>
            </span>
          </div>
          <div className="mt-0.5 flex items-baseline justify-between gap-3 text-[11px]">
            <span className="text-muted-foreground">输入 / 输出</span>
            <span className="font-mono tabular-nums">
              {fmtTok(tip.cell.input)} / {fmtTok(tip.cell.output)}
            </span>
          </div>
          {(tip.cell.cacheRead > 0 || tip.cell.cacheWrite > 0) && (
            <>
              <div className="mt-1 flex items-center gap-2 text-[11px]">
                <span className="w-10 shrink-0 text-muted-foreground">缓存读</span>
                <MiniBar pct={tip.cell.tokens ? tip.cell.cacheRead / tip.cell.tokens : 0} color="var(--meter-3)" />
                <span className="w-[92px] shrink-0 text-right font-mono tabular-nums">
                  {fmtTok(tip.cell.cacheRead)} · {tip.cell.tokens ? ((tip.cell.cacheRead / tip.cell.tokens) * 100).toFixed(0) : 0}%
                </span>
              </div>
              <div className="mt-0.5 flex items-center gap-2 text-[11px]">
                <span className="w-10 shrink-0 text-muted-foreground">缓存写</span>
                <MiniBar pct={tip.cell.tokens ? tip.cell.cacheWrite / tip.cell.tokens : 0} color="var(--meter-6)" />
                <span className="w-[92px] shrink-0 text-right font-mono tabular-nums">
                  {fmtTok(tip.cell.cacheWrite)} · {tip.cell.tokens ? ((tip.cell.cacheWrite / tip.cell.tokens) * 100).toFixed(1) : 0}%
                </span>
              </div>
            </>
          )}
          {tip.cell.topModel && (
            <div className="mt-1 flex items-center gap-2 border-t border-border/60 pt-1 text-[11px]">
              <span className="h-2 w-2 shrink-0 rounded-[3px]" style={{ background: SERIES_COLORS[0] }} />
              <span className="w-[86px] shrink-0 truncate text-muted-foreground" title={tip.cell.topModel.model}>
                {tip.cell.topModel.model}
              </span>
              <MiniBar pct={tip.cell.tokens ? tip.cell.topModel.tokens / tip.cell.tokens : 0} color="var(--heat-4)" />
              <span className="w-[86px] shrink-0 text-right font-mono tabular-nums">
                {fmtTok(tip.cell.topModel.tokens)} ·{" "}
                {tip.cell.tokens ? ((tip.cell.topModel.tokens / tip.cell.tokens) * 100).toFixed(0) : 0}%
              </span>
            </div>
          )}
        </FloatTip>
      )}
    </div>
  );
}
