import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Led } from "@/components/site/led";
import { statusTone, type ReleaseState, type SourceId, type SourceStatus } from "@/lib/release";
import { cn } from "cn";

/**
 * 更新源二选一。
 *
 * 这不是装饰性的分段控件：切一下，全页所有直链、体积口径与说明文字都会换 ——
 * 与启动器设置里的「更新源」是同一件事的两个入口。选择会记住（localStorage），
 * 也能用 `?source=github` 直接深链过来。
 */
export function SourceSwitch({
  value,
  onChange,
  state,
  className,
  showStatus = true,
}: {
  value: SourceId;
  onChange: (next: SourceId) => void;
  state: ReleaseState;
  className?: string;
  showStatus?: boolean;
}) {
  const status: Record<SourceId, SourceStatus> = { r2: state.r2, github: state.github };

  return (
    <ToggleGroup
      type="single"
      value={value}
      onValueChange={(next) => {
        if (next === "r2" || next === "github") onChange(next);
      }}
      variant="outline"
      size="sm"
      spacing={0}
      className={cn("overflow-hidden rounded-lg [&>*]:rounded-none [&>*:first-child]:rounded-l-lg [&>*:last-child]:rounded-r-lg", className)}
      aria-label="下载源"
    >
      {(["r2", "github"] as const).map((id) => (
        <ToggleGroupItem
          key={id}
          value={id}
          className="h-8 gap-2 px-3 text-xs data-[state=on]:bg-primary/15 data-[state=on]:text-foreground"
          title={id === "r2" ? "自建源（Cloudflare R2）：国内直连可用，地址永久不变" : "GitHub Release：与版本绑定的地址，作为兜底"}
        >
          {showStatus ? (
            <Led tone={statusTone(state.settled ? status[id] : "probing")} pulse={!state.settled} />
          ) : null}
          <span className="font-mono tracking-wide">{id === "r2" ? "自建源" : "GitHub"}</span>
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
