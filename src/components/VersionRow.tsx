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

export function compareVersions(a: string, b: string): number {
  const core = (v: string) =>
    v
      .replace(/^v/, "")
      .split("-")[0]
      .split(".")
      .map((x) => parseInt(x, 10) || 0);
  const pre = (v: string) => (v.includes("-") ? v.split("-").slice(1).join("-") : "");
  const ca = core(a);
  const cb = core(b);
  const n = Math.max(ca.length, cb.length);
  for (let i = 0; i < n; i++) {
    const x = ca[i] ?? 0;
    const y = cb[i] ?? 0;
    if (x !== y) return y - x;
  }
  const pa = pre(a);
  const pb = pre(b);
  if (!pa && !pb) return 0;
  if (!pa) return -1;
  if (!pb) return 1;
  return pb.localeCompare(pa);
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
  return [...rows.values()].sort((a, b) => compareVersions(a.version, b.version));
}

interface Props {
  row: MergedRow;
  isLatestTag: boolean;
  busy: boolean;
  runningProcs: ProcEntry[];
  onLaunch: (version: string) => void;
  onTerminal: (version: string) => void;
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
  onLaunch,
  onTerminal,
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
        </div>
        <div className="d">
          {[
            row.remote ? formatDate(row.remote.publishedAt) : null,
            row.remote ? formatSize(row.remote.unpackedSize) : null,
          ]
            .filter(Boolean)
            .join(" · ")}
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
          <button
            className="primary"
            onClick={() => onLaunch(row.version)}
            title="内嵌启动：进程随启动器生命周期，日志在下方面板查看"
          >
            启动
          </button>
        )}
        {!inst && (
          <button
            className="primary"
            disabled={busy}
            onClick={() => onInstall(row.version, false)}
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
              disabled={busy}
              onClick={() => onInstall(row.version, true)}
              title="删除后重新下载安装"
            >
              重装
            </button>
            <button
              className="sm danger"
              disabled={busy}
              onClick={() => onUninstall(row.version)}
            >
              卸载
            </button>
          </>
        )}
        <button
          className="sm ghost"
          onClick={() => onTerminal(row.version)}
          title="在独立系统终端窗口中启动（不受启动器生命周期管理，可交互）"
        >
          终端
        </button>
      </div>
    </div>
  );
}
