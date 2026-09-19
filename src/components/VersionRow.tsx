import { FolderOpen, PlayCircle, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import type { InstalledVersion } from "../types";

interface MergedRow {
  version: string;
  remote: { publishedAt: string | null; description: string | null; unpackedSize: number | null } | null;
  installed: InstalledVersion | null;
  channel: string;
}

interface Props {
  row: MergedRow;
  isLatestTag: boolean;
  busy: boolean;
  isActive: boolean;
  upgradeTo: string | null;
  onUpgrade: (target: string) => void;
  onSetActive: (version: string) => void;
  onInstall: (version: string, force: boolean) => void;
  onUninstall: (version: string) => void;
  onReveal: (path: string) => void;
}

const CHANNEL_VARIANT: Record<string, "default" | "success" | "warning" | "info" | "secondary"> = {
  latest: "default",
  stable: "success",
  rc: "info",
  alpha: "warning",
  beta: "warning",
  next: "secondary",
};

export default function VersionRow({
  row, isLatestTag, busy, isActive, upgradeTo, onUpgrade, onSetActive, onInstall, onUninstall, onReveal,
}: Props) {
  const inst = row.installed;
  const fmtDate = (iso: string | null) =>
    iso && !isNaN(new Date(iso).getTime()) ? new Date(iso).toISOString().slice(0, 10) : "";
  const fmtSize = (n: number | null) =>
    n == null ? "" : n > 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`;

  return (
    <Card
      className={`flex items-center gap-4 p-3.5 transition-colors ${
        isActive ? "border-primary/50 ring-1 ring-primary/30" : "hover:border-primary/30"
      }`}
    >
      <div className="w-[280px] shrink-0">
        <div className="flex items-center gap-2">
          <span className="font-mono text-[15px] font-bold">{row.version}</span>
          <Badge variant={CHANNEL_VARIANT[row.channel] ?? "secondary"}>{row.channel}</Badge>
          {isLatestTag && <Badge variant="info">latest</Badge>}
          {isActive && <Badge variant="success">当前版本</Badge>}
        </div>
        <div className="mt-1 text-[11px] text-muted-foreground">
          {[
            row.remote ? fmtDate(row.remote.publishedAt) : null,
            row.remote ? fmtSize(row.remote.unpackedSize) : null,
          ]
            .filter(Boolean)
            .join(" · ")}
          {upgradeTo && (
            <>
              {" · "}
              <button
                className="font-semibold text-amber-500 underline-offset-2 hover:underline"
                onClick={() => onUpgrade(upgradeTo)}
                title={`已安装 ${row.version}，点击安装 ${upgradeTo}`}
              >
                可升级 → {upgradeTo}
              </button>
            </>
          )}
        </div>
      </div>

      <div className="min-w-0 flex-1">
        {inst ? (
          <>
            <Badge variant={inst.source === "managed" ? "success" : inst.source === "global" ? "info" : "outline"}>
              {inst.source === "managed" ? "已装 · 启动器管理" : inst.source === "global" ? "已装 · npm 全局" : "已装 · PATH"}
            </Badge>
            <div className="mt-1 truncate font-mono text-[10.5px] text-muted-foreground" title={inst.location}>
              {inst.location}
            </div>
          </>
        ) : (
          <span className="text-xs text-muted-foreground">{row.remote?.description ?? "未安装"}</span>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-2">
        {inst && inst.version !== "unknown" && !isActive && (
          <Button size="sm" disabled={busy} onClick={() => onSetActive(row.version)} title="设为当前版本：所有 Profile 实例将基于该版本启动">
            设为当前
          </Button>
        )}
        {!inst && (
          <Button size="sm" disabled={busy} onClick={() => onInstall(row.version, false)} title="安装完成后自动设为当前版本">
            <PlayCircle /> 安装
          </Button>
        )}
        {inst && inst.source === "managed" && (
          <>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => onReveal(inst.location)}>
              <FolderOpen /> 目录
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={busy || isActive}
              onClick={() => onInstall(row.version, true)}
              title={isActive ? "当前版本正在使用，请先切换到其他版本再重装" : "删除后重新下载安装（会设为当前版本）"}
            >
              <RefreshCw /> 重装
            </Button>
            <Button
              size="sm"
              variant="outline"
              className="text-destructive hover:bg-destructive/10 hover:text-destructive"
              disabled={busy || isActive}
              onClick={() => onUninstall(row.version)}
              title={isActive ? "当前版本不允许卸载，请先切换到其他版本" : undefined}
            >
              卸载
            </Button>
          </>
        )}
      </div>
    </Card>
  );
}
