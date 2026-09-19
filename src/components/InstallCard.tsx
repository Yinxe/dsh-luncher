import { Loader2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";

interface Props {
  version: string;
  logs: string[];
  onCancel: () => void;
}

export default function InstallCard({ version, logs, onCancel }: Props) {
  return (
    <Card className="border-primary/50 bg-primary/5 p-3.5">
      <div className="flex items-center gap-2.5">
        <Loader2 className="h-4 w-4 animate-spin text-primary" />
        <span className="text-sm font-semibold">正在安装 dsh {version}</span>
        <span className="text-xs text-muted-foreground">依赖较多，请耐心等待…</span>
        <span className="flex-1" />
        <Button size="sm" variant="destructive" onClick={onCancel}>
          <X /> 取消
        </Button>
      </div>
      <div className="mt-2.5 max-h-44 overflow-y-auto rounded-md bg-background p-2.5 font-mono text-[11px] leading-relaxed text-muted-foreground select-text">
        {logs.length === 0 ? "正在启动 npm…" : logs.join("\n")}
      </div>
    </Card>
  );
}
