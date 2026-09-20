import { CircleAlert, PackageCheck, Puzzle, Workflow } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { PluginCandidate } from "../types";

interface Props {
  candidates: PluginCandidate[];
  /** 已选中的安装规格（installSpec） */
  selected: string[];
  onChange: (specs: string[]) => void;
  /** 多选（monorepo 一次装多个子包）/ 单选 */
  multiple?: boolean;
  emptyText?: string;
  /** 每个候选行尾的额外动作（如 clone 模式「填入子路径」） */
  renderExtra?: (c: PluginCandidate) => React.ReactNode;
}

const rootLabel = (c: PluginCandidate) => (c.path === "" ? "仓库根目录" : c.path);

/**
 * 插件候选列表（GitHub 仓库探测 / 本地目录探测共用）。
 *
 * 每行是一个 ToggleGroup 选项：徽标说明是否为 dsh 插件包、lib/ 构建产物是否齐备、
 * 是否命中 workspace 成员 glob；monorepo 下可多选一次装多个子包。
 */
export default function PluginCandidateList({
  candidates, selected, onChange, multiple = true, emptyText, renderExtra,
}: Props) {
  if (candidates.length === 0) {
    return (
      <p className="py-2 text-center text-[11.5px] text-muted-foreground">
        {emptyText ?? "没有探测到插件包"}
      </p>
    );
  }

  // radix 的 ToggleGroup 是 type 判别联合：多选 / 单选的 value 类型不同，分开构造 props
  const valueProps = multiple
    ? {
        type: "multiple" as const,
        value: selected,
        onValueChange: (v: string[]) => onChange(v),
      }
    : {
        type: "single" as const,
        value: selected[0] ?? "",
        onValueChange: (v: string) => onChange(v ? [v] : []),
      };

  return (
    <ToggleGroup
      {...valueProps}
      orientation="vertical"
      spacing={6}
      className="w-full items-stretch"
    >
      {candidates.map((c) => (
        <ToggleGroupItem
          key={`${c.path}|${c.name ?? ""}`}
          value={c.installSpec}
          variant="outline"
          size="sm"
          // 带 ! 的几项是为了压过 toggleVariants 的 size/variant 默认值（h-7 / font-medium / whitespace-nowrap）
          className="h-auto! w-full flex-col items-stretch gap-1.5 overflow-visible rounded-lg! px-2.5 py-2 text-left font-normal! whitespace-normal! data-[state=on]:border-primary/50 data-[state=on]:bg-primary/5"
        >
          <span className="flex w-full items-start gap-2">
            <span className="flex min-w-0 flex-1 flex-wrap items-center gap-1.5">
              <span
                className="min-w-0 truncate font-mono text-[12.5px] font-semibold"
                title={c.name ?? c.path}
              >
                {c.name ?? rootLabel(c)}
              </span>
              {c.version && (
                <Badge variant="outline" className="font-mono text-[10px]">
                  {c.version}
                </Badge>
              )}
            </span>
            {renderExtra && (
              // 行尾动作不能触发选中态：阻止冒泡到 ToggleGroupItem
              <span className="shrink-0" onClick={(e) => e.stopPropagation()}>
                {renderExtra(c)}
              </span>
            )}
          </span>

          <span className="flex flex-wrap items-center gap-1.5">
            {c.hasBundle ? (
              <Badge variant="success" className="text-[10px]">
                <PackageCheck /> dsh 插件包
              </Badge>
            ) : (
              <Badge variant="warning" className="text-[10px]">
                <CircleAlert /> 未声明 dsh.bundle
              </Badge>
            )}
            {c.libOk ? (
              <Badge variant="outline" className="text-[10px]">lib/ 已就绪</Badge>
            ) : (
              <Badge variant="destructive" className="text-[10px]">lib/ 缺失</Badge>
            )}
            {c.workspaceMember && c.path !== "" && (
              <Badge variant="secondary" className="text-[10px]">
                <Workflow /> workspace 子包
              </Badge>
            )}
            {!c.hasBundle && c.libOk && c.hasPatch && (
              <Badge variant="outline" className="text-[10px]">含 cordis.patch.yml</Badge>
            )}
          </span>

          {c.description && (
            <span className="line-clamp-2 text-[11px] leading-relaxed text-muted-foreground">
              {c.description}
            </span>
          )}

          {c.path !== "" && (
            <span className="break-all font-mono text-[10.5px] text-muted-foreground">
              <Puzzle className="mr-1 inline h-3 w-3" />
              {c.path}
            </span>
          )}

          {!c.libOk && (
            <span className="text-[10.5px] leading-relaxed text-amber-600 dark:text-amber-400">
              仓库未提交构建产物：直接安装会缺 lib/，建议改用「Clone 仓库」方式（会装依赖并构建）
            </span>
          )}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
