import { useState } from "react";
import { Rocket, Sparkles, TriangleAlert } from "lucide-react";
import {
  Alert, AlertAction, AlertDescription, AlertTitle,
} from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter,
  DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Progress } from "@/components/ui/progress";
import ReleaseNotes, { notesTeaser } from "./ReleaseNotes";
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

/** 下载进度行：横幅与「新特性」弹窗里共用 */
function DownloadBar({
  progress, needsElevation,
}: {
  progress?: { received: number; total: number } | null;
  needsElevation?: boolean;
}) {
  return (
    <div className="flex items-center gap-2">
      <Progress
        className="h-1.5 w-64"
        value={progress && progress.total > 0 ? Math.round((progress.received / progress.total) * 100) : 0}
      />
      <span className="text-[11px] tabular-nums text-muted-foreground">
        {progress && progress.total > 0
          ? progress.received >= progress.total && needsElevation
            // deb/rpm：下载完由 pkexec 接管，此时正在等用户输密码
            ? "等待管理员授权…"
            : `${(progress.received / 1048576).toFixed(1)} / ${(progress.total / 1048576).toFixed(1)} MB`
          : "正在连接更新源…"}
      </span>
    </div>
  );
}

export default function UpdateBanner({
  status, onDismiss, onOpenUrl, onApply, applying, progress,
}: Props) {
  const [notesOpen, setNotesOpen] = useState(false);

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

  const notes = (status.notes || "").trim();
  const teaser = notesTeaser(notes);
  const installable = status.mode === "builtin";
  const downloadable = status.mode === "manifest" && !!status.url;

  return (
    <>
      <Alert className="shrink-0 animate-in gap-1 rounded-none border-x-0 border-t-0 border-primary/30 bg-gradient-to-r from-primary/15 to-primary/5 px-4 py-2 pr-80 text-[13px] fade-in slide-in-from-top-2 duration-300">
        <Rocket />
        <AlertTitle>
          启动器新版本 <b>v{status.latest}</b> 已发布（当前 v{status.current}）
        </AlertTitle>
        {notes ? (
          <AlertDescription className="line-clamp-2">{teaser}</AlertDescription>
        ) : (
          <AlertDescription className="line-clamp-2">{status.message}</AlertDescription>
        )}
        {applying && installable && (
          <div className="mt-1">
            <DownloadBar progress={progress} needsElevation={status.needsElevation} />
          </div>
        )}
        <AlertAction className="flex gap-1.5">
          {notes && (
            <Button size="sm" variant="outline" onClick={() => setNotesOpen(true)}>
              <Sparkles /> 查看新特性
            </Button>
          )}
          {installable && (
            <Button size="sm" disabled={applying} onClick={onApply}>
              {applying
                ? "下载安装中…"
                : status.needsElevation ? "下载并安装（需管理员授权）" : "下载并安装"}
            </Button>
          )}
          {downloadable && (
            <Button size="sm" onClick={() => onOpenUrl(status.url!)}>前往下载</Button>
          )}
          <Button size="sm" variant="ghost" onClick={onDismiss}>稍后</Button>
        </AlertAction>
      </Alert>

      {/* 更新说明全文：横幅只放摘要，想看细节的人点这里（说明来自 CHANGELOG.md 的版本段落） */}
      <Dialog open={notesOpen} onOpenChange={setNotesOpen}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>v{status.latest} 新特性</DialogTitle>
            <DialogDescription>
              当前 v{status.current} · 以下说明与更新日志同源
            </DialogDescription>
          </DialogHeader>
          <div className="max-h-[55vh] overflow-y-auto rounded-lg border bg-muted/30 p-3">
            {notes
              ? <ReleaseNotes notes={notes} onOpenUrl={onOpenUrl} />
              : <p className="text-[13px] text-muted-foreground">这个版本没有提供更新说明。</p>}
          </div>
          {applying && installable && (
            <DownloadBar progress={progress} needsElevation={status.needsElevation} />
          )}
          <DialogFooter>
            {installable && (
              <Button disabled={applying} onClick={onApply}>
                {applying
                  ? "下载安装中…"
                  : status.needsElevation ? "下载并安装（需管理员授权）" : "下载并安装"}
              </Button>
            )}
            {downloadable && (
              <Button onClick={() => onOpenUrl(status.url!)}>前往下载</Button>
            )}
            <DialogClose asChild>
              <Button variant="outline">关闭</Button>
            </DialogClose>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
