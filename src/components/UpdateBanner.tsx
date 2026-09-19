import type { LauncherUpdateStatus } from "../types";

interface Props {
  status: LauncherUpdateStatus;
  onDismiss: () => void;
  onOpenUrl: (url: string) => void;
  onApply: () => void;
  applying: boolean;
}

export default function UpdateBanner({
  status,
  onDismiss,
  onOpenUrl,
  onApply,
  applying,
}: Props) {
  if (!status.available) {
    if (status.mode === "error" && status.message) {
      return (
        <div className="banner err">
          <span>⚠ {status.message}</span>
          <span className="grow" />
          <button className="sm ghost" onClick={onDismiss}>
            知道了
          </button>
        </div>
      );
    }
    return null;
  }

  return (
    <div className="banner">
      <span>
        🚀 启动器新版本 <strong>v{status.latest}</strong> 已发布（当前 v{status.current}）
        {status.notes ? `：${status.notes}` : ""}
      </span>
      <span className="grow" />
      {status.mode === "builtin" && (
        <button className="primary sm" disabled={applying} onClick={onApply}>
          {applying ? "下载安装中…" : "下载并安装"}
        </button>
      )}
      {status.mode === "manifest" && status.url && (
        <button className="primary sm" onClick={() => onOpenUrl(status.url!)}>
          前往下载
        </button>
      )}
      <button className="sm ghost" onClick={onDismiss}>
        稍后
      </button>
    </div>
  );
}
