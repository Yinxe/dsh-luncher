import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { check as updaterCheck, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { api, events } from "./api";
import type {
  EnvironmentInfo,
  InstalledVersion,
  LauncherUpdateStatus,
  ProcEntry,
  ProcExitEvent,
  ProcLogEvent,
  ProfileInfo,
  RegistryInfo,
  Settings,
  Toast,
} from "./types";
import InstallCard from "./components/InstallCard";
import ProcessDock from "./components/ProcessDock";
import SettingsModal from "./components/SettingsModal";
import UpdateBanner from "./components/UpdateBanner";
import VersionRow, { mergeRows } from "./components/VersionRow";

let toastId = 0;

export default function App() {
  const [env, setEnv] = useState<EnvironmentInfo | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [remote, setRemote] = useState<RegistryInfo | null>(null);
  const [remoteErr, setRemoteErr] = useState<string | null>(null);
  const [remoteLoading, setRemoteLoading] = useState(false);
  const [installed, setInstalled] = useState<InstalledVersion[]>([]);
  const [installJob, setInstallJob] = useState<{ version: string; logs: string[] } | null>(null);
  const [update, setUpdate] = useState<LauncherUpdateStatus | null>(null);
  const [updateApplying, setUpdateApplying] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [query, setQuery] = useState("");
  const [channel, setChannel] = useState<string | null>(null);
  const [onlyInstalled, setOnlyInstalled] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [selectedProfile, setSelectedProfile] = useState("");
  const [procs, setProcs] = useState<Record<number, ProcEntry>>({});
  const [activeProc, setActiveProc] = useState<number | null>(null);
  const [dockOpen, setDockOpen] = useState(true);
  const [runtimeJob, setRuntimeJob] = useState<{
    received: number;
    total: number;
    log: string[];
  } | null>(null);
  const runtimeBusy = useRef(false);

  const pendingLaunch = useRef<string | null>(null);
  const profileRef = useRef<string>("");
  profileRef.current = selectedProfile;
  const procsRef = useRef<Record<number, ProcEntry>>({});
  procsRef.current = procs;
  const builtinUpdate = useRef<Update | null>(null);
  const installedRef = useRef<InstalledVersion[]>([]);
  installedRef.current = installed;

  const addToast = useCallback((kind: Toast["kind"], text: string) => {
    const id = ++toastId;
    setToasts((t) => [...t, { id, kind, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), kind === "err" ? 7000 : 4000);
  }, []);

  const refreshInstalled = useCallback(async () => {
    try {
      setInstalled(await api.listInstalled());
    } catch (e) {
      addToast("err", `扫描已安装版本失败: ${e}`);
    }
  }, [addToast]);

  const refreshRemote = useCallback(async () => {
    setRemoteLoading(true);
    setRemoteErr(null);
    try {
      setRemote(await api.listRemote());
    } catch (e) {
      setRemoteErr(String(e));
    } finally {
      setRemoteLoading(false);
    }
  }, []);

  const refreshProfiles = useCallback(async () => {
    try {
      setProfiles(await api.listProfiles());
    } catch {
      /* profile 目录不存在等情况，静默处理 */
    }
  }, []);

  // ── 启动初始化 ──────────────────────────────
  useEffect(() => {
    (async () => {
      try {
        const s = await api.getSettings();
        const [e, installedList, ps] = await Promise.all([
          api.getEnvironment(),
          api.listInstalled(),
          api.listProfiles().catch(() => [] as ProfileInfo[]),
        ]);
        setProfiles(ps);
        // 不提供“默认 profile”空选项：默认选中 web，其次取第一个
        const def =
          s.defaultProfile ||
          (ps.some((p) => p.name === "web") ? "web" : (ps[0]?.name ?? ""));
        setSelectedProfile(def);
        if (s.defaultProfile !== def) {
          const next = { ...s, defaultProfile: def };
          setSettings(next);
          api.saveSettings(next).catch(() => undefined);
        } else {
          setSettings(s);
        }
        setEnv(e);
        setInstalled(installedList);
        if (s.autoCheckVersions) refreshRemote();
        if (s.autoCheckUpdate) {
          api
            .checkLauncherUpdate()
            .then((u) => {
              if (u.available || u.mode === "error") setUpdate(u);
            })
            .catch(() => undefined);
        }
      } catch (e) {
        addToast("err", `初始化失败: ${e}`);
      }
      try {
        const running = await api.installRunning();
        if (running) setInstallJob({ version: running, logs: [] });
      } catch {
        /* ignore */
      }
    })();
  }, [refreshRemote, refreshProfiles, addToast]);

  // ── 事件订阅 ────────────────────────────────
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    const track = (p?: Promise<() => void>) =>
      p?.then((u) => unlisteners.push(u)).catch(() => undefined);
    track(
      events.onInstallLog?.((e) => {
        setInstallJob((job) =>
          job && job.version === e.version
            ? { ...job, logs: [...job.logs.slice(-400), e.line] }
            : job
        );
      })
    );
    track(
      events.onInstallFinished?.(async (e) => {
        setInstallJob((job) => (job && job.version === e.version ? null : job));
        if (e.success) {
          addToast("ok", `dsh ${e.version} 安装完成`);
          await refreshInstalled();
          if (pendingLaunch.current === e.version) {
            pendingLaunch.current = null;
            doLaunch(e.version);
          }
        } else {
          addToast("err", `dsh ${e.version} 安装失败：${e.message.split("\n")[0]}`);
        }
      })
    );
    track(events.onLauncherUpdate?.((s) => setUpdate(s)));
    track(
      events.onProcLog?.((e: ProcLogEvent) => {
        setProcs((m) => {
          const old =
            m[e.id] ??
            ({
              id: e.id,
              version: e.version,
              profile: e.profile,
              startedAt: Date.now(),
              lines: [],
              exited: false,
              code: null,
            } as ProcEntry);
          return {
            ...m,
            [e.id]: { ...old, lines: [...old.lines.slice(-500), e.line] },
          };
        });
        setActiveProc((a) => a ?? e.id);
      })
    );
    track(
      events.onProcExit?.((e: ProcExitEvent) => {
        setProcs((m) => {
          const old = m[e.id];
          if (!old) return m;
          return { ...m, [e.id]: { ...old, exited: true, code: e.code } };
        });
        const tag = e.profile ? ` (${e.profile})` : "";
        if (e.code == null) {
          addToast("info", `dsh ${e.version}${tag} 已停止`);
        } else if (e.code === 0) {
          addToast("ok", `dsh ${e.version}${tag} 正常退出`);
        } else {
          addToast("err", `dsh ${e.version}${tag} 已退出，退出码 ${e.code}`);
        }
      })
    );
    track(events.onRuntimeLog?.((line) =>
      setRuntimeJob((j) => (j ? { ...j, log: [...j.log.slice(-20), line] } : j))
    ));
    track(
      events.onRuntimeProgress?.((e) =>
        setRuntimeJob((j) => (j ? { ...j, received: e.received, total: e.total } : j))
      )
    );
    track(
      events.onRuntimeFinished?.((e) => {
        addToast(e.ok ? "ok" : "err", e.message);
        if (!e.ok) setRuntimeJob(null);
      })
    );
    track(events.onToast?.((text) => addToast("err", text)));
    return () => {
      for (const u of unlisteners) u();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [addToast, refreshInstalled]);

  // ── 动作 ───────────────────────────────────
  const doInstall = useCallback(
    async (version: string, force: boolean) => {
      // 先乐观建任务，保证最早的日志事件有落点
      setInstallJob((job) => (job ? job : { version, logs: [] }));
      try {
        await api.install(version, force);
      } catch (e) {
        setInstallJob(null);
        addToast("err", `安装失败: ${e}`);
      }
    },
    [addToast]
  );

  const doLaunch = useCallback(
    async (version: string) => {
      try {
        const info = await api.startEmbedded(version, profileRef.current);
        setProcs((m) => ({
          ...m,
          [info.id]: {
            id: info.id,
            version: info.version,
            profile: info.profile,
            startedAt: info.startedAt,
            lines: [],
            exited: false,
            code: null,
          },
        }));
        setActiveProc(info.id);
        setDockOpen(true);
        addToast("ok", `dsh ${info.version} 已内嵌启动（PID ${info.id}）`);
      } catch (e) {
        addToast("err", `启动失败: ${e}`);
      }
    },
    [addToast]
  );

  const doTerminal = useCallback(
    async (version: string) => {
      try {
        const r = await api.launch(version, null, profileRef.current);
        addToast(r.ok ? "ok" : "err", r.message);
      } catch (e) {
        addToast("err", `启动失败: ${e}`);
      }
    },
    [addToast]
  );

  const doStopProc = useCallback(
    async (id: number) => {
      try {
        await api.stopProcess(id);
        addToast("info", `已请求停止进程 ${id}`);
      } catch (e) {
        addToast("err", `停止失败: ${e}`);
      }
    },
    [addToast]
  );

  const doInstallRuntime = useCallback(async () => {
    if (runtimeBusy.current) return;
    runtimeBusy.current = true;
    setRuntimeJob({ received: 0, total: 0, log: [] });
    try {
      const msg = await api.installRuntime();
      addToast("ok", msg);
      setEnv(await api.getEnvironment());
    } catch (e) {
      addToast("err", String(e));
    } finally {
      runtimeBusy.current = false;
      setRuntimeJob(null);
    }
  }, [addToast]);

  const changeProfile = useCallback(
    async (name: string) => {
      setSelectedProfile(name);
      if (!settings) return;
      const next = { ...settings, defaultProfile: name };
      setSettings(next);
      try {
        await api.saveSettings(next);
      } catch (e) {
        addToast("err", `保存默认 profile 失败: ${e}`);
      }
    },
    [settings, addToast]
  );

  const doUninstall = useCallback(
    async (version: string) => {
      if (!window.confirm(`确定卸载 dsh ${version}？将删除其安装目录。`)) return;
      try {
        await api.uninstall(version);
        addToast("ok", `dsh ${version} 已卸载`);
        await refreshInstalled();
      } catch (e) {
        addToast("err", `卸载失败: ${e}`);
      }
    },
    [addToast, refreshInstalled]
  );

  const doCancelInstall = useCallback(async () => {
    try {
      await api.cancelInstall();
      addToast("info", "已请求取消安装");
    } catch (e) {
      addToast("err", `取消失败: ${e}`);
    }
  }, [addToast]);

  const doCheckUpdate = useCallback(async () => {
    try {
      let status = await api.checkLauncherUpdate();
      // 未配置清单时，尝试内置 Tauri updater（正式发布版才会配置 endpoints）
      if (status.mode === "unconfigured") {
        try {
          const u = await updaterCheck();
          builtinUpdate.current = u;
          if (u) {
            status = {
              available: true,
              current: status.current,
              latest: u.version,
              notes: u.body ?? null,
              url: null,
              mode: "builtin",
              message: `发现新版本 ${u.version}`,
            };
          }
        } catch {
          /* 未配置 updater endpoints，属正常 */
        }
      }
      setUpdate(status);
      if (!status.available && status.mode !== "error") {
        addToast("ok", status.message ?? "已是最新版本");
      }
    } catch (e) {
      addToast("err", `检查更新失败: ${e}`);
    }
  }, [addToast]);

  const doApplyUpdate = useCallback(async () => {
    const u = builtinUpdate.current;
    if (!u) return;
    setUpdateApplying(true);
    try {
      await u.downloadAndInstall();
      await relaunch();
    } catch (e) {
      setUpdateApplying(false);
      addToast("err", `自动更新失败: ${e}`);
    }
  }, [addToast]);

  const doSaveSettings = useCallback(
    async (s: Settings) => {
      try {
        await api.saveSettings(s);
        setSettings(s);
        setShowSettings(false);
        addToast("ok", "设置已保存");
        const e = await api.getEnvironment();
        setEnv(e);
      } catch (err) {
        addToast("err", `保存设置失败: ${err}`);
      }
    },
    [addToast]
  );

  // ── 派生数据 ───────────────────────────────
  const rows = useMemo(
    () => mergeRows(remote?.versions ?? [], installed),
    [remote, installed]
  );

  const channels = useMemo(() => {
    const set = new Set<string>();
    for (const r of rows) set.add(r.channel);
    return [...set].sort();
  }, [rows]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return rows.filter((r) => {
      if (channel && r.channel !== channel) return false;
      if (onlyInstalled && !r.installed) return false;
      if (q && !r.version.toLowerCase().includes(q)) return false;
      return true;
    });
  }, [rows, query, channel, onlyInstalled]);

  const latestVersion = remote?.tags?.latest;

  const busyProfiles = useMemo(() => {
    const s = new Set<string>();
    for (const p of Object.values(procs)) {
      if (!p.exited && p.profile) s.add(p.profile);
    }
    return s;
  }, [procs]);
  const profileBusy = selectedProfile !== "" && busyProfiles.has(selectedProfile);

  if (!settings || !env) {
    return (
      <div className="layout">
        <div className="empty">
          <div className="spinner" />
          <div>正在加载…</div>
        </div>
      </div>
    );
  }

  return (
    <div className="layout">
      {/* 顶栏 */}
      <div className="topbar">
        <div className="brand">
          <div className="logo">DSH</div>
          <div>
            DSH Launcher
            <div className="sub">@deepseek-ai/dsh 版本管理器 · v{env.appVersion}</div>
          </div>
        </div>
        <div className="chips">
          <span className="chip" title={env.nodePath ?? ""}>
            <span className={`dot${env.node ? " on" : ""}`} />
            Node {env.node ? `v${env.node}` : "未检测到"}
          </span>
          <span className="chip" title={env.npmPath ?? ""}>
            <span className={`dot${env.npm ? " on" : ""}`} />
            npm {env.npm ?? "未检测到"}
          </span>
          <span className="chip">
            {env.os}/{env.arch}
          </span>
          <select
            className="prof-select"
            value={selectedProfile}
            title={`Profile 目录：${env.profilesDir}\n点击可重新扫描；每个 profile 同时只能运行一个实例`}
            onClick={() => refreshProfiles()}
            onChange={(e) => changeProfile(e.target.value)}
          >
            {profiles.length === 0 && <option value="" disabled>（未找到 profile）</option>}
            {selectedProfile && !profiles.some((p) => p.name === selectedProfile) && (
              <option value={selectedProfile}>{selectedProfile}（目录中已不存在）</option>
            )}
            {profiles.map((p) => (
              <option key={p.path} value={p.name}>
                {p.name}
              </option>
            ))}
          </select>
        </div>
        <button disabled={remoteLoading} onClick={refreshRemote}>
          {remoteLoading ? "刷新中…" : "刷新版本"}
        </button>
        <button onClick={doCheckUpdate}>检查更新</button>
        <button className="ghost" onClick={() => setShowSettings(true)} title="设置">
          ⚙
        </button>
      </div>

      {update && (
        <UpdateBanner
          status={update}
          applying={updateApplying}
          onDismiss={() => setUpdate(null)}
          onOpenUrl={(u) => api.openUrl(u).catch((e) => addToast("err", String(e)))}
          onApply={doApplyUpdate}
        />
      )}

      {env && !env.node && (
        <div className="banner err">
          {runtimeJob ? (
            <>
              <span>
                正在安装内置 Node…
                {runtimeJob.total > 0 &&
                  ` ${Math.round((runtimeJob.received / runtimeJob.total) * 100)}% (${(
                    runtimeJob.received / 1048576
                  ).toFixed(1)}/${(runtimeJob.total / 1048576).toFixed(1)} MB)`}
              </span>
              <span className="grow" />
              <span className="mono" style={{ fontSize: 11 }}>
                {runtimeJob.log[runtimeJob.log.length - 1] ?? "连接镜像站…"}
              </span>
            </>
          ) : (
            <>
              <span>
                ⚠ 未检测到 Node.js（dsh 依赖 Node 运行）。可一键安装启动器内置 Node
                LTS（用户级安装，无需 root；下载默认走 npmmirror 镜像）
              </span>
              <span className="grow" />
              <button className="primary sm" onClick={doInstallRuntime}>
                一键安装 Node
              </button>
            </>
          )}
        </div>
      )}

      {/* 主区 */}
      <div className="main">
        <div className="filters">
          <h3>搜索</h3>
          <input
            type="text"
            placeholder="版本号，如 0.1.5"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <h3>通道</h3>
          <div className="tag-group">
            <button className={`tag${channel === null ? " active" : ""}`} onClick={() => setChannel(null)}>
              全部
            </button>
            {channels.map((c) => (
              <button
                key={c}
                className={`tag${channel === c ? " active" : ""}`}
                onClick={() => setChannel(channel === c ? null : c)}
              >
                {c}
              </button>
            ))}
          </div>
          <h3>筛选</h3>
          <div className="tag-group">
            <button
              className={`tag${onlyInstalled ? " active" : ""}`}
              onClick={() => setOnlyInstalled((v) => !v)}
            >
              仅已安装
            </button>
          </div>
          <div className="meta">
            <div>官方版本 {remote?.versions.length ?? "—"} 个</div>
            <div>已安装 {installed.length} 个</div>
            <div>
              profile {profiles.length} 个（{env.dshNativeHome}/profile）
            </div>
            <div>registry：{env.registry}</div>
            <div>数据目录：{env.versionsDir}</div>
          </div>
        </div>

        <div className="list">
          {installJob && (
            <InstallCard version={installJob.version} logs={installJob.logs} onCancel={doCancelInstall} />
          )}

          {remoteErr && !remoteLoading && (
            <div className="empty err">
              <div className="big">⚠</div>
              <div>拉取官方版本列表失败：{remoteErr}</div>
              <button onClick={refreshRemote}>重试</button>
            </div>
          )}

          {!remoteErr && filtered.length === 0 && !remoteLoading && (
            <div className="empty">
              <div className="big">📦</div>
              <div>
                {rows.length === 0
                  ? "还没有版本数据，点击“刷新版本”从 registry 拉取 @deepseek-ai/dsh 的官方发布版本"
                  : "没有符合筛选条件的版本"}
              </div>
            </div>
          )}

          {filtered.map((r) => (
            <VersionRow
              key={`${r.version}-${r.installed?.source ?? "remote"}`}
              row={r}
              isLatestTag={latestVersion === r.version}
              busy={installJob !== null}
              runningProcs={Object.values(procs).filter(
                (p) => p.version === r.version && !p.exited
              )}
              profileBusy={profileBusy}
              onLaunch={doLaunch}
              onTerminal={doTerminal}
              onShowProc={(id) => {
                setActiveProc(id);
                setDockOpen(true);
              }}
              onStopProc={doStopProc}
              onInstall={(v) => {
                pendingLaunch.current = null;
                doInstall(v, false);
              }}
              onUninstall={doUninstall}
              onReveal={(p) => api.reveal(p).catch((e) => addToast("err", String(e)))}
            />
          ))}
        </div>
      </div>

      <ProcessDock
        procs={Object.values(procs).sort((a, b) => a.id - b.id)}
        activeId={activeProc}
        open={dockOpen}
        onToggle={() => setDockOpen((v) => !v)}
        onSelect={setActiveProc}
        onStop={doStopProc}
        onClearExited={() => {
          const next = Object.fromEntries(
            Object.entries(procsRef.current).filter(([, p]) => !p.exited)
          ) as Record<number, ProcEntry>;
          setProcs(next);
          setActiveProc((a) =>
            a != null && next[a] ? a : (Object.values(next)[0]?.id ?? null)
          );
        }}
      />

      <div className="statusbar">
        <span>官方源 npm:{env.registry}</span>
        <span className="mono">dsh 数据目录 {env.dshHome}</span>
        <span style={{ marginLeft: "auto" }}>
          「启动」为内嵌运行：退出启动器会结束所有 dsh 进程；关闭窗口最小化到托盘
        </span>
      </div>

      {showSettings && (
        <SettingsModal
          initial={settings}
          env={env}
          onSave={doSaveSettings}
          onClose={() => setShowSettings(false)}
          onReveal={(p) => api.reveal(p).catch((e) => addToast("err", String(e)))}
        />
      )}

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
