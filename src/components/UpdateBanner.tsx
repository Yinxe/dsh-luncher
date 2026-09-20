import { Rocket, TriangleAlert } from "lucide-react";
import {
  Alert, AlertAction, AlertDescription, AlertTitle,
} from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import type { LauncherUpdateStatus } from "../types";

interface Props {
  status: LauncherUpdateStatus;
  onDismiss: () => void;
  onOpenUrl: (url: string) => void;
  onApply: () => void;
  applying: boolean;
  /** 下载进度（内置 updater 模式才有） */
  progress?: { received: number; total: number } | null;
}

export default function UpdateBanner({
  status, onDismiss, onOpenUrl, onApply, applying, progress,
}: Props) {
  if (!status.available) {
    if (status.mode === "error" && status.message) {
      return (
        <Alert
          variant="destructive"
          className="shrink-0 animate-in gap-1.5 rounded-none border-x-0 border-t-0 border-red-500/30 bg-red-500/10 px-4 py-2 text-[13px] fade-in slide-in-from-top-2 duration-300"
        >
          <TriangleAlert />
          <AlertTitle className="font-normal">{status.message}</AlertTitle>
          <AlertAction>
            <Button size="sm" variant="ghost" onClick={onDismiss}>知道了</Button>
          </AlertAction>
        </Alert>
      );
    }
    return null;
  }

  return (
    <Alert className="shrink-0 animate-in gap-1 rounded-none border-x-0 border-t-0 border-primary/30 bg-gradient-to-r from-primary/15 to-primary/5 px-4 py-2 pr-56 text-[13px] fade-in slide-in-from-top-2 duration-300">
      <Rocket />
      <AlertTitle>
        启动器新版本 <b>v{status.latest}</b> 已发布（当前 v{status.current}）
      </AlertTitle>
      {status.notes && <AlertDescription className="line-clamp-2">{status.notes}</AlertDescription>}
      {applying && status.mode === "builtin" && (
        <div className="mt-1 flex items-center gap-2">
          <Progress
            className="h-1.5 w-64"
            value={progress && progress.total > 0 ? Math.round((progress.received / progress.total) * 100) : 0}
          />
          <span className="text-[11px] tabular-nums text-muted-foreground">
            {progress && progress.total > 0
              ? `${(progress.received / 1048576).toFixed(1)} / ${(progress.total / 1048576).toFixed(1)} MB`
              : "正在连接更新源…"}
          </span>
        </div>
      )}
      <AlertAction className="flex gap-1.5">
        {status.mode === "builtin" && (
          <Button size="sm" disabled={applying} onClick={onApply}>
            {applying ? "下载安装中…" : "下载并安装"}
          </Button>
        )}
        {status.mode === "manifest" && status.url && (
          <Button size="sm" onClick={() => onOpenUrl(status.url!)}>前往下载</Button>
        )}
        <Button size="sm" variant="ghost" onClick={onDismiss}>稍后</Button>
      </AlertAction>
    </Alert>
  );
}
