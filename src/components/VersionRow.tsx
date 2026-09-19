import { FolderOpen, RefreshCw } from "lucide-react";
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

const RAIL: Record<string, string> = {
  latest: "bg-emerald-500",
  stable: "bg-emerald-600/70",
  rc: "bg-sky-500",
  alpha: "bg-amber-500",
  beta: "bg-amber-500",
  next: "bg-violet-500",
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
      className={`brackets relative flex-row items-stretch gap-0 overflow-hidden p-0 transition-colors ${
        isActive ? "border-primary/45 shadow-[0_0_0_1px_rgba(99,102,241,0.15)]" : "hover:border-primary/25"
      }`}
    >
      {/* 通道色轨 */}
      <span className={`w-[3px] shrink-0 ${RAIL[row.channel] ?? "bg-border"} ${isActive ? "bg-primary" : ""}`} />

      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-4 gap-y-1.5 px-4 py-2.5">
        {/* 版本标识区 */}
        <div className="w-60 shrink-0">
          <div className="flex items-center gap-1.5">
            <span className="font-mono text-[13.5px] font-bold tracking-tight">{row.version}</span>
            <Badge variant={CHANNEL_V[row.channel] ?? "secondary"} className="uppercase">
              {row.channel}
            </Badge>
            {isLatestTag && <Badge variant="info">latest</Badge>}
          </div>
          <div className="mt-0.5 font-mono text-[10.5px] text-muted-foreground">
            {[
              row.remote ? fmtDate(row.remote.publishedAt) : null,
              row.remote ? fmtSize(row.remote.unpackedSize) : null,
              upgradeTo ? null : null,
            ]
              .filter(Boolean)
              .join(" · ")}
          </div>
        </div>

        {/* 状态区 */}
        <div className="min-w-0 flex-1">
          {inst ? (
            <div className="min-w-0">
              <div className="flex items-center gap-1.5">
                <span className={`led ${isActive ? "bg-primary led-glow text-primary" : "bg-emerald-500 text-emerald-500"}`} />
                <span className="text-[11.5px]">
                  {inst.source === "managed" ? "启动器管理" : inst.source === "global" ? "npm 全局" : "PATH"}
                </span>
                {upgradeTo && (
                  <button
                    className="text-[11px] font-semibold text-amber-500 underline-offset-2 hover:underline"
                    onClick={() => onUpgrade(upgradeTo)}
                    title={`已安装 ${row.version}，点击安装 ${upgradeTo}`}
                  >
                    可升级 → {upgradeTo}
                  </button>
                )}
              </div>
              <div className="mt-0.5 truncate font-mono text-[10.5px] text-muted-foreground/80" title={inst.location}>
                {inst.location}
              </div>
            </div>
          ) : (
            <div className="truncate text-xs text-muted-foreground/80">
              {row.remote?.description ?? "未安装"}
            </div>
          )}
        </div>

        {/* 操作区 */}
        <div className="flex shrink-0 items-center gap-1.5">
          {inst && inst.version !== "unknown" && !isActive && (
            <Button size="sm" disabled={busy} onClick={() => onSetActive(row.version)}>
              设为当前
            </Button>
          )}
          {!inst && (
            <Button size="sm" disabled={busy} onClick={() => onInstall(row.version, false)} title="安装完成后自动设为当前版本">
              安装
            </Button>
          )}
          {inst && inst.source === "managed" && (
            <>
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => onReveal(inst.location)} title="打开安装目录">
                <FolderOpen />
              </Button>
              <Button
                size="sm"
                variant="ghost"
                disabled={busy || isActive}
                onClick={() => onInstall(row.version, true)}
                title={isActive ? "当前版本正在使用，请先切换到其他版本再重装" : "删除后重新下载安装（会设为当前版本）"}
              >
                <RefreshCw /> 重装
              </Button>
              <Button
                size="sm"
                variant="ghost"
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
      </div>
    </Card>
  );
}

const CHANNEL_V: Record<string, "default" | "success" | "warning" | "info" | "secondary" | "outline"> = {
  latest: "success",
  stable: "success",
  rc: "info",
  alpha: "warning",
  beta: "warning",
  next: "secondary",
};
