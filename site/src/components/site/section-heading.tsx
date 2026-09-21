import type { ReactNode } from "react";

import { cn } from "cn";

/**
 * 区块标题：`[ 02 ]` 编号 + 分类词 + 一条拉向右边界的发展线。
 * 这条线是全页的骨架 —— 每节都从同一条刻度出发，读起来像一份图纸。
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
      <div className="flex items-center gap-3">
        <span className="eyebrow text-foreground/70">[ {index} ]</span>
        {kicker ? <span className="eyebrow text-primary">{kicker}</span> : null}
        <span className="rule-fade min-w-6 flex-1" />
        {action}
      </div>
      <h2 className="text-display-cjk mt-5 text-[1.75rem] sm:text-[2.1rem]">{title}</h2>
      {lead ? <div className="mt-3 max-w-3xl text-[15px] text-muted-foreground">{lead}</div> : null}
    </div>
  );
}
