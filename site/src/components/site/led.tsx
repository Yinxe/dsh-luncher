import { cn } from "cn";

/** 状态灯：颜色即语义（可用 / 需要注意 / 未知），可选呼吸圈表示「正在探测」 */
export function Led({
  tone = "signal",
  pulse = false,
  className,
}: {
  tone?: "signal" | "warn" | "muted";
  pulse?: boolean;
  className?: string;
}) {
  const color =
    tone === "signal" ? "text-signal" : tone === "warn" ? "text-warn" : "text-muted-foreground";
  return <span aria-hidden="true" className={cn("led", color, pulse && "led-pulse", className)} />;
}
