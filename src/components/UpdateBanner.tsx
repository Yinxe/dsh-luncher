import { Button } from "@/components/ui/button";
import type { LauncherUpdateStatus } from "../types";

interface Props {
  status: LauncherUpdateStatus;
  onDismiss: () => void;
  onOpenUrl: (url: string) => void;
  onApply: () => void;
  applying: boolean;
}

export default function UpdateBanner({ status, onDismiss, onOpenUrl, onApply, applying }: Props) {
  if (!status.available) {
    if (status.mode === "error" && status.message) {
      return (
        <div className="flex shrink-0 items-center gap-3 border-b border-red-500/30 bg-red-500/10 px-4 py-2 text-[13px]">
          <span>⚠ {status.message}</span>
          <span className="flex-1" />
          <Button size="sm" variant="ghost" onClick={onDismiss}>知道了</Button>
        </div>
      );
    }
    return null;
  }

  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-primary/30 bg-gradient-to-r from-primary/15 to-violet-500/5 px-4 py-2 text-[13px]">
      <span>
        🚀 启动器新版本 <b>v{status.latest}</b> 已发布（当前 v{status.current}）
        {status.notes ? `：${status.notes}` : ""}
      </span>
      <span className="flex-1" />
      {status.mode === "builtin" && (
        <Button size="sm" disabled={applying} onClick={onApply}>
          {applying ? "下载安装中…" : "下载并安装"}
        </Button>
      )}
      {status.mode === "manifest" && status.url && (
        <Button size="sm" onClick={() => onOpenUrl(status.url!)}>前往下载</Button>
      )}
      <Button size="sm" variant="ghost" onClick={onDismiss}>稍后</Button>
    </div>
  );
}
