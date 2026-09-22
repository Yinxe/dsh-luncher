import { useMemo, type CSSProperties } from "react";
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
export const WEEKDAYS = ["日", "一", "二", "三", "四", "五", "六"];

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

/**
 * GitHub 风格活跃热力图：列=周、行=星期日→星期六。
 * 非零 token 按四分位分 4 档（--heat-1..4 绿阶），带月份与星期刻度。
 */
export function Heatmap({ cells }: { cells: DayCell[] }) {
  const { weeks, levels, monthMarks } = useMemo(() => {
    const nz = cells.filter((c) => c.tokens > 0).map((c) => c.tokens).sort((a, b) => a - b);
    const q = (f: number) => (nz.length === 0 ? Infinity : nz[Math.min(nz.length - 1, Math.floor(nz.length * f))]);
    const levels = [q(0.25), q(0.5), q(0.75)];
    // 首日向前补空格，让每列对齐星期日→星期六
    const lead = cells.length === 0 ? 0 : (new Date(`${cells[0].day}T00:00:00`).getDay() + 7) % 7;
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
    let lv = 0;
    for (let i = 0; i < 3; i++) if (tokens >= levels[i]) lv = i + 1;
    return `var(--heat-${lv})`;
  };

  return (
    <div className="overflow-x-auto pb-1">
      <div className="flex flex-col gap-[3px]">
        {/* 月份刻度行：每列 11px 格 + 3px 缝 = 14px 一个身位 */}
        <div className="flex gap-[3px] pl-[18px] text-[9px] leading-none text-muted-foreground">
          {weeks.map((_, i) => {
            const mark = monthMarks.find((m) => m.col === i);
            return (
              <div key={i} className="relative h-[10px] w-[11px] shrink-0">
                {mark && <span className="absolute left-0 top-0 whitespace-nowrap">{mark.label}</span>}
              </div>
            );
          })}
        </div>
        <div className="flex gap-[3px]">
          {/* 星期刻度列：只标一/三/五，避免拥挤 */}
          <div className="flex w-[15px] shrink-0 flex-col gap-[3px] text-[9px] leading-[11px] text-muted-foreground">
            {WEEKDAYS.map((w, i) => (
              <div key={i} className="h-[11px]">{i % 2 === 1 ? w : ""}</div>
            ))}
          </div>
          {weeks.map((wk, i) => (
            <div key={i} className="flex flex-col gap-[3px]">
              {wk.map((c, j) => (
                <div
                  key={j}
                  className="h-[11px] w-[11px] rounded-[2px] transition-transform hover:scale-125"
                  style={{ background: c ? cellColor(c.tokens) : "transparent" }}
                  title={c ? `${c.day} · ${fmtTok(c.tokens)} tokens · ${c.sessions} 个会话` : undefined}
                />
              ))}
            </div>
          ))}
        </div>
      </div>
      <div className="mt-1.5 flex items-center justify-end gap-1 pl-[18px] text-[10px] text-muted-foreground">
        <span>少</span>
        {["var(--muted)", "var(--heat-1)", "var(--heat-2)", "var(--heat-3)", "var(--heat-4)"].map((c) => (
          <span key={c} className="h-[10px] w-[10px] rounded-[2px]" style={{ background: c }} />
        ))}
        <span>多</span>
      </div>
    </div>
  );
}
