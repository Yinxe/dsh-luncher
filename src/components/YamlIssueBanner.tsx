import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { YamlIssue } from "@/lib/yaml-validate";

interface Props {
  issues: YamlIssue[];
  /** 有未保存修改才显示「语法正常」的绿点，避免打开即宣告 */
  dirty: boolean;
  className?: string;
}

/** YAML 校验状态条：有错时列出前几条（与编辑器行内标红同源），无错且有改动时给静默确认 */
export default function YamlIssueBanner({ issues, dirty, className }: Props) {
  if (issues.length === 0) {
    if (!dirty) return null;
    return (
      <div className={cn("flex items-center gap-1.5 text-[11px] text-muted-foreground", className)}>
        <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-emerald-500" />
        YAML 语法校验通过，可保存
      </div>
    );
  }
  return (
    <div className={cn("space-y-0.5 rounded-lg border border-destructive/25 bg-destructive/5 px-2.5 py-1.5", className)}>
      <div className="text-[11.5px] font-medium text-destructive">
        {issues.length >= 8 ? "存在多处语法错误（已显示前 8 处）" : `发现 ${issues.length} 处语法错误`}——修正后才能保存
      </div>
      <ul className="space-y-0.5">
        {issues.slice(0, 3).map((it, i) => (
          <li key={i} className="truncate text-[11px] text-muted-foreground">
            第 {it.line} 行 第 {it.column} 列：{it.message}
          </li>
        ))}
      </ul>
    </div>
  );
}

/** 未保存/有错的状态徽标：语法错误优先于「未保存」展示 */
export function YamlDirtyBadge({ dirty, issueCount }: { dirty: boolean; issueCount: number }) {
  if (issueCount > 0) {
    return (
      <Badge variant="destructive" className="shrink-0">
        {issueCount >= 8 ? "8+ 处语法错误" : `${issueCount} 处语法错误`}
      </Badge>
    );
  }
  if (dirty) return <Badge variant="warning">未保存</Badge>;
  return null;
}
