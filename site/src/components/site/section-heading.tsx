import type { ReactNode } from "react";

import { cn } from "cn";

/**
 * 区块的「仪表盘记号」：`[ 02 ]` 编号 + 分类词 + 一条拉向右边界的发展线。
 * 抽出来是因为有两种排版都要用它：居中堆叠（SectionHeading）与左侧悬挂（split 区块）。
 */
export function SectionEyebrow({
  index,
  kicker,
  action,
  className,
}: {
  index: string;
  kicker?: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex items-center gap-3", className)}>
      <span className="eyebrow text-foreground/70">[ {index} ]</span>
      {kicker ? <span className="eyebrow text-primary">{kicker}</span> : null}
      <span className="rule-fade min-w-6 flex-1" />
      {action}
    </div>
  );
}

/**
 * 常规区块标题：编号行 + 标题 + 导语，整段压在内容上方。
 * 长内容区块（更新日志、常见问题）改用左侧悬挂的两栏排版，见各自组件。
 */
export function SectionHeading({
  index,
  kicker,
  title,
  lead,
  id,
  className,
  action,
}: {
  index: string;
  kicker?: string;
  title: ReactNode;
  lead?: ReactNode;
  id?: string;
  className?: string;
  action?: ReactNode;
}) {
  return (
    <div id={id} className={cn("scroll-mt-24", className)}>
      <SectionEyebrow index={index} kicker={kicker} action={action} />
      <h2 className="section-title mt-5">{title}</h2>
      {lead ? <div className="mt-3 max-w-2xl text-[14.5px] text-muted-foreground">{lead}</div> : null}
    </div>
  );
}
