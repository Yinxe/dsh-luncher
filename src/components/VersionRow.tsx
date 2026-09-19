import { FolderOpen, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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

/** 通道 → 左侧色轨（完整类名，保证 Tailwind 扫描到） */
const RAIL: Record<string, string> = {
  latest: "border-l-emerald-500",
  stable: "border-l-emerald-600/70",
  rc: "border-l-sky-500",
  alpha: "border-l-amber-500",
  beta: "border-l-amber-500",
  next: "border-l-violet-500",
};

const CHANNEL_V: Record<string, "default" | "success" | "warning" | "info" | "secondary" | "outline"> = {
  latest: "success",
  stable: "success",
  rc: "info",
  alpha: "warning",
  beta: "warning",
  next: "secondary",
};

export default function VersionTableRow({
  row, isLatestTag, busy, isActive, upgradeTo, onUpgrade, onSetActive, onInstall, onUninstall, onReveal,
}: Props) {
  const inst = row.installed;
  const fmtDate = (iso: string | null) =>
    iso && !isNaN(new Date(iso).getTime()) ? new Date(iso).toISOString().slice(0, 10) : "";
  const fmtSize = (n: number | null) =>
    n == null ? "" : n > 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`;

  return (
    <tr
      className={`border-l-2 transition-colors hover:bg-muted/40 ${
        isActive
          ? `border-l-primary bg-primary/[0.05]`
          : RAIL[row.channel] ?? "border-l-transparent"
      }`}
    >
      <td className="px-4 py-2.5">
        <div className="flex items-center gap-1.5">
          <span className={`font-mono text-[13px] font-bold ${isActive ? "text-primary" : ""}`}>{row.version}</span>
          <Badge variant={CHANNEL_V[row.channel] ?? "secondary"} className="uppercase">{row.channel}</Badge>
          {isLatestTag && <Badge variant="info">latest</Badge>}
          {isActive && <Badge>当前</Badge>}
        </div>
        {upgradeTo && (
          <button
            className="mt-0.5 text-[11px] font-semibold text-amber-500 underline-offset-2 hover:underline"
            onClick={() => onUpgrade(upgradeTo)}
            title={`已安装 ${row.version}，点击安装 ${upgradeTo}`}
          >
            可升级 → {upgradeTo}
          </button>
        )}
      </td>
      <td className="px-3 py-2.5 font-mono text-[11.5px] text-muted-foreground">{fmtDate(row.remote?.publishedAt ?? null) || "—"}</td>
      <td className="px-3 py-2.5 font-mono text-[11.5px] text-muted-foreground">{fmtSize(row.remote?.unpackedSize ?? null) || "—"}</td>
      <td className="px-3 py-2.5">
        {isActive ? (
          <Badge>当前版本</Badge>
        ) : inst ? (
          <Badge variant={inst.source === "managed" ? "success" : inst.source === "global" ? "info" : "outline"}>
            {inst.source === "managed" ? "已装 · 管理" : inst.source === "global" ? "npm 全局" : "PATH"}
          </Badge>
        ) : (
          <span className="text-xs text-muted-foreground">未安装</span>
        )}
        {inst && (
          <div className="mt-0.5 max-w-[260px] truncate font-mono text-[10px] text-muted-foreground/70" title={inst.location}>
            {inst.location}
          </div>
        )}
      </td>
      <td className="whitespace-nowrap px-3 py-2.5 text-right">
        <div className="flex items-center justify-end gap-1">
          {!inst && (
            <Button size="sm" disabled={busy} onClick={() => onInstall(row.version, false)} title="安装完成后自动设为当前版本">
              安装
            </Button>
          )}
          {inst && inst.version !== "unknown" && !isActive && (
            <Button size="sm" variant="outline" disabled={busy} onClick={() => onSetActive(row.version)}>
              设为当前
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
      </td>
    </tr>
  );
}
