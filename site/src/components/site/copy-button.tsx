import { useState } from "react";
import { CheckIcon, CopyIcon } from "lucide-react";
import { cn } from "cn";

import { Button } from "@/components/ui/button";
import { copyText } from "@/lib/copy";

/**
 * 复制按钮：成功后就地变成 ✓（1.6 秒后复位），不弹 toast ——
 * 一页里会有十几个复制点，每个都弹一次提示反而吵。
 */
export function CopyButton({
  text,
  label = "复制直链",
  className,
  variant = "outline",
  size = "sm",
  iconOnly = false,
}: {
  text: string;
  label?: string;
  className?: string;
  variant?: React.ComponentProps<typeof Button>["variant"];
  size?: React.ComponentProps<typeof Button>["size"];
  iconOnly?: boolean;
}) {
  const [done, setDone] = useState(false);

  async function run() {
    const ok = await copyText(text);
    if (!ok) return;
    setDone(true);
    window.setTimeout(() => setDone(false), 1600);
  }

  return (
    <Button
      type="button"
      variant={variant}
      size={iconOnly ? "icon-sm" : size}
      onClick={run}
      aria-label={iconOnly ? label : undefined}
      title={label}
      className={cn("font-normal", className)}
    >
      {done ? <CheckIcon className="text-signal" /> : <CopyIcon />}
      {iconOnly ? null : <span>{done ? "已复制" : label}</span>}
    </Button>
  );
}
