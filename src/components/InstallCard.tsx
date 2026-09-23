import { Loader2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";

interface Props {
  version: string;
  onCancel: () => void;
  /** 聚焦右侧通用终端面板中本次安装任务（实时输出都在那里） */
  onOpenTerminal: () => void;
}

/**
 * 版本页安装进行中的状态条：进度入口 + 取消。
 * 实时日志已统一到右侧终端面板（SystemTaskView），这里不再嵌一块滚动输出。
 */
export default function InstallCard({ version, onCancel, onOpenTerminal }: Props) {
  return (
    <Card className="border-primary/50 bg-primary/5 p-3.5">
      <div className="flex items-center gap-2.5">
        <Loader2 className="h-4 w-4 shrink-0 animate-spin text-primary" />
        <span className="text-sm font-semibold">正在安装 dsh {version}</span>
        <span className="min-w-0 truncate text-xs text-muted-foreground">
          依赖较多，请耐心等待…实时输出在右侧终端面板
        </span>
        <span className="flex-1" />
        <Button size="sm" variant="outline" onClick={onOpenTerminal}>
          在终端中查看
        </Button>
        <Button size="sm" variant="destructive" onClick={onCancel}>
          <X /> 取消
        </Button>
      </div>
    </Card>
  );
}
