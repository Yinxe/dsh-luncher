import type { InstalledVersion, ProcEntry, RemoteVersion } from "../types";

export function formatSize(n: number | null): string {
  if (n == null) return "";
  if (n > 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(n / 1024))} KB`;
}

export function formatDate(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
    d.getDate()
  ).padStart(2, "0")}`;
}

export function deriveChannel(version: string): string {
  const pre = version.includes("-") ? version.split("-").slice(1).join("-") : "";
  if (pre.startsWith("alpha")) return "alpha";
  if (pre.startsWith("beta")) return "beta";
  if (pre.startsWith("rc")) return "rc";
  return "stable";
}

/**
 * 标准语义比较：a > b 返回正数，a < b 返回负数，相等返回 0。
 * 覆盖 0.1.5-rc.2 / 0.1.6-alpha.1 等格式，预发布段按 semver 规则
 * （无预发布 > 有预发布；数字段 < 字母段；数字段按数值比较）。
 */
export function compareVersions(a: string, b: string): number {
  const parse = (v: string) => {
    const clean = v.trim().replace(/^v/, "");
    const idx = clean.indexOf("-");
    const core = (idx === -1 ? clean : clean.slice(0, idx))
      .split(".")
      .map((x) => parseInt(x, 10) || 0);
    const pre = idx === -1 ? [] : clean.slice(idx + 1).split(".");
    return { core, pre };
  };
  const pa = parse(a);
  const pb = parse(b);

  const n = Math.max(pa.core.length, pb.core.length);
  for (let i = 0; i < n; i++) {
    const x = pa.core[i] ?? 0;
    const y = pb.core[i] ?? 0;
    if (x !== y) return x - y;
  }

  if (pa.pre.length === 0 && pb.pre.length === 0) return 0;
  if (pa.pre.length === 0) return 1;
  if (pb.pre.length === 0) return -1;

  const m = Math.max(pa.pre.length, pb.pre.length);
  for (let i = 0; i < m; i++) {
    const xs = pa.pre[i];
    const ys = pb.pre[i];
    if (xs === undefined) return -1;
    if (ys === undefined) return 1;
    const xn = /^\d+$/.test(xs) ? parseInt(xs, 10) : null;
    const yn = /^\d+$/.test(ys) ? parseInt(ys, 10) : null;
    if (xn !== null && yn !== null) {
      if (xn !== yn) return xn - yn;
    } else if (xn !== null) {
      return -1;
    } else if (yn !== null) {
      return 1;
    } else if (xs !== ys) {
      return xs < ys ? -1 : 1;
    }
  }
  return 0;
}

export interface MergedRow {
  version: string;
  remote: RemoteVersion | null;
  installed: InstalledVersion | null;
  channel: string;
  tags: string[];
}

export function mergeRows(
  remote: RemoteVersion[],
  installed: InstalledVersion[]
): MergedRow[] {
  const rows = new Map<string, MergedRow>();
  for (const r of remote) {
    rows.set(r.version, {
      version: r.version,
      remote: r,
      installed: null,
      channel: r.channel || deriveChannel(r.version),
      tags: r.tags,
    });
  }
  for (const i of installed) {
    const exist = rows.get(i.version);
    if (exist) {
      exist.installed = i;
    } else if (i.version !== "unknown") {
      rows.set(i.version, {
        version: i.version,
        remote: null,
        installed: i,
        channel: deriveChannel(i.version),
        tags: [],
      });
    }
  }
  return [...rows.values()].sort((a, b) => compareVersions(b.version, a.version));
}

interface Props {
  row: MergedRow;
  isLatestTag: boolean;
  busy: boolean;
  runningProcs: ProcEntry[];
  /** 该版本是否为全局「当前版本」 */
  isActive: boolean;
  /** 当前选中的 profile 已有实例在运行 */
  profileBusy: boolean;
  /** 已安装版本低于官方 latest 时的目标版本 */
  upgradeTo: string | null;
  onUpgrade: (target: string) => void;
  onLaunch: (version: string) => void;
  onTerminal: (version: string) => void;
  onSetActive: (version: string) => void;
  onShowProc: (id: number) => void;
  onStopProc: (id: number) => void;
  onInstall: (version: string, force: boolean) => void;
  onUninstall: (version: string) => void;
  onReveal: (path: string) => void;
}

export default function VersionRow({
  row,
  isLatestTag,
  busy,
  runningProcs,
  isActive,
  profileBusy,
  upgradeTo,
  onUpgrade,
  onLaunch,
  onTerminal,
  onSetActive,
  onShowProc,
  onStopProc,
  onInstall,
  onUninstall,
  onReveal,
}: Props) {
  const inst = row.installed;
  return (
    <div className={`vrow${isLatestTag ? " latest-pin" : ""}`}>
      <div className="vmain">
        <div className="v">
          <span>{row.version}</span>
          <span className={`badge ${row.channel}`}>{row.channel}</span>
          {isLatestTag && <span className="badge latest">latest</span>}
          {isActive && <span className="badge installed">当前版本</span>}
        </div>
        <div className="d">
          {[
            row.remote ? formatDate(row.remote.publishedAt) : null,
            row.remote ? formatSize(row.remote.unpackedSize) : null,
          ]
            .filter(Boolean)
            .join(" · ")}
          {upgradeTo && (
            <>
              {" · "}
              <button
                className="badge upgrade"
                onClick={() => onUpgrade(upgradeTo)}
                title={`已安装 ${row.version}，点击安装 ${upgradeTo}`}
              >
                可升级 → {upgradeTo}
              </button>
            </>
          )}
        </div>
      </div>

      <div className="vdesc">
        {inst ? (
          <>
            <span
              className={`badge ${
                inst.source === "managed" ? "installed" : inst.source
              }`}
            >
              {inst.source === "managed"
                ? "已装 · 启动器管理"
                : inst.source === "global"
                ? "已装 · npm 全局"
                : "已装 · PATH"}
            </span>
            <div className="loc" title={inst.location}>
              {inst.location}
            </div>
          </>
        ) : (
          <span>{row.remote?.description ?? "未安装"}</span>
        )}
      </div>

      <div className="vactions">
        {inst && inst.version !== "unknown" && (
          isActive ? (
            <button
              className="primary"
              disabled={profileBusy}
              onClick={() => onLaunch(row.version)}
              title={
                profileBusy
                  ? "选中的 profile 已有实例在运行，每个 profile 同时只能运行一个"
                  : "以内嵌方式启动当前版本（进程随启动器生命周期，日志在下方面板查看）"
              }
            >
              启动
            </button>
          ) : (
            <button
              className="sm"
              disabled={busy}
              onClick={() => onSetActive(row.version)}
              title="设为当前版本：所有 Profile 实例将基于该版本启动"
            >
              设为当前
            </button>
          )
        )}
        {!inst && (
          <button
            className="primary"
            disabled={busy}
            onClick={() => onInstall(row.version, false)}
            title="安装完成后自动设为当前版本"
          >
            安装
          </button>
        )}
        {runningProcs.length > 0 && (
          <>
            {runningProcs.map((p) => (
              <button
                key={p.id}
                className="sm run-chip"
                onClick={() => onShowProc(p.id)}
                title={`PID ${p.id} · 查看日志`}
              >
                <span className="dot ok" /> {p.id}
              </button>
            ))}
            {runningProcs.length > 0 && (
              <button
                className="sm danger"
                onClick={() => runningProcs.forEach((p) => onStopProc(p.id))}
                title="停止该版本的所有内嵌进程"
              >
                停止
              </button>
            )}
          </>
        )}
        {inst && inst.source === "managed" && (
          <>
            <button className="sm" disabled={busy} onClick={() => onReveal(inst.location)}>
              目录
            </button>
            <button
              className="sm"
              disabled={busy || runningProcs.length > 0}
              onClick={() => onInstall(row.version, true)}
              title={
                runningProcs.length > 0
                  ? "该版本正在运行，请先停止再重装"
                  : "删除后重新下载安装"
              }
            >
              重装
            </button>
            <button
              className="sm danger"
              disabled={busy || runningProcs.length > 0}
              onClick={() => onUninstall(row.version)}
              title={
                runningProcs.length > 0 ? "该版本正在运行，请先停止再卸载" : undefined
              }
            >
              卸载
            </button>
          </>
        )}
        <button
          className="sm ghost"
          disabled={profileBusy || !isActive}
          onClick={() => onTerminal(row.version)}
          title={
            !isActive
              ? "仅当前版本可启动；先点「设为当前」"
              : profileBusy
              ? "该 profile 已有实例在运行，每个 profile 同时只能运行一个"
              : "在独立系统终端窗口中启动（不受启动器生命周期管理，可交互）"
          }
        >
          终端
        </button>
      </div>
    </div>
  );
}
