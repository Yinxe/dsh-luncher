import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { check as updaterCheck, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  Package, Rocket, Puzzle, FileCog, RefreshCw, Settings as SettingsIcon,
  ExternalLink, Play, Square, CheckCircle2, XCircle, Loader2, Sun, Moon,
} from "lucide-react";
import { api, events } from "./api";
import { useTheme } from "@/lib/theme";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import InstallCard from "./components/InstallCard";
import ConfigView from "./components/ConfigView";
import PluginsView from "./components/PluginsView";
import ProcessDock from "./components/ProcessDock";
import VersionRow from "./components/VersionRow";
import SettingsModal from "./components/SettingsModal";
import UpdateBanner from "./components/UpdateBanner";
import type {
  EnvironmentInfo, InstalledVersion, LauncherUpdateStatus, ProcEntry,
  ProcExitEvent, ProcLogEvent, ProfileInfo, ProfileInstance, RegistryInfo,
  Settings as SettingsT, Toast,
} from "./types";

let toastId = 0;
type View = "versions" | "profiles" | "plugins" | "config";

export default function App() {
  const [env, setEnv] = useState<EnvironmentInfo | null>(null);
  const [settings, setSettings] = useState<SettingsT | null>(null);
  const [remote, setRemote] = useState<RegistryInfo | null>(null);
  const [remoteErr, setRemoteErr] = useState<string | null>(null);
  const [remoteLoading, setRemoteLoading] = useState(false);
  const [installed, setInstalled] = useState<InstalledVersion[]>([]);
  const [installJob, setInstallJob] = useState<{ version: string; logs: string[] } | null>(null);
  const [update, setUpdate] = useState<LauncherUpdateStatus | null>(null);
  const [updateApplying, setUpdateApplying] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [showSettings, setShowSettings] = useState(false);
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [procs, setProcs] = useState<Record<number, ProcEntry>>({});
  const [activeProc, setActiveProc] = useState<number | null>(null);
  const [dockOpen, setDockOpen] = useState(true);
  const [runtimeJob, setRuntimeJob] = useState<{ received: number; total: number; log: string[] } | null>(null);
  const runtimeBusy = useRef(false);
  const [instances, setInstances] = useState<ProfileInstance[]>([]);
  const [view, setView] = useState<View>("versions");

  const procsRef = useRef<Record<number, ProcEntry>>({});
  procsRef.current = procs;
  const settingsRef = useRef<SettingsT | null>(null);
  settingsRef.current = settings;
  const builtinUpdate = useRef<Update | null>(null);

  const { resolved, setTheme } = useTheme();

  const addToast = useCallback((kind: Toast["kind"], text: string) => {
    const id = ++toastId;
    setToasts((t) => [...t, { id, kind, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), kind === "err" ? 7000 : 4000);
  }, []);

  const refreshInstalled = useCallback(async () => {
    try { setInstalled(await api.listInstalled()); }
    catch (e) { addToast("err", `扫描已安装版本失败: ${e}`); }
  }, [addToast]);

  const refreshRemote = useCallback(async () => {
    setRemoteLoading(true);
    setRemoteErr(null);
    try { setRemote(await api.listRemote()); }
    catch (e) { setRemoteErr(String(e)); }
    finally { setRemoteLoading(false); }
  }, []);

  const refreshProfiles = useCallback(async () => {
    try { setProfiles(await api.listProfiles()); } catch { /* 目录不存在等 */ }
  }, []);

  const refreshInstances = useCallback(async () => {
    try { setInstances(await api.listProfileInstances()); } catch { /* ignore */ }
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
        let next = s;
        if (!s.defaultProfile && ps.length > 0) {
          next = { ...s, defaultProfile: ps.some((p) => p.name === "web") ? "web" : ps[0].name };
        }
        if (!next.activeVersion && installedList.length > 0) {
          const best = [...installedList]
            .filter((i) => i.version !== "unknown")
            .sort((a, b) => {
              const cmp = (x: string, y: string) => {
                const pa = x.replace(/^v/, "").split("-")[0].split(".").map(Number);
                const pb = y.replace(/^v/, "").split("-")[0].split(".").map(Number);
                for (let i = 0; i < 3; i++) if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pb[i] ?? 0) - (pa[i] ?? 0);
                return y.includes("-") ? -1 : x.includes("-") ? 1 : 0;
              };
              return cmp(a.version, b.version);
            })[0];
          if (best) next = { ...next, activeVersion: best.version };
        }
        setSettings(next);
        if (next !== s) api.saveSettings(next).catch(() => undefined);
        setEnv(e);
        setInstalled(installedList);
        refreshInstances();
        if (next.autoCheckVersions) refreshRemote();
        if (next.autoCheckUpdate) {
          api.checkLauncherUpdate().then((u) => {
            if (u.available || u.mode === "error") setUpdate(u);
          }).catch(() => undefined);
        }
      } catch (e) {
        addToast("err", `初始化失败: ${e}`);
      }
      try {
        const running = await api.installRunning();
        if (running) setInstallJob({ version: running, logs: [] });
      } catch { /* ignore */ }
    })();
  }, [refreshRemote, refreshInstances, addToast]);

  // Profile 实例状态轮询（外部终端启动的进程也要能感知）
  useEffect(() => {
    refreshInstances();
    const t = setInterval(refreshInstances, 3000);
    return () => clearInterval(t);
  }, [refreshInstances]);

  // ── 事件订阅 ────────────────────────────────
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    const track = (p?: Promise<() => void>) =>
      p?.then((u) => unlisteners.push(u)).catch(() => undefined);
    track(events.onInstallLog?.((e) => {
      setInstallJob((job) =>
        job && job.version === e.version ? { ...job, logs: [...job.logs.slice(-400), e.line] } : job
      );
    }));
    track(events.onInstallFinished?.(async (e) => {
      setInstallJob((job) => (job && job.version === e.version ? null : job));
      if (e.success) {
        addToast("ok", `dsh ${e.version} 安装完成，已设为当前版本`);
        await refreshInstalled();
        if (settingsRef.current && settingsRef.current.activeVersion !== e.version) {
          const next = { ...settingsRef.current, activeVersion: e.version };
          setSettings(next);
          api.saveSettings(next).catch(() => undefined);
        }
      } else {
        addToast("err", `dsh ${e.version} 安装失败：${e.message.split("\n")[0]}`);
      }
    }));
    track(events.onLauncherUpdate?.((s) => setUpdate(s)));
    track(events.onProcLog?.((e: ProcLogEvent) => {
      const hit = /dsh web:\s*(https?:\/\/\S+)/.exec(e.line);
      setProcs((m) => {
        const old = m[e.id] ?? {
          id: e.id, version: e.version, profile: e.profile, startedAt: Date.now(),
          lines: [], exited: false, code: null, webUrl: null,
        } as ProcEntry;
        const webUrl = hit ? hit[1] : old.webUrl;
        return { ...m, [e.id]: { ...old, webUrl, lines: [...old.lines.slice(-500), e.line] } };
      });
      if (hit) addToast("ok", `dsh「${e.profile}」启动成功，可点击「打开」进入主界面`);
      setActiveProc((a) => a ?? e.id);
    }));
    track(events.onProcExit?.((e: ProcExitEvent) => {
      setProcs((m) => {
        const old = m[e.id];
        if (!old) return m;
        return { ...m, [e.id]: { ...old, exited: true, code: e.code } };
      });
      const tag = e.profile ? ` (${e.profile})` : "";
      if (e.code == null) addToast("info", `dsh ${e.version}${tag} 已停止`);
      else if (e.code === 0) addToast("ok", `dsh ${e.version}${tag} 正常退出`);
      else addToast("err", `dsh ${e.version}${tag} 已退出，退出码 ${e.code}`);
    }));
    track(events.onRuntimeLog?.((line) =>
      setRuntimeJob((j) => (j ? { ...j, log: [...j.log.slice(-20), line] } : j))
    ));
    track(events.onRuntimeProgress?.((e) =>
      setRuntimeJob((j) => (j ? { ...j, received: e.received, total: e.total } : j))
    ));
    track(events.onRuntimeFinished?.((e) => {
      addToast(e.ok ? "ok" : "err", e.message);
      if (!e.ok) setRuntimeJob(null);
    }));
    track(events.onToast?.((text) => addToast("err", text)));
    return () => { for (const u of unlisteners) u(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [addToast, refreshInstalled]);

  // ── 动作 ───────────────────────────────────
  const doInstall = useCallback(async (version: string, force: boolean) => {
    setInstallJob((job) => (job ? job : { version, logs: [] }));
    try {
      await api.install(version, force);
    } catch (e) {
      setInstallJob(null);
      addToast("err", `安装失败: ${e}`);
    }
  }, [addToast]);

  const doStopProc = useCallback(async (id: number) => {
    try {
      await api.stopProcess(id);
      addToast("info", `已请求停止进程 ${id}`);
    } catch (e) { addToast("err", `停止失败: ${e}`); }
  }, [addToast]);

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

  const doSetActiveVersion = useCallback(async (v: string) => {
    const s = settingsRef.current;
    if (!s || s.activeVersion === v) return;
    const next = { ...s, activeVersion: v };
    setSettings(next);
    try {
      await api.saveSettings(next);
      addToast("ok", `当前版本已切换为 ${v}，Profile 实例将基于它启动`);
    } catch (e) { addToast("err", `切换版本失败: ${e}`); }
  }, [addToast]);

  const doUninstall = useCallback(async (version: string) => {
    if (!window.confirm(`确定卸载 dsh ${version}？将删除其安装目录。`)) return;
    try {
      await api.uninstall(version);
      addToast("ok", `dsh ${version} 已卸载`);
      await refreshInstalled();
    } catch (e) { addToast("err", `卸载失败: ${e}`); }
  }, [addToast, refreshInstalled]);

  const doCancelInstall = useCallback(async () => {
    try { await api.cancelInstall(); addToast("info", "已请求取消安装"); }
    catch (e) { addToast("err", `取消失败: ${e}`); }
  }, [addToast]);

  const doCheckUpdate = useCallback(async () => {
    try {
      let status = await api.checkLauncherUpdate();
      if (status.mode === "unconfigured") {
        try {
          const u = await updaterCheck();
          builtinUpdate.current = u;
          if (u) {
            status = {
              available: true, current: status.current, latest: u.version,
              notes: u.body ?? null, url: null, mode: "builtin", message: `发现新版本 ${u.version}`,
            };
          }
        } catch { /* 未配置 updater endpoints */ }
      }
      setUpdate(status);
      if (!status.available && status.mode !== "error") addToast("ok", status.message ?? "已是最新版本");
    } catch (e) { addToast("err", `检查更新失败: ${e}`); }
  }, [addToast]);

  const doApplyUpdate = useCallback(async () => {
    const u = builtinUpdate.current;
    if (!u) return;
    setUpdateApplying(true);
    try { await u.downloadAndInstall(); await relaunch(); }
    catch (e) { setUpdateApplying(false); addToast("err", `自动更新失败: ${e}`); }
  }, [addToast]);

  const doSaveSettings = useCallback(async (s: SettingsT) => {
    try {
      await api.saveSettings(s);
      setSettings(s);
      setShowSettings(false);
      addToast("ok", "设置已保存");
      setEnv(await api.getEnvironment());
    } catch (err) { addToast("err", `保存设置失败: ${err}`); }
  }, [addToast]);

  const doStartProfile = useCallback(async (profile: string) => {
    try {
      const info = await api.startEmbedded(null, profile);
      setProcs((m) => ({
        ...m,
        [info.id]: {
          id: info.id, version: info.version, profile: info.profile,
          startedAt: info.startedAt, lines: [], exited: false, code: null, webUrl: null,
        },
      }));
      setActiveProc(info.id);
      setDockOpen(true);
      addToast("ok", `profile「${info.profile}」启动中（dsh ${info.version}，PID ${info.id}）`);
    } catch (e) { addToast("err", `启动失败: ${e}`); }
  }, [addToast]);

  const doStopProfileInstance = useCallback(async (profile: string) => {
    try {
      const hit = await api.stopProfileInstance(profile);
      addToast(hit ? "ok" : "info", hit ? `已停止 profile「${profile}」` : "该 profile 未在运行");
      refreshInstances();
    } catch (e) { addToast("err", `停止失败: ${e}`); }
  }, [addToast, refreshInstances]);

  // ── 派生数据 ───────────────────────────────
  const rows = useMemo(
    () => mergeRowsLocal(remote?.versions ?? [], installed),
    [remote, installed]
  );

  const latestVersion = remote?.tags?.latest;

  const upgradableCount = useMemo(
    () => rows.filter(
      (r) => r.installed && r.installed.version !== "unknown" &&
        latestVersion != null && cmpVer(latestVersion, r.version) > 0
    ).length,
    [rows, latestVersion]
  );

  const runningInstanceCount = useMemo(
    () => Object.values(procs).filter((p) => !p.exited).length,
    [procs]
  );

  const liveWebProcs = useMemo(
    () => Object.values(procs).filter((p) => !p.exited && p.webUrl).sort((a, b) => b.startedAt - a.startedAt),
    [procs]
  );

  // Profile 实例阶段：stopped → starting → ready（出现 URL）/ failed
  const instanceRows = useMemo(() => {
    type Phase = "stopped" | "starting" | "ready" | "failed" | "external";
    type Row = { profile: string; phase: Phase; pid: number | null; source: "embedded" | "external" | null; version: string | null; webUrl: string | null; code: number | null };
    const map = new Map<string, Row>();
    for (const p of profiles) {
      map.set(p.name, { profile: p.name, phase: "stopped", pid: null, source: null, version: null, webUrl: null, code: null });
    }
    for (const i of instances) {
      map.set(i.profile, { profile: i.profile, phase: "external", pid: i.pid, source: "external", version: i.version, webUrl: null, code: null });
    }
    const latest = new Map<string, ProcEntry>();
    for (const p of Object.values(procs)) {
      const cur = latest.get(p.profile);
      if (!cur || p.startedAt > cur.startedAt) latest.set(p.profile, p);
    }
    for (const p of latest.values()) {
      const phase: Phase = p.exited
        ? p.code == null || p.code === 0 ? "stopped" : "failed"
        : p.webUrl ? "ready" : "starting";
      map.set(p.profile, { profile: p.profile, phase, pid: p.id, source: "embedded", version: p.version, webUrl: p.webUrl, code: p.code });
    }
    return [...map.values()].sort((a, b) => a.profile.localeCompare(b.profile));
  }, [profiles, instances, procs]);

  if (!settings || !env) {
    return (
      <div className="flex h-full items-center justify-center gap-3 text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" /> 正在加载…
      </div>
    );
  }

  const navItems: Array<[View, string, typeof Package, number | null]> = [
    ["versions", "版本与安装", Package, upgradableCount > 0 ? upgradableCount : null],
    ["profiles", "Profile 实例", Rocket, runningInstanceCount > 0 ? runningInstanceCount : null],
    ["plugins", "插件管理", Puzzle, null],
    ["config", "配置文件", FileCog, null],
  ];

  return (
    <div className="flex h-full flex-col">
      {/* 顶栏 */}
      <header className="flex h-11 shrink-0 items-center gap-2 border-b border-border bg-background/80 px-4">
        <span className="eyebrow mr-1">Environment</span>
        <Badge variant="outline" title={env.nodePath ?? ""} className="font-mono">
          <span className={`led ${env.node ? "bg-emerald-500 text-emerald-500 led-glow" : "bg-red-500"}`} />
          node {env.node ? `v${env.node}` : "未装"}
        </Badge>
        <Badge variant="outline" title={env.npmPath ?? ""} className="font-mono">
          <span className={`led ${env.npm ? "bg-emerald-500" : "bg-red-500"}`} />
          npm {env.npm ?? "未装"}
        </Badge>
        <Badge variant="outline" className="font-mono">{env.os}/{env.arch}</Badge>
        <div className="flex-1" />
        {liveWebProcs.length > 0 && (
          <Button
            size="sm"
            onClick={() => liveWebProcs[0].webUrl && api.openUrl(liveWebProcs[0].webUrl).catch((e) => addToast("err", String(e)))}
            title={liveWebProcs.length === 1
              ? `打开 dsh 主界面：${liveWebProcs[0].webUrl}`
              : `${liveWebProcs.length} 个实例运行中，点击打开最新一个`}
          >
            <ExternalLink /> 打开 DSH 界面
          </Button>
        )}
        <Button variant="outline" size="sm" disabled={remoteLoading} onClick={refreshRemote}>
          <RefreshCw className={remoteLoading ? "animate-spin" : ""} /> 刷新版本
        </Button>
        <Button variant="outline" size="sm" onClick={doCheckUpdate}>检查更新</Button>
        <Button variant="ghost" size="icon" title="设置" onClick={() => setShowSettings(true)}>
          <SettingsIcon className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          title={resolved === "dark" ? "切换浅色" : "切换深色"}
          onClick={() => setTheme(resolved === "dark" ? "light" : "dark")}
        >
          {resolved === "dark" ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
        </Button>
      </header>

      {/* 更新 / 运行时横幅 */}
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
        <div className="flex shrink-0 items-center gap-3 border-b border-red-500/30 bg-red-500/10 px-4 py-2 text-[13px]">
          {runtimeJob ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin" />
              <span>
                正在安装内置 Node…
                {runtimeJob.total > 0 &&
                  ` ${Math.round((runtimeJob.received / runtimeJob.total) * 100)}% (${(runtimeJob.received / 1048576).toFixed(1)}/${(runtimeJob.total / 1048576).toFixed(1)} MB)`}
              </span>
              <span className="flex-1" />
              <span className="font-mono text-[11px] text-muted-foreground">
                {runtimeJob.log[runtimeJob.log.length - 1] ?? "连接镜像站…"}
              </span>
            </>
          ) : (
            <>
              <XCircle className="h-4 w-4 text-destructive" />
              <span>未检测到 Node.js（dsh 依赖 Node 运行）。可一键安装启动器内置 Node LTS（用户级、无需 root，默认走 npmmirror 镜像）</span>
              <span className="flex-1" />
              <Button size="sm" onClick={doInstallRuntime}>一键安装 Node</Button>
            </>
          )}
        </div>
      )}

      {/* 主体 */}
      <div className="flex min-h-0 flex-1">
        {/* 侧栏导航 */}
        <nav className="flex w-56 shrink-0 flex-col gap-0.5 border-r border-border bg-card/70 p-2.5">
          <div className="mb-3 flex items-center gap-2.5 px-1.5 pb-2 pt-0.5">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-primary to-violet-500 text-[9.5px] font-extrabold text-primary-foreground shadow-md">
              DSH
            </div>
            <div className="leading-tight">
              <div className="text-[13px] font-bold tracking-tight">DSH Launcher</div>
              <div className="text-[10px] text-muted-foreground">@deepseek-ai/dsh · v{env.appVersion}</div>
            </div>
          </div>
          {navItems.map(([key, label, Icon, badge]) => (
            <Button
              key={key}
              variant="ghost"
              className={`relative h-8 w-full justify-start gap-2.5 ${view === key ? "bg-accent text-foreground" : "text-muted-foreground hover:text-foreground"}`}
              onClick={() => setView(key)}
            >
              {view === key && (
                <span className="absolute left-0 top-1/2 h-4 w-[2.5px] -translate-y-1/2 rounded-full bg-primary" />
              )}
              <Icon className="h-4 w-4 opacity-80" />
              <span>{label}</span>
              {badge != null && (
                <Badge variant={key === "profiles" ? "success" : "warning"} className="ml-auto">
                  {badge}
                </Badge>
              )}
            </Button>
          ))}

          <div className="mt-auto space-y-2 border-t border-border pt-3 text-[11.5px]">
            <div className="flex items-center gap-2">
              <span className={`h-1.5 w-1.5 rounded-full ${env.node ? "bg-emerald-500" : "bg-red-500"}`} />
              <span className="font-mono">{env.node ? `Node v${env.node}` : "Node 未装"}</span>
            </div>
            <div className="flex items-center gap-2">
              <span className={`h-1.5 w-1.5 rounded-full ${settings.activeVersion ? "bg-emerald-500" : "bg-amber-500"}`} />
              <span className="font-mono">{settings.activeVersion || "版本未选"}</span>
            </div>
            <div className="flex items-center gap-2">
              <span className={`h-1.5 w-1.5 rounded-full ${runningInstanceCount > 0 ? "bg-emerald-500" : "bg-muted-foreground/40"}`} />
              <span className="font-mono">{runningInstanceCount} 个实例运行中</span>
            </div>
            <div className="pt-1 text-[10.5px] leading-relaxed text-muted-foreground">
              <div>registry：{env.registry}</div>
              <div className="break-all">数据目录：{env.versionsDir}</div>
            </div>
          </div>
        </nav>

        {/* 内容区 */}
        <main className="min-w-0 flex-1 overflow-y-auto p-5">
          {view === "versions" && (
            <div className="space-y-4">
              <div className="grid grid-cols-[300px_1fr] gap-3">
                <Card className="p-4">
                  <div className="eyebrow mb-2.5">① Node 环境</div>
                  {env.node ? (
                    <div className="flex items-center gap-2 text-sm">
                      <CheckCircle2 className="h-4 w-4 text-emerald-500" />
                      <span className="font-mono">Node v{env.node}</span>
                      <span className="text-xs text-muted-foreground">
                        {env.nodePath?.includes(".dsh-launcher") ? "（内置运行时）" : "（系统）"}
                      </span>
                    </div>
                  ) : (
                    <Button size="sm" onClick={doInstallRuntime}>一键安装内置 Node</Button>
                  )}
                </Card>
                <Card className="p-4">
                  <div className="eyebrow mb-2.5">② 当前 DSH 版本 —— 所有 Profile 实例基于它运行</div>
                  {installed.length > 0 ? (
                    <Select value={settings.activeVersion} onValueChange={doSetActiveVersion}>
                      <SelectTrigger className="w-64 font-mono">
                        <SelectValue placeholder="选择版本" />
                      </SelectTrigger>
                      <SelectContent>
                        {installed.map((i, idx) => (
                          <SelectItem key={`${i.version}-${idx}`} value={i.version}>
                            {i.version}
                            {i.source === "managed" ? "" : ` · ${i.source === "global" ? "全局" : "PATH"}`}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  ) : (
                    <div className="flex items-center gap-2 text-sm text-muted-foreground">
                      <XCircle className="h-4 w-4 text-amber-500" /> 未安装——在下方列表点「安装」
                    </div>
                  )}
                </Card>
              </div>

              {installJob && (
                <InstallCard version={installJob.version} logs={installJob.logs} onCancel={doCancelInstall} />
              )}

              {remoteErr && !remoteLoading && (
                <Card className="border-destructive/40 p-6 text-center text-destructive">
                  <div className="mb-2 text-3xl">⚠</div>
                  <div className="mb-3">拉取官方版本列表失败：{remoteErr}</div>
                  <Button variant="outline" size="sm" onClick={refreshRemote}>重试</Button>
                </Card>
              )}

              {!remoteErr && rows.length === 0 && !remoteLoading && (
                <Card className="p-10 text-center text-muted-foreground">
                  <div className="mb-2 text-3xl">📦</div>
                  点击右上角「刷新版本」从 registry 拉取 @deepseek-ai/dsh 的官方发布版本
                </Card>
              )}

              <div className="space-y-2">
                {rows.map((r) => (
                  <VersionRow
                    key={`${r.version}-${r.installed?.source ?? "remote"}`}
                    row={r}
                    isLatestTag={latestVersion === r.version}
                    busy={installJob !== null}
                    upgradeTo={
                      r.installed && r.installed.version !== "unknown" && latestVersion &&
                      cmpVer(latestVersion, r.version) > 0
                        ? latestVersion : null
                    }
                    onUpgrade={(v) => doInstall(v, false)}
                    isActive={settings.activeVersion === r.version}
                    onSetActive={doSetActiveVersion}
                    onInstall={(v, force) => doInstall(v, force)}
                    onUninstall={doUninstall}
                    onReveal={(p) => api.reveal(p).catch((e) => addToast("err", String(e)))}
                  />
                ))}
              </div>
            </div>
          )}

          {view === "profiles" && (
            <div className="space-y-3">
              <div className="flex items-center gap-2">
                <h2 className="text-base font-semibold">Profile 实例</h2>
                <span className="text-xs text-muted-foreground">
                  基于「当前版本」<b className="font-mono">{settings.activeVersion || "（未选择）"}</b>；
                  不同 profile 可并行，同一 profile 同时只能运行一个
                </span>
                <span className="flex-1" />
                <Button size="sm" variant="outline" onClick={refreshProfiles} title="重新扫描 $DSH_HOME/profiles">
                  <RefreshCw /> 重扫目录
                </Button>
              </div>
              <div className="rounded-lg border border-amber-500/25 bg-amber-500/5 p-2.5 text-[11.5px] leading-relaxed text-muted-foreground">
                目前仅验证过 <b className="text-amber-500">web</b> 类 profile 可正常启动；其他 profile
                可能是复制 web 的配置（实例名不同、内容同为 web，仅端口等不同），也可能启动失败——以实际日志为准。
              </div>
              <div className="space-y-2">
                {instanceRows.map((row) => {
                  const phaseText =
                    row.phase === "starting" ? "启动中…"
                    : row.phase === "ready" ? "启动成功"
                    : row.phase === "failed" ? `启动失败${row.code != null ? `（退出码 ${row.code}）` : ""}`
                    : row.phase === "external" ? "运行中（外部启动）" : "未运行";
                  const canStop = row.phase === "starting" || row.phase === "ready" || row.phase === "external";
                  const canOpen = row.phase === "ready" && !!row.webUrl;
                  return (
                    <Card key={row.profile} className="flex-row items-center gap-3 p-3">
                      {row.phase === "starting" ? (
                        <Loader2 className="h-4 w-4 shrink-0 animate-spin text-amber-500" />
                      ) : row.phase === "ready" ? (
                        <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-500" />
                      ) : row.phase === "failed" ? (
                        <XCircle className="h-4 w-4 shrink-0 text-red-500" />
                      ) : (
                        <span className={`h-2.5 w-2.5 shrink-0 rounded-full ${row.phase === "external" ? "bg-sky-500" : "bg-muted-foreground/30"}`} />
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="font-mono text-[13px] font-semibold">{row.profile}</div>
                        <div className="text-[11px] text-muted-foreground">
                          {phaseText}
                          {row.pid ? ` · PID ${row.pid}` : ""}
                          {row.version ? ` · ${row.version}` : ""}
                        </div>
                      </div>
                      {canOpen && (
                        <Button
                          size="sm"
                          onClick={() => row.webUrl && api.openUrl(row.webUrl).catch((e) => addToast("err", String(e)))}
                        >
                          <ExternalLink /> 打开
                        </Button>
                      )}
                      {canStop ? (
                        <Button size="sm" variant="destructive" onClick={() => doStopProfileInstance(row.profile)}>
                          <Square /> 停止
                        </Button>
                      ) : (
                        <Button
                          size="sm"
                          variant="outline"
                          disabled={installed.length === 0}
                          title={installed.length === 0
                            ? "请先在「版本与安装」页安装 dsh"
                            : row.phase === "failed"
                            ? "重新启动该 profile"
                            : `基于当前版本（${settings.activeVersion}）启动 ${row.profile}`}
                          onClick={() => doStartProfile(row.profile)}
                        >
                          <Play /> 启动
                        </Button>
                      )}
                    </Card>
                  );
                })}
                {instanceRows.length === 0 && (
                  <Card className="p-8 text-center text-muted-foreground">
                    未找到 profile（检查 $DSH_HOME/profiles 目录）
                  </Card>
                )}
              </div>
              <div className="text-[11px] text-muted-foreground">
                启停遇到插件问题时，到「插件管理」页停用可疑插件后重启实例。
              </div>
            </div>
          )}

          {view === "plugins" && <PluginsView profiles={profiles.map((p) => p.name)} onToast={addToast} />}
          {view === "config" && <ConfigView onToast={addToast} />}
        </main>
      </div>

      <ProcessDock
        procs={Object.values(procs).sort((a, b) => a.id - b.id)}
        activeId={activeProc}
        open={dockOpen}
        onToggle={() => setDockOpen((v) => !v)}
        onSelect={setActiveProc}
        onStop={doStopProc}
        onOpenWeb={(u) => api.openUrl(u).catch((e) => addToast("err", String(e)))}
        onExport={() => {
          const p = activeProc != null ? procsRef.current[activeProc] : null;
          if (!p) return;
          api.exportProcLog(p.profile || "default", p.id, p.lines.join("\n"))
            .then((path) => addToast("ok", `日志已导出：${path}`))
            .catch((e) => addToast("err", `导出失败: ${e}`));
        }}
        onClearExited={() => {
          const next = Object.fromEntries(
            Object.entries(procsRef.current).filter(([, p]) => !p.exited)
          ) as Record<number, ProcEntry>;
          setProcs(next);
          setActiveProc((a) => (a != null && next[a] ? a : (Object.values(next)[0]?.id ?? null)));
        }}
      />

      <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-border bg-card px-4 text-[11px] text-muted-foreground">
        <span>官方源 {env.registry}</span>
        <span className="font-mono">{env.dshHome}</span>
        <span className="ml-auto">退出启动器会结束所有内嵌 dsh 进程；关闭窗口最小化到托盘</span>
      </footer>

      {showSettings && (
        <SettingsModal
          initial={settings}
          env={env}
          onSave={doSaveSettings}
          onClose={() => setShowSettings(false)}
          onReveal={(p) => api.reveal(p).catch((e) => addToast("err", String(e)))}
        />
      )}

      {/* Toasts */}
      <div className="fixed bottom-4 right-4 z-[100] flex flex-col gap-2">
        {toasts.map((t) => (
          <div
            key={t.id}
            className={`max-w-md rounded-lg border bg-card px-3.5 py-2.5 text-[13px] shadow-lg ${
              t.kind === "ok" ? "border-emerald-500/40" : t.kind === "err" ? "border-red-500/40" : "border-border"
            }`}
          >
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}

// ── 本地辅助 ───────────────────────────────
function cmpVer(a: string, b: string): number {
  const core = (v: string) => v.replace(/^v/, "").split("-")[0].split(".").map((x) => parseInt(x, 10) || 0);
  const ca = core(a); const cb = core(b);
  for (let i = 0; i < 3; i++) if ((ca[i] ?? 0) !== (cb[i] ?? 0)) return (ca[i] ?? 0) - (cb[i] ?? 0);
  const pa = a.includes("-") ? a.split("-").slice(1).join("-") : "";
  const pb = b.includes("-") ? b.split("-").slice(1).join("-") : "";
  if (pa === pb) return 0;
  if (!pa) return 1;
  if (!pb) return -1;
  return pa.localeCompare(pb);
}

interface MergedRow {
  version: string;
  remote: { version: string; publishedAt: string | null; description: string | null; unpackedSize: number | null } | null;
  installed: InstalledVersion | null;
  channel: string;
}

function mergeRowsLocal(remoteVersions: Array<{ version: string; channel: string; tags: string[]; publishedAt: string | null; description: string | null; unpackedSize: number | null }>, installed: InstalledVersion[]): MergedRow[] {
  const map = new Map<string, MergedRow>();
  for (const r of remoteVersions) {
    map.set(r.version, {
      version: r.version,
      remote: { version: r.version, publishedAt: r.publishedAt, description: r.description, unpackedSize: r.unpackedSize },
      installed: null,
      channel: r.channel || deriveChannel(r.version),
    });
  }
  for (const i of installed) {
    const exist = map.get(i.version);
    if (exist) exist.installed = i;
    else if (i.version !== "unknown") {
      map.set(i.version, { version: i.version, remote: null, installed: i, channel: deriveChannel(i.version) });
    }
  }
  return [...map.values()].sort((a, b) => cmpVer(b.version, a.version));
}

function deriveChannel(v: string): string {
  const pre = v.includes("-") ? v.split("-").slice(1).join("-") : "";
  if (pre.startsWith("alpha")) return "alpha";
  if (pre.startsWith("beta")) return "beta";
  if (pre.startsWith("rc")) return "rc";
  return "stable";
}


