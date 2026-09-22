import { FolderOpen, MoreHorizontal, RefreshCw, ScrollText, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { TableCell, TableRow } from "@/components/ui/table";
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
  /** 有 Profile 实例运行中：锁定「设为当前」（实例都基于当前版本运行） */
  switchLocked: boolean;
  upgradeTo: string | null;
  onUpgrade: (target: string) => void;
  onSetActive: (version: string) => void;
  onInstall: (version: string, force: boolean) => void;
  onUninstall: (version: string) => void;
  onReveal: (path: string) => void;
  /** 打开「这个版本改了什么」（官方 Release 正文） */
  onShowNotes: (version: string) => void;
}

/** 通道 → 左侧色轨（完整类名，保证 Tailwind 扫描到） */
const RAIL: Record<string, string> = {
  latest: "border-l-emerald-500",
  stable: "border-l-emerald-600/70",
  rc: "border-l-teal-500",
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

/**
 * 宽屏阈值（表格外层 `@container` 的宽度）选 52rem 的理由：
 * 实测四个次要操作 +「设为当前」加起来约 560px，再加上版本 / 日期 / 大小 / 状态
 * 四列就得 830px 以上才排得下 —— 低于这个宽度与其让按钮换行把行撑高，
 * 不如收进「更多」菜单（主操作始终留在外面）。
 *
 * 注意：`@[52rem]:flex` / `@[52rem]:hidden` 必须**字面量**写在 className 里。
 * Tailwind 只扫描源码里的完整类名，用常量拼接（`${WIDE}:hidden`）不会生成对应 CSS ——
 * 那样两个操作组会同时显示（曾经真踩过），所以这里不再抽常量。
 */

export default function VersionTableRow({
  row, isLatestTag, busy, isActive, switchLocked, upgradeTo, onUpgrade, onSetActive, onInstall, onUninstall, onReveal,
  onShowNotes,
}: Props) {
  const inst = row.installed;
  const fmtDate = (iso: string | null) =>
    iso && !isNaN(new Date(iso).getTime()) ? new Date(iso).toISOString().slice(0, 10) : "";
  const fmtSize = (n: number | null) =>
    n == null ? "" : n > 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`;

  const canReinstall = !!inst && inst.source === "managed" && !busy && !isActive;
  const canUninstall = canReinstall;

  return (
    <TableRow
      className={`border-l-2 hover:bg-muted/40 ${
        isActive
          ? `border-l-primary bg-primary/[0.05]`
          : RAIL[row.channel] ?? "border-l-transparent"
      }`}
    >
      {/* 版本：窄容器里让徽标换到第二行，但版本号本身不截断（截断的版本号没有意义） */}
      <TableCell className="px-3 py-2.5 sm:pl-4">
        <div className="flex flex-wrap items-center gap-x-1.5 gap-y-1">
          <span className={`font-mono text-[13px] font-bold whitespace-nowrap ${isActive ? "text-primary" : ""}`}>
            {row.version}
          </span>
          <Badge variant={CHANNEL_V[row.channel] ?? "secondary"} className="uppercase">{row.channel}</Badge>
          {/* channel 已经是 latest 时不再重复挂一个 latest 徽标（同一行里两个同义徽标） */}
          {isLatestTag && row.channel !== "latest" && <Badge variant="info">latest</Badge>}
          {isActive && <Badge>当前</Badge>}
        </div>
        {upgradeTo && (
          <button
            className="mt-0.5 text-[11px] font-semibold text-amber-500 underline-offset-2 hover:underline disabled:pointer-events-none disabled:opacity-50"
            disabled={busy}
            onClick={() => onUpgrade(upgradeTo)}
            title={`已安装 ${row.version}，点击安装 ${upgradeTo}`}
          >
            可升级 → {upgradeTo}
          </button>
        )}
      </TableCell>

      {/* 发布日期 / 大小：按容器宽度逐级收起，它们的优先级最低 */}
      <TableCell className="hidden px-3 py-2.5 font-mono text-[11.5px] whitespace-nowrap text-muted-foreground @[34rem]:table-cell">
        {fmtDate(row.remote?.publishedAt ?? null) || "—"}
      </TableCell>
      <TableCell className="hidden px-3 py-2.5 font-mono text-[11.5px] whitespace-nowrap text-muted-foreground @[40rem]:table-cell">
        {fmtSize(row.remote?.unpackedSize ?? null) || "—"}
      </TableCell>

      <TableCell className="px-3 py-2.5">
        {isActive ? (
          <Badge title={inst?.location}>当前版本</Badge>
        ) : inst ? (
          <Badge
            variant={inst.source === "managed" ? "success" : inst.source === "global" ? "info" : "outline"}
            title={inst.location}
          >
            {inst.source === "managed" ? "已装 · 管理" : inst.source === "global" ? "npm 全局" : "PATH"}
          </Badge>
        ) : (
          <span className="text-xs whitespace-nowrap text-muted-foreground">未安装</span>
        )}
      </TableCell>

      {/* 操作：主操作常驻 + 次要操作（宽屏铺开 / 窄屏收进「更多」）。
          容器查询而不是视口断点：侧栏收放会改掉这里真正可用的宽度。 */}
      <TableCell className="px-3 py-2.5 text-right sm:pr-4">
        <div className="flex flex-nowrap items-center justify-end gap-1">
          {!inst && (
            <Button size="sm" disabled={busy} onClick={() => onInstall(row.version, false)} title="安装完成后自动设为当前版本">
              安装
            </Button>
          )}
          {inst && inst.version !== "unknown" && !isActive && (
            <Button
              size="sm"
              variant="outline"
              disabled={busy || switchLocked}
              title={switchLocked ? "有 Profile 实例正在运行，停止所有实例后才能切换版本" : "设为当前版本"}
              onClick={() => onSetActive(row.version)}
            >
              设为当前
            </Button>
          )}

          <div className="hidden items-center gap-0.5 @[52rem]:flex">
            <Button
              size="sm"
              variant="ghost"
              title={`查看 dsh ${row.version} 改了什么（官方 Release 正文）`}
              onClick={() => onShowNotes(row.version)}
            >
              <ScrollText />
            </Button>
            {inst && inst.source === "managed" && (
              <>
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => onReveal(inst.location)} title="打开安装目录">
                  <FolderOpen />
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={!canReinstall}
                  onClick={() => onInstall(row.version, true)}
                  title={isActive ? "当前版本正在使用，请先切换到其他版本再重装" : "删除后重新下载安装（会设为当前版本）"}
                >
                  <RefreshCw /> 重装
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                  disabled={!canUninstall}
                  onClick={() => onUninstall(row.version)}
                  title={isActive ? "当前版本不允许卸载，请先切换到其他版本" : undefined}
                >
                  卸载
                </Button>
              </>
            )}
          </div>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="sm" variant="ghost" className="@[52rem]:hidden" title="更多操作">
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-44">
              <DropdownMenuItem onSelect={() => onShowNotes(row.version)}>
                <ScrollText /> 更新日志
              </DropdownMenuItem>
              {inst && inst.source === "managed" && (
                <>
                  <DropdownMenuItem onSelect={() => onReveal(inst.location)}>
                    <FolderOpen /> 打开安装目录
                  </DropdownMenuItem>
                  <DropdownMenuItem disabled={!canReinstall} onSelect={() => onInstall(row.version, true)}>
                    <RefreshCw /> 重装
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem variant="destructive" disabled={!canUninstall} onSelect={() => onUninstall(row.version)}>
                    <Trash2 /> 卸载
                  </DropdownMenuItem>
                </>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </TableCell>
    </TableRow>
  );
}
