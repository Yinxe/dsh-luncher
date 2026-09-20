import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Puzzle, RefreshCw, Settings as SettingsIcon,
  ExternalLink, Play, Square, CheckCircle2, XCircle, Loader2, Sun, Moon, Terminal,
  TriangleAlert, ChevronDown, CopyPlus, Info, RotateCw, FileText,
  Pencil, Trash2, ShieldPlus, MoreHorizontal,
} from "lucide-react";
import { toast } from "sonner";
import { api, events } from "./api";
import { useTheme } from "@/lib/theme";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Toaster } from "@/components/ui/sonner";
import { SidebarInset, SidebarProvider, SidebarTrigger } from "@/components/ui/sidebar";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel,
  DropdownMenuSeparator, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useSidebarOpen } from "@/hooks/use-layout";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import ProfileConfigPanel from "./components/ProfileConfigPanel";
import CopyProfileDialog from "./components/CopyProfileDialog";
import RenameProfileDialog from "./components/RenameProfileDialog";
import TrashDialog from "./components/TrashDialog";
import { Progress } from "@/components/ui/progress";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import InstallCard from "./components/InstallCard";
import ConfigView from "./components/ConfigView";
import ModelConfigView from "./components/ModelConfigView";
import CredentialsView from "./components/CredentialsView";
import PluginsView from "./components/PluginsView";
import ProcessSidePanel from "./components/ProcessSidePanel";
import VersionRow from "./components/VersionRow";
import SettingsDrawer from "./components/SettingsDrawer";
import UpdateBanner from "./components/UpdateBanner";
import AppSidebar from "./components/AppSidebar";
import type {
  EnvironmentInfo, InstalledVersion, LauncherUpdateStatus, ProcEntry,
  ProcExitEvent, ProcLogEvent, ProfileInfo, ProfileInstance, ProfileTarget,
  RegistryInfo, Settings as SettingsT, View,
} from "./types";

/** 恢复模式 profile 名（与后端 profile_cfg::RECOVERY_PROFILE 保持一致） */
const RECOVERY_PROFILE = "web-Recovery";

/**
 * 各 Target 的标签展示与启动支持状态；新增 Target（如 CLI）在此扩展。
 * Target 由后端按 profile 的 package.json dsh.profile.bundles 识别。
 */
const TARGET_META: Record<
  ProfileTarget,
  { label: string; variant: "info" | "secondary" | "outline"; desc: string; launchable: boolean }
> = {
  web: {
    label: "Web",
    variant: "info",
    desc: "Web 应用：启动后从日志识别地址并打开浏览器",
    launchable: true,
  },
  desktop: {
    label: "Desktop",
    variant: "secondary",
    desc: "桌面应用：启动方式将在后续版本支持",
    launchable: false,
  },
  unknown: {
    label: "未识别",
    variant: "outline",
    desc: "未识别 Target（bundles 中无已知 Target 插件）",
    launchable: false,
  },
};

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
  const [updateProgress, setUpdateProgress] = useState<{ received: number; total: number } | null>(null);
  const [pendingUninstall, setPendingUninstall] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [procs, setProcs] = useState<Record<number, ProcEntry>>({});
  const [activeProc, setActiveProc] = useState<number | null>(null);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [runtimeJob, setRuntimeJob] = useState<{ received: number; total: number; log: string[] } | null>(null);
  const runtimeBusy = useRef(false);
  const [instances, setInstances] = useState<ProfileInstance[]>([]);
  const [view, setView] = useState<View>("versions");
  const [verScope, setVerScope] = useState<"all" | "installed">("all");
  const [verType, setVerType] = useState<"all" | "stable" | "pre">("all");
  /** 各 profile 配置折叠面板的展开状态 */
  const [expandedProfiles, setExpandedProfiles] = useState<Record<string, boolean>>({});
  /** 正在重启的 profile（停止→等待消失→再拉起） */
  const [restartingProfile, setRestartingProfile] = useState<string | null>(null);
  /** 启动请求在途的 profile：按钮转圈并禁用，避免连点重复启动 */
  const [startingProfile, setStartingProfile] = useState<string | null>(null);
  /** 启动/重启在途的 profile 集合（ref 版）：连点时 state 还没重渲染，
   *  只靠 state 判断会放第二次请求过去 */
  const startingProfilesRef = useRef<Set<string>>(new Set());
  const restartingRef = useRef(false);
  /** 复制实例对话框的源 profile；null = 关闭 */
  const [copySource, setCopySource] = useState<string | null>(null);
  /** 重命名对话框的目标 profile；null = 关闭 */
  const [renameTarget, setRenameTarget] = useState<string | null>(null);
  /** 删除确认的目标 profile；null = 关闭 */
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  /** 回收站（删除的 profile 可还原 / 彻底删除） */
  const [trashOpen, setTrashOpen] = useState(false);
  /** 恢复模式创建确认 */
  const [recoveryOpen, setRecoveryOpen] = useState(false);
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  /** 插件管理页的预选 profile（从 Profile 实例卡片跳转时种子化；导航进入时清空走默认） */
  const [pluginsSeed, setPluginsSeed] = useState<string | null>(null);

  const procsRef = useRef<Record<number, ProcEntry>>({});
  procsRef.current = procs;
  const settingsRef = useRef<SettingsT | null>(null);
  settingsRef.current = settings;
  const instancesRef = useRef<ProfileInstance[]>([]);
  instancesRef.current = instances;

  const { resolved, setTheme } = useTheme();

  /** 侧栏开合：默认跟随窗口档位（宽展开 / 窄收成图标栏 / 超窄走抽屉），手动可覆盖 */
  const [sidebarOpen, setSidebarOpen] = useSidebarOpen();

  const addToast = useCallback((kind: "ok" | "err" | "info", text: string) => {
    if (kind === "ok") toast.success(text);
    else if (kind === "err") toast.error(text, { duration: 7000 });
    else toast.info(text);
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
          // 默认 profile 优先选 Web 类型（当前唯一支持的启动方式）
          next = { ...s, defaultProfile: ps.find((p) => p.target === "web")?.name ?? ps[0].name };
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
    // StrictMode/重挂载时，清理可能在 listen() Promise resolve 之前跑完；用 alive 兜住，
    // 晚到的 unlisten 立即调用，避免监听器重复订阅（proc-log 重复行、双 toast）。
    let alive = true;
    const track = (p?: Promise<() => void>) =>
      p?.then((u) => { if (alive) unlisteners.push(u); else u(); }).catch(() => undefined);
    track(events.onInstallLog?.((e) => {
      setInstallJob((job) =>
        job && job.version === e.version ? { ...job, logs: [...job.logs.slice(-400), e.line] } : job
      );
    }));
    track(events.onInstallFinished?.(async (e) => {
      setInstallJob((job) => (job && job.version === e.version ? null : job));
      if (e.success) {
        await refreshInstalled();
        const s = settingsRef.current;
        if (!s || s.activeVersion === e.version) {
          addToast("ok", `dsh ${e.version} 安装完成，已设为当前版本`);
        } else if (instancesRef.current.some((i) => i.running)) {
          // 与手动切换同样的限制：有实例运行时不切换，只安装
          addToast(
            "err",
            `dsh ${e.version} 安装完成，但有 Profile 实例正在运行，未自动切换为当前版本。请先停止所有实例，再手动切换`
          );
        } else {
          const next = { ...s, activeVersion: e.version };
          setSettings(next);
          try {
            await api.saveSettings(next);
            addToast("ok", `dsh ${e.version} 安装完成，已设为当前版本`);
          } catch (err) {
            setSettings(s);
            addToast("err", `dsh ${e.version} 已安装，但自动设为当前版本失败: ${err}`);
          }
        }
      } else {
        addToast("err", `dsh ${e.version} 安装失败：${e.message.split("\n")[0]}`);
      }
    }));
    track(events.onLauncherUpdate?.((s) => {
      setUpdate(s);
      // 静默自动更新：后端已经在下载了，这里把进度条顶起来
      if (s.available && settingsRef.current?.autoInstallUpdate && s.mode === "builtin") {
        setUpdateApplying(true);
        setUpdateProgress({ received: 0, total: 0 });
      }
    }));
    track(events.onLauncherUpdateProgress?.((p) => setUpdateProgress(p)));
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
      const old = procsRef.current[e.id];
      setProcs((m) => {
        const cur = m[e.id];
        if (!cur) return m;
        return { ...m, [e.id]: { ...cur, exited: true, code: e.code } };
      });
      const tag = e.profile ? ` (${e.profile})` : "";
      if (e.code == null) addToast("info", `dsh ${e.version}${tag} 已停止`);
      else if (
        e.code === 0 && old && old.lines.length === 0 && Date.now() - old.startedAt < 3000
      ) {
        // 秒退且零输出：进程什么都没做就退出，最典型的是 Node 版本过旧
        // （dsh 入口依赖 import.meta.main）导致 runCli 从未执行
        addToast(
          "err",
          `dsh ${e.version}${tag} 启动后立即退出且无任何输出。常见原因：Node 版本过低（需 ≥ v24.2），请在「版本与安装」页安装/升级内置 Node 后重试`
        );
      } else if (e.code === 0) addToast("ok", `dsh ${e.version}${tag} 正常退出`);
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

  /** 实例终端的停止：内嵌/独立/外部统一按 PID 走 stop_process */
  const doStopProc = useCallback(async (id: number) => {
    try {
      const hit = await api.stopProcess(id);
      addToast(hit ? "ok" : "info", hit ? `已停止实例 ${id}` : `实例 ${id} 未在运行`);
      refreshInstances();
    } catch (e) { addToast("err", `停止失败: ${e}`); }
  }, [addToast, refreshInstances]);

  /** 独立进程实例的日志尾部（内嵌实例走实时管道，不用这个） */
  const readInstanceLog = useCallback((pid: number) => api.readInstanceLog(pid), []);
  const revealPath = useCallback((path: string) => {
    api.reveal(path).catch((e) => addToast("err", String(e)));
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

  const doSetNodeSource = useCallback(async (v: "auto" | "system" | "runtime") => {
    const s = settingsRef.current;
    if (!s || s.nodeSource === v) return;
    const next = { ...s, nodeSource: v };
    setSettings(next);
    try {
      await api.saveSettings(next);
      setEnv(await api.getEnvironment());
      addToast("ok", `Node 来源已切换为 ${v === "auto" ? "自动" : v === "system" ? "系统级" : "隔离（内置）"}`);
    } catch (e) {
      setSettings(s); // 保存失败时回滚乐观更新，避免 UI 与实际配置不一致
      addToast("err", `切换失败: ${e}`);
    }
  }, [addToast]);

  const doSetLaunchMode = useCallback(async (v: "child" | "detached") => {
    const s = settingsRef.current;
    if (!s || s.launchMode === v) return;
    const next = { ...s, launchMode: v };
    setSettings(next);
    try {
      await api.saveSettings(next);
      addToast(
        "ok",
        v === "detached"
          ? "启动方式已切换为独立进程（后台常驻，日志写入 ~/.dsh-launcher/instance-logs）"
          : "启动方式已切换为子进程（随启动器退出结束）"
      );
    } catch (e) {
      setSettings(s);
      addToast("err", `切换失败: ${e}`);
    }
  }, [addToast]);

  const doSetActiveVersion = useCallback(async (v: string) => {
    const s = settingsRef.current;
    if (!s || s.activeVersion === v) return;
    // 有实例运行时禁止切换：所有实例都基于当前版本，需先手动全部停止
    if (instancesRef.current.some((i) => i.running)) {
      addToast("err", "有 Profile 实例正在运行，不能切换 DSH 版本。请先停止所有实例后再切换");
      return;
    }
    const next = { ...s, activeVersion: v };
    setSettings(next);
    try {
      await api.saveSettings(next);
      addToast("ok", `当前版本已切换为 ${v}，Profile 实例将基于它启动`);
    } catch (e) {
      setSettings(s); // 后端兜底拒绝（如拦截窗口内新起了实例）时回滚乐观更新
      addToast("err", `切换版本失败: ${e}`);
    }
  }, [addToast]);

  const doUninstall = useCallback((version: string) => {
    setPendingUninstall(version);
  }, []);

  const confirmUninstall = useCallback(async () => {
    if (!pendingUninstall) return;
    const version = pendingUninstall;
    setPendingUninstall(null);
    try {
      await api.uninstall(version);
      addToast("ok", `dsh ${version} 已卸载`);
      await refreshInstalled();
    } catch (e) { addToast("err", `卸载失败: ${e}`); }
  }, [pendingUninstall, addToast, refreshInstalled]);

  const confirmDeleteProfile = useCallback(async () => {
    if (!deleteTarget) return;
    const name = deleteTarget;
    setDeleteTarget(null);
    try {
      const trash = await api.deleteProfile(name);
      addToast("ok", `已删除 profile「${name}」，配置已移入 ${trash}，可手动找回`);
      refreshProfiles();
      refreshInstances();
      api.getSettings().then(setSettings).catch(() => undefined);
    } catch (e) { addToast("err", `删除失败: ${e}`); }
  }, [deleteTarget, addToast, refreshProfiles, refreshInstances]);

  const confirmCreateRecovery = useCallback(async () => {
    setRecoveryOpen(false);
    setRecoveryBusy(true);
    try {
      const r = await api.createRecoveryProfile();
      addToast(
        "ok",
        `已创建恢复模式「${r.name}」（端口 ${r.port}）：由 dsh 自带的 web 模板新建，只有官方 base + web-app 插件，可在 Profile 实例页启动`
      );
      refreshProfiles();
      refreshInstances();
    } catch (e) {
      addToast("err", `创建恢复模式失败: ${e}`);
    } finally { setRecoveryBusy(false); }
  }, [addToast, refreshProfiles, refreshInstances]);

  const doCancelInstall = useCallback(async () => {
    try { await api.cancelInstall(); addToast("info", "已请求取消安装"); }
    catch (e) { addToast("err", `取消失败: ${e}`); }
  }, [addToast]);

  const doCheckUpdate = useCallback(async () => {
    try {
      // 后端统一决策：配了自建清单就查清单，否则查内置 updater（后者可在应用内安装）
      const status = await api.checkLauncherUpdate();
      setUpdate(status);
      if (status.available) return;
      if (status.mode === "manifest" || status.mode === "builtin") {
        addToast("ok", status.message ?? "已是最新版本");
      } else {
        addToast("err", status.message ?? "检查更新失败");
      }
    } catch (e) { addToast("err", `检查更新失败: ${e}`); }
  }, [addToast]);

  const doApplyUpdate = useCallback(async () => {
    setUpdateApplying(true);
    setUpdateProgress(null);
    try {
      // 非 Windows 上这个调用不会返回：安装成功后进程直接重启
      const msg = await api.installLauncherUpdate();
      addToast("ok", msg);
      setUpdateApplying(false);
    } catch (e) {
      setUpdateApplying(false);
      setUpdateProgress(null);
      addToast("err", `自动更新失败: ${e}`);
    }
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
    // 连点/重复触发守卫：同一 profile 的启动请求还在途时直接忽略。
    // 后端 ensure_profile_free 依赖进程枚举（外部实例还有扫描/缓存延迟），
    // 挡不住同一瞬间挤进来的两次请求 —— 那样会起两个 dsh 抢同一个 web 端口。
    if (startingProfilesRef.current.has(profile)) return;
    startingProfilesRef.current.add(profile);
    setStartingProfile(profile);
    const detached = settingsRef.current?.launchMode === "detached";
    try {
      const info = await api.startEmbedded(null, profile, undefined, detached);
      if (detached) {
        // 独立进程没有日志管道：先刷新实例列表把它带进「实例终端」，再选中并展开，
        // 否则用户启动完看不到任何反馈（只能去 Profile 实例页找）
        addToast(
          "ok",
          `profile「${info.profile}」已以独立进程启动（PID ${info.id}），日志：~/.dsh-launcher/instance-logs`
        );
        await refreshInstances();
        setActiveProc(info.id);
        setDrawerOpen(true);
      } else {
        setProcs((m) => ({
          ...m,
          [info.id]: {
            id: info.id, version: info.version, profile: info.profile,
            startedAt: info.startedAt, lines: [], exited: false, code: null, webUrl: null,
          },
        }));
        setActiveProc(info.id);
        setDrawerOpen(true);
        addToast("ok", `profile「${info.profile}」启动中（dsh ${info.version}，PID ${info.id}）`);
      }
      refreshInstances(); // 立即感知新实例，让「切换版本」锁定尽快生效（否则要等 3s 轮询）
    } catch (e) { addToast("err", `启动失败: ${e}`); }
    finally {
      startingProfilesRef.current.delete(profile);
      setStartingProfile(null);
    }
  }, [addToast, refreshInstances]);

  const doRestartProfile = useCallback(
    async (profile: string) => {
      if (restartingRef.current || startingProfilesRef.current.has(profile)) return;
      restartingRef.current = true;
      setRestartingProfile(profile);
      try {
        if (instancesRef.current.some((i) => i.profile === profile)) {
          addToast("info", `正在停止 profile「${profile}」…`);
          // 反复停止直到实例列表不再出现（幂等）；进程枚举有 TTL 缓存
          //（Windows 5s），等待过短会被唯一性守卫拒绝立即重启
          const deadline = Date.now() + 15_000;
          while (Date.now() < deadline) {
            await api.stopProfileInstance(profile).catch(() => false);
            const list = await api.listProfileInstances();
            if (!list.some((i) => i.profile === profile)) break;
            await new Promise((r) => setTimeout(r, 500));
          }
        }
        await doStartProfile(profile);
      } catch (e) {
        addToast("err", `重启失败: ${e}`);
      } finally {
        restartingRef.current = false;
        setRestartingProfile(null);
      }
    },
    [addToast, doStartProfile]
  );

  /** 统一停止入口：已知 PID 就按 PID 停（内嵌/独立/外部都走这一条），
   *  否则退回按 profile 停。这样「按端口发现的外部实例」也一定有停止途径。 */
  const doStopInstance = useCallback(async (row: { profile: string; pid: number | null }) => {
    const name = row.profile || (row.pid != null ? `PID ${row.pid}` : "该实例");
    try {
      const hit = row.pid != null
        ? await api.stopProcess(row.pid)
        : await api.stopProfileInstance(row.profile);
      addToast(hit ? "ok" : "info", hit ? `已停止「${name}」` : `「${name}」未在运行`);
      refreshInstances();
    } catch (e) { addToast("err", `停止失败: ${e}`); }
  }, [addToast, refreshInstances]);

  // ── 派生数据 ───────────────────────────────
  const rows = useMemo(
    () => mergeRowsLocal(remote?.versions ?? [], installed),
    [remote, installed]
  );

  const latestVersion = remote?.tags?.latest;

  // 是否已安装 ≥ latest 标签的版本——装过最新版后，旧版本行不再提示「可升级」
  const hasLatestInstalled = useMemo(
    () =>
      latestVersion != null &&
      installed.some((i) => i.version !== "unknown" && cmpVer(i.version, latestVersion) >= 0),
    [installed, latestVersion]
  );

  const filteredVerRows = useMemo(
    () =>
      rows.filter((r) => {
        if (verScope === "installed" && !r.installed) return false;
        // RC 属于正式版；仅 alpha/beta 视为预发布
        const isPre = r.channel === "alpha" || r.channel === "beta";
        if (verType === "stable" && isPre) return false;
        if (verType === "pre" && !isPre) return false;
        return true;
      }),
    [rows, verScope, verType]
  );

  const upgradableCount = useMemo(
    () =>
      hasLatestInstalled
        ? 0
        : rows.filter(
            (r) => r.installed && r.installed.version !== "unknown" &&
              latestVersion != null && cmpVer(latestVersion, r.version) > 0
          ).length,
    [rows, latestVersion, hasLatestInstalled]
  );

  // 「当前 DSH 版本」卡片展示安装位置用
  const activeInst = useMemo(
    () => (settings ? installed.find((i) => i.version === settings.activeVersion) : undefined),
    [installed, settings]
  );

  // 实例终端抽屉的数据源：内嵌子进程（实时管道）+ 独立/外部实例（无管道，
  // 独立进程按需 tail 日志文件，外部实例只提示「日志在启动它的终端里」）
  const panelProcs = useMemo<ProcEntry[]>(() => {
    const list: ProcEntry[] = Object.values(procs).map((p) => ({ ...p, external: false }));
    const seen = new Set(list.map((p) => p.id));
    for (const i of instances) {
      if (i.source === "embedded" || i.pid == null || seen.has(i.pid)) continue;
      seen.add(i.pid);
      list.push({
        id: i.pid,
        version: i.version ?? "",
        profile: i.profile,
        startedAt: i.startedAt ?? 0,
        lines: [],
        exited: false,
        code: null,
        webUrl: i.webUrl ?? null,
        external: true,
        logFile: i.logFile ?? null,
        port: i.port ?? null,
      });
    }
    return list.sort((a, b) => a.id - b.id);
  }, [procs, instances]);

  // 运行中实例数：必须走 panelProcs——独立进程/终端外部启动的实例不在 procs 里，
  // 早先只数 procs 会出现「界面上有 2 个在跑、角标却显示 1」
  const runningInstanceCount = useMemo(
    () => panelProcs.filter((p) => !p.exited).length,
    [panelProcs]
  );

  // 有无任何运行中的 Profile 实例（含外部终端启动的）——运行期间禁止切换 DSH 版本
  const instancesRunning = useMemo(
    () => instances.some((i) => i.running),
    [instances]
  );

  // 顶栏「打开 DSH 界面」：内嵌实例从前端实时日志解析，独立进程由后端从日志解析
  const liveWebProcs = useMemo(
    () => panelProcs.filter((p) => !p.exited && p.webUrl).sort((a, b) => b.startedAt - a.startedAt),
    [panelProcs]
  );

  // Profile 实例阶段：stopped → starting → ready（出现 URL）/ failed
  const instanceRows = useMemo(() => {
    type Phase = "stopped" | "starting" | "ready" | "failed" | "external";
    type Row = { key: string; profile: string; phase: Phase; pid: number | null; source: string | null; version: string | null; webUrl: string | null; code: number | null; target: ProfileTarget; port: number | null; logFile: string | null; reserved: boolean };
    // 行主键：有 profile 名就用名字；没有 profile（终端 `dsh web` 没带 --profile）就用
    // PID/端口，否则多个无名实例会互相覆盖，界面上只剩一个
    const keyOf = (profile: string, pid: number | null, port: number | null) =>
      profile || (pid != null ? `pid:${pid}` : port != null ? `port:${port}` : "unknown");
    const map = new Map<string, Row>();
    for (const p of profiles) {
      map.set(p.name, { key: p.name, profile: p.name, phase: "stopped", pid: null, source: null, version: null, webUrl: null, code: null, target: p.target, port: null, logFile: null, reserved: p.reserved });
    }
    for (const i of instances) {
      const key = keyOf(i.profile, i.pid, i.port);
      const known = map.get(key);
      map.set(key, {
        key,
        profile: i.profile,
        phase: "external",
        pid: i.pid,
        source: i.source,
        version: i.version,
        webUrl: i.webUrl ?? null,
        code: null,
        target: known?.target ?? "unknown",
        port: i.port ?? null,
        logFile: i.logFile ?? null,
        reserved: known?.reserved ?? false,
      });
    }
    const latest = new Map<string, ProcEntry>();
    for (const p of Object.values(procs)) {
      const cur = latest.get(p.profile);
      if (!cur || p.startedAt > cur.startedAt) latest.set(p.profile, p);
    }
    for (const p of latest.values()) {
      const key = keyOf(p.profile, p.id, null);
      const phase: Phase = p.exited
        ? p.code == null || p.code === 0 ? "stopped" : "failed"
        : p.webUrl ? "ready" : "starting";
      map.set(key, { key, profile: p.profile, phase, pid: p.id, source: "embedded", version: p.version, webUrl: p.webUrl, code: p.code, target: map.get(key)?.target ?? "unknown", port: null, logFile: null, reserved: map.get(key)?.reserved ?? false });
    }
    const label = (r: Row) => r.profile || `:${r.port ?? "?"}`;
    return [...map.values()].sort((a, b) => label(a).localeCompare(label(b)));
  }, [profiles, instances, procs]);

  /** 恢复模式 profile 是否已存在（存在就不再显示创建入口） */
  const recoveryExists = useMemo(
    () => profiles.some((p) => p.name === RECOVERY_PROFILE),
    [profiles]
  );

  if (!settings || !env) {
    return (
      <div className="flex h-full items-center justify-center gap-3 text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" /> 正在加载…
      </div>
    );
  }

  return (
    <SidebarProvider
      open={sidebarOpen}
      onOpenChange={setSidebarOpen}
      className="h-full min-h-0 w-full"
    >
      <AppSidebar
        view={view}
        onNavigate={(v) => {
          // 从侧栏进入插件页：清掉 Profile 卡片带过来的预选，走默认 profile
          if (v === "plugins") setPluginsSeed(null);
          setView(v);
        }}
        env={env}
        settings={settings}
        runningInstanceCount={runningInstanceCount}
        upgradableCount={upgradableCount}
      />

      {/* 内容侧：顶栏 / 视图 / 状态栏都放在 SidebarInset 内，随侧栏收放一起让位 */}
      <SidebarInset className="min-w-0 overflow-hidden bg-transparent">
        {/* 顶栏（低优先级项按断点逐级收起，超窄窗口统一进「更多」菜单） */}
        <header className="flex h-11 shrink-0 items-center gap-1.5 border-b border-border bg-background/80 px-2 lg:gap-2 lg:px-4">
          <SidebarTrigger title="折叠 / 展开侧栏（Ctrl+B）" />
          {/* 抽屉态（< md）侧栏不可见，顶栏补上品牌标识 */}
          <img src="/dsh-logo.svg" alt="DSH" className="h-6 w-6 shrink-0 md:hidden" draggable={false} />
          <span className="eyebrow mr-1 hidden xl:inline">Environment</span>
          <Badge variant="outline" title={env.nodePath ?? ""} className="font-mono">
            <span className={`led ${env.node ? "bg-emerald-500 text-emerald-500 led-glow" : "bg-red-500"}`} />
            <span className="hidden sm:inline">node&nbsp;</span>
            {env.node ? `v${env.node}` : "未装"}
          </Badge>
          <Badge variant="outline" title={env.npmPath ?? ""} className="hidden font-mono lg:inline-flex">
            <span className={`led ${env.npm ? "bg-emerald-500" : "bg-red-500"}`} />
            npm {env.npm ?? "未装"}
          </Badge>
          <Badge variant="outline" className="hidden font-mono xl:inline-flex">{env.os}/{env.arch}</Badge>
          {/* 顶栏空白处也能拖窗口（data-tauri-drag-region 只管自己那一层，
              按钮之类的可点元素不受影响） */}
          <div className="flex-1" data-tauri-drag-region />
          {liveWebProcs.length > 0 && (
            <Button
              size="sm"
              onClick={() => liveWebProcs[0].webUrl && api.openUrl(liveWebProcs[0].webUrl).catch((e) => addToast("err", String(e)))}
              title={liveWebProcs.length === 1
                ? `打开 dsh 主界面：${liveWebProcs[0].webUrl}`
                : `${liveWebProcs.length} 个实例运行中，点击打开最新一个`}
            >
              <ExternalLink /> <span className="hidden lg:inline">打开 DSH 界面</span>
            </Button>
          )}
          <Button variant="outline" size="sm" className="hidden md:inline-flex" onClick={doCheckUpdate}>
            检查更新
          </Button>
          <Button
            variant={drawerOpen ? "secondary" : "outline"}
            size="sm"
            onClick={() => setDrawerOpen((v) => !v)}
            title="打开/收起实例终端"
          >
            <Terminal /> <span className="hidden lg:inline">实例终端</span>
            {runningInstanceCount > 0 && (
              <Badge variant="success" className="ml-0.5">{runningInstanceCount}</Badge>
            )}
          </Button>
          <Button variant="ghost" size="icon" title="设置" onClick={() => setShowSettings(true)}>
            <SettingsIcon className="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            title={resolved === "dark" ? "切换浅色" : "切换深色"}
            onClick={() => setTheme(resolved === "dark" ? "light" : "dark")}
          >
            {resolved === "dark" ? (
              <Sun className="h-4 w-4 animate-in fade-in zoom-in-50 duration-150" />
            ) : (
              <Moon className="h-4 w-4 animate-in fade-in zoom-in-50 duration-150" />
            )}
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="md:hidden" title="更多操作与环境信息">
                <MoreHorizontal className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
              <DropdownMenuLabel>操作</DropdownMenuLabel>
              {liveWebProcs.length > 0 && (
                <DropdownMenuItem
                  onSelect={() => {
                    const u = liveWebProcs[0].webUrl;
                    if (u) api.openUrl(u).catch((e) => addToast("err", String(e)));
                  }}
                >
                  <ExternalLink /> 打开 DSH 界面
                </DropdownMenuItem>
              )}
              <DropdownMenuItem onSelect={() => { doCheckUpdate(); }}>
                <RefreshCw /> 检查更新
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuLabel>环境</DropdownMenuLabel>
              <DropdownMenuItem disabled className="font-mono text-[11.5px]">
                node {env.node ? `v${env.node}` : "未装"}
              </DropdownMenuItem>
              <DropdownMenuItem disabled className="font-mono text-[11.5px]">
                npm {env.npm ?? "未装"} · {env.os}/{env.arch}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </header>

        {/* 更新 / 运行时横幅 */}
        {update && (
          <UpdateBanner
            status={update}
            applying={updateApplying}
            progress={updateProgress}
            onDismiss={() => setUpdate(null)}
            onOpenUrl={(u) => api.openUrl(u).catch((e) => addToast("err", String(e)))}
            onApply={doApplyUpdate}
          />
        )}
        {env && !env.node && (
          <Alert
            variant="destructive"
            className={`shrink-0 animate-in gap-1.5 rounded-none border-x-0 border-t-0 border-red-500/30 bg-red-500/10 px-4 py-2 text-[13px] fade-in slide-in-from-top-2 duration-300 ${runtimeJob ? "" : "pr-40"}`}
          >
            <XCircle />
            {runtimeJob ? (
              <>
                <AlertTitle className="font-normal">
                  正在安装内置 Node…
                  {runtimeJob.total > 0 &&
                    ` ${Math.round((runtimeJob.received / runtimeJob.total) * 100)}% (${(runtimeJob.received / 1048576).toFixed(1)}/${(runtimeJob.total / 1048576).toFixed(1)} MB)`}
                </AlertTitle>
                {runtimeJob.total > 0 && (
                  <AlertDescription>
                    <Progress
                      value={(runtimeJob.received / runtimeJob.total) * 100}
                      className="h-1.5 max-w-md bg-red-500/20"
                    />
                  </AlertDescription>
                )}
                <AlertDescription className="truncate font-mono text-[11px]">
                  {runtimeJob.log[runtimeJob.log.length - 1] ?? "连接镜像站…"}
                </AlertDescription>
              </>
            ) : (
              <>
                <AlertTitle className="font-normal">未检测到 Node.js（dsh 依赖 Node 运行）</AlertTitle>
                <AlertDescription>
                  可一键安装启动器内置 Node LTS（用户级、无需 root，默认走 npmmirror 镜像）
                </AlertDescription>
                <AlertAction>
                  <Button size="sm" onClick={doInstallRuntime}>一键安装 Node</Button>
                </AlertAction>
              </>
            )}
          </Alert>
        )}

        {/* 内容区（key 随视图变化：切换时重新挂载并播放入场动画）
            超窄窗口收紧内边距，把横向空间尽量留给表格与表单 */}
        <div
          key={view}
          className="min-h-0 flex-1 animate-in fade-in slide-in-from-bottom-2 overflow-y-auto p-3 duration-200 sm:p-4 lg:p-5"
        >
          {view === "versions" && (
            <div className="space-y-4">
              {/* 两张总览卡片：窄窗口单列堆叠，宽窗口并排 */}
              <div className="grid gap-3 lg:grid-cols-2">
                <Card className="p-4">
                  <div className="eyebrow mb-2.5">① Node 环境 —— 系统级 / 隔离级可切换</div>
                  <Tabs
                    value={settings.nodeSource}
                    onValueChange={(v) => doSetNodeSource(v as "auto" | "system" | "runtime")}
                  >
                    <TabsList className="mb-3 w-full">
                      <TabsTrigger value="auto" className="text-xs">自动</TabsTrigger>
                      <TabsTrigger value="system" className="text-xs">系统级</TabsTrigger>
                      <TabsTrigger value="runtime" className="text-xs">隔离（内置）</TabsTrigger>
                    </TabsList>
                  </Tabs>
                  {env.node ? (
                    <div className="flex items-center gap-2 text-sm">
                      <CheckCircle2 className="h-4 w-4 text-emerald-500" />
                      <span className="font-mono">Node v{env.node}</span>
                      <span className="text-xs text-muted-foreground">
                        {env.nodePath?.includes(".dsh-launcher") ? "（隔离 · 仅本软件使用）" : "（系统级）"}
                      </span>
                    </div>
                  ) : (
                    <div className="flex items-center gap-2 text-sm text-amber-500">
                      <XCircle className="h-4 w-4" />
                      {settings.nodeSource === "system" ? "系统级未检测到 Node" : "尚无可用 Node"}
                    </div>
                  )}
                  {settings.nodeSource !== "system" && !env.runtimeInstalled && (
                    <>
                      <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
                        安装<b className="text-foreground">隔离的内置 Node</b>（约 25MB）：仅写入启动器数据目录、仅供本软件使用，
                        与系统 Node 互不干扰；即使系统已有 Node 也可安装，装好后随时在上方切换。
                      </p>
                      <Button size="sm" className="mt-2" onClick={doInstallRuntime}>
                        安装隔离 Node
                      </Button>
                    </>
                  )}
                  {env.runtimeInstalled && (
                    <div className="mt-2 truncate font-mono text-[10.5px] text-muted-foreground">
                      内置运行时已就绪：{env.runtimeDir}
                    </div>
                  )}
                  {runtimeJob && (
                    <div className="mt-2.5 space-y-1.5 text-xs text-muted-foreground">
                      {runtimeJob.total > 0 && (
                        <Progress value={(runtimeJob.received / runtimeJob.total) * 100} className="h-1.5" />
                      )}
                      <div className="flex items-center gap-1.5">
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        {runtimeJob.total > 0
                          ? `${Math.round((runtimeJob.received / runtimeJob.total) * 100)}% (${(runtimeJob.received / 1048576).toFixed(1)}/${(runtimeJob.total / 1048576).toFixed(1)} MB)`
                          : "连接镜像站…"}
                      </div>
                    </div>
                  )}
                </Card>
                <Card className="p-4">
                  <div className="eyebrow mb-2.5">② 当前 DSH 版本 —— 所有 Profile 实例基于它运行</div>
                  {installed.length > 0 ? (
                    <>
                      <Select
                        value={settings.activeVersion}
                        onValueChange={doSetActiveVersion}
                        disabled={instancesRunning}
                      >
                        <SelectTrigger className="w-full max-w-sm font-mono">
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
                      {activeInst?.location && (
                        <div className="mt-2 truncate font-mono text-[10.5px] text-muted-foreground">
                          安装位置：{activeInst.location}
                        </div>
                      )}
                      {instancesRunning && (
                        <p className="mt-2 text-[11.5px] text-amber-500">
                          有 Profile 实例正在运行，停止所有实例后才能切换版本
                        </p>
                      )}
                    </>
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
                <Alert variant="destructive" className="pr-28">
                  <TriangleAlert />
                  <AlertTitle>拉取官方版本列表失败</AlertTitle>
                  <AlertDescription>{remoteErr}</AlertDescription>
                  <AlertAction>
                    <Button variant="outline" size="sm" onClick={refreshRemote}>重试</Button>
                  </AlertAction>
                </Alert>
              )}

              {!remoteErr && rows.length === 0 && !remoteLoading && (
                <Card className="p-10 text-center text-muted-foreground">
                  <div className="mb-2 text-3xl">📦</div>
                  点击下方「刷新版本」从 registry 拉取 @deepseek-ai/dsh 的官方发布版本
                </Card>
              )}

              {/* 版本筛选工具栏 */}
              <div className="flex flex-wrap items-center gap-2">
                <Tabs value={verScope} onValueChange={(v) => setVerScope(v as "all" | "installed")}>
                  <TabsList>
                    <TabsTrigger value="all" className="px-3 text-xs">全部版本</TabsTrigger>
                    <TabsTrigger value="installed" className="px-3 text-xs">已安装</TabsTrigger>
                  </TabsList>
                </Tabs>
                <Tabs value={verType} onValueChange={(v) => setVerType(v as "all" | "stable" | "pre")}>
                  <TabsList>
                    <TabsTrigger value="all" className="px-3 text-xs">全部类型</TabsTrigger>
                    <TabsTrigger value="stable" className="px-3 text-xs">正式版（含 RC）</TabsTrigger>
                    <TabsTrigger value="pre" className="px-3 text-xs">预发布</TabsTrigger>
                  </TabsList>
                </Tabs>
                <span className="flex-1" />
                <span className="text-xs text-muted-foreground">
                  {filteredVerRows.length} / {rows.length} 个版本
                </span>
                <Button variant="outline" size="sm" disabled={remoteLoading} onClick={refreshRemote}>
                  <RefreshCw className={remoteLoading ? "animate-spin" : ""} /> 刷新版本
                </Button>
              </div>

              {rows.length > 0 && (
                <div className="flex items-start gap-1.5 text-xs text-muted-foreground">
                  <Info className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                  <span>从高版本降回低版本后，若 DSH 出现任何报错问题，请交给 Agent 定位问题并处理</span>
                </div>
              )}

              {/* 版本表 */}
              {rows.length > 0 && (
                <Card className="py-0">
                  <Table className="min-w-[600px]">
                    <TableHeader>
                      <TableRow className="bg-muted/40 hover:bg-muted/40">
                        <TableHead className="h-9 pl-4 text-[10.5px] uppercase tracking-wider">版本</TableHead>
                        <TableHead className="h-9 text-[10.5px] uppercase tracking-wider">发布日期</TableHead>
                        <TableHead className="h-9 text-[10.5px] uppercase tracking-wider">大小</TableHead>
                        <TableHead className="h-9 text-[10.5px] uppercase tracking-wider">状态</TableHead>
                        <TableHead className="h-9 pr-4 text-right text-[10.5px] uppercase tracking-wider">操作</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {filteredVerRows.map((r) => (
                        <VersionRow
                          key={`${r.version}-${r.installed?.source ?? "remote"}`}
                          row={r}
                          isLatestTag={latestVersion === r.version}
                          busy={installJob !== null}
                          upgradeTo={
                            !hasLatestInstalled &&
                            r.installed && r.installed.version !== "unknown" && latestVersion &&
                            cmpVer(latestVersion, r.version) > 0
                              ? latestVersion : null
                          }
                          onUpgrade={(v) => doInstall(v, false)}
                          isActive={settings.activeVersion === r.version}
                          switchLocked={instancesRunning}
                          onSetActive={doSetActiveVersion}
                          onInstall={(v, force) => doInstall(v, force)}
                          onUninstall={doUninstall}
                          onReveal={(p) => api.reveal(p).catch((e) => addToast("err", String(e)))}
                        />
                      ))}
                      {filteredVerRows.length === 0 && (
                        <TableRow>
                          <TableCell colSpan={5} className="py-10 text-center text-muted-foreground">
                            没有符合筛选条件的版本
                          </TableCell>
                        </TableRow>
                      )}
                    </TableBody>
                  </Table>
                </Card>
              )}
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
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => setTrashOpen(true)}
                  title="回收站：删除的 profile 会移到这里，可还原或彻底删除"
                >
                  <Trash2 /> 回收站
                </Button>
                <Button size="sm" variant="outline" onClick={refreshProfiles} title="重新扫描 $DSH_HOME/profiles">
                  <RefreshCw /> 重扫目录
                </Button>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-xs text-muted-foreground">启动方式</span>
                <Tabs
                  value={settings.launchMode === "detached" ? "detached" : "child"}
                  onValueChange={(v) => doSetLaunchMode(v as "child" | "detached")}
                >
                  <TabsList>
                    <TabsTrigger value="child" className="text-xs">子进程</TabsTrigger>
                    <TabsTrigger value="detached" className="text-xs">独立进程</TabsTrigger>
                  </TabsList>
                </Tabs>
                <span className="text-[11px] text-muted-foreground">
                  {settings.launchMode === "detached"
                    ? "独立进程随系统常驻：关闭启动器后 DSH 继续运行，重启启动器后会自动扫描识别，日志写入 ~/.dsh-launcher/instance-logs"
                    : "子进程模式：日志回传「实例终端」，启动器退出时结束所有 DSH"}
                </span>
              </div>
              <div className="rounded-lg border border-amber-500/25 bg-amber-500/5 p-2.5 text-[11.5px] leading-relaxed text-muted-foreground">
                <span className="text-foreground">Target 标签</span>
                {" "}按各 profile 的 <span className="font-mono">package.json</span> 中
                <span className="font-mono"> dsh.profile.bundles </span>
                识别运行形态：
                <Badge variant="info">Web</Badge>
                含 <span className="font-mono"> @deepseek-ai/dsh-web-app </span>
                插件，启动后从日志识别地址并打开浏览器（当前唯一支持的启动方式）；
                <Badge variant="secondary">Desktop</Badge>
                为桌面应用外壳；
                <Badge variant="outline">未识别</Badge>
                暂无可用的启动方式。新 Target 的启动方式将在后续版本扩展。
                若插件导致启动异常，到「插件管理」页停用可疑插件后重启实例。
              </div>
              <div className="space-y-2">
                {instanceRows.map((row) => {
                  const phaseText =
                    row.phase === "starting" ? "启动中…"
                    : row.phase === "ready" ? "启动成功"
                    : row.phase === "failed" ? `启动失败${row.code != null ? `（退出码 ${row.code}）` : ""}`
                    : row.phase === "external"
                    ? row.source === "detached" ? "运行中（独立进程）"
                    : row.source === "port" ? "运行中（端口探测）"
                    : "运行中（外部启动）"
                    : "未运行";
                  const canStop = row.phase === "starting" || row.phase === "ready" || row.phase === "external";
                  // 独立进程是从日志文件解析出的地址，phase 是 external 但同样能打开
                  const canOpen = !!row.webUrl && row.phase !== "failed" && row.phase !== "stopped";
                  const targetMeta = TARGET_META[row.target];
                  const canStart = targetMeta.launchable;
                  const expanded = !!expandedProfiles[row.profile];
                  return (
                    <Card key={row.key} className="gap-0 py-0">
                      <Collapsible
                        open={expanded}
                        onOpenChange={(o) => setExpandedProfiles((m) => ({ ...m, [row.profile]: o }))}
                      >
                        {/* 窄窗口：状态块独占一行，操作按钮整排换到第二行（否则会被卡片裁掉） */}
                        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 p-3">
                          {row.phase === "starting" ? (
                            <Loader2 className="h-4 w-4 shrink-0 animate-spin text-amber-500" />
                          ) : row.phase === "ready" ? (
                            <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-500" />
                          ) : row.phase === "failed" ? (
                            <XCircle className="h-4 w-4 shrink-0 text-red-500" />
                          ) : (
                            <span className={`h-2.5 w-2.5 shrink-0 rounded-full ${row.phase === "external" ? "bg-sky-500" : "bg-muted-foreground/30"}`} />
                          )}
                          <div className="min-w-0 grow basis-[calc(100%-1.75rem)] xl:basis-0">
                            <div className="flex items-center gap-1.5">
                              <span className="font-mono text-[13px] font-semibold">
                                {row.profile || (row.port != null ? `:${row.port}` : "（未命名实例）")}
                              </span>
                              <Badge variant={targetMeta.variant} title={targetMeta.desc}>
                                {targetMeta.label}
                              </Badge>
                              {row.reserved && (
                                <Badge
                                  variant="outline"
                                  title="dsh 内置保留 profile：不可重命名/删除，避免核心数据丢失"
                                >
                                  内置
                                </Badge>
                              )}
                            </div>
                            <div className="text-[11px] text-muted-foreground">
                              {phaseText}
                              {row.pid ? ` · PID ${row.pid}` : ""}
                              {row.port != null ? ` · :${row.port}` : ""}
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
                          {/* 恢复模式入口：恢复模式以官方 web 模板新建（不再复制当前 web），
                              入口仍挂在原版 dsh 内置的 web profile 行上；已存在就不再显示 */}
                          {row.reserved && row.target === "web" && !recoveryExists && (
                            <Button
                              size="sm"
                              variant="outline"
                              title={`用 dsh 的 --from-default-profile web 新建一份只含官方插件、换邻近端口的「${RECOVERY_PROFILE}」`}
                              onClick={() => setRecoveryOpen(true)}
                            >
                              <ShieldPlus /> 恢复模式
                            </Button>
                          )}
                          {canStop ? (
                            <>
                              {row.profile && (
                                <Button
                                  size="sm"
                                  variant="outline"
                                  disabled={restartingProfile === row.profile}
                                  onClick={() => doRestartProfile(row.profile)}
                                  title="停止并按当前启动方式重新启动"
                                >
                                  {restartingProfile === row.profile ? (
                                    <Loader2 className="animate-spin" />
                                  ) : (
                                    <RotateCw />
                                  )}{" "}
                                  重启
                                </Button>
                              )}
                              <Button
                                size="sm"
                                variant="destructive"
                                disabled={restartingProfile === row.profile}
                                onClick={() => doStopInstance(row)}
                              >
                                <Square /> 停止
                              </Button>
                            </>
                          ) : (
                            <Button
                              size="sm"
                              variant="outline"
                              disabled={installed.length === 0 || !canStart || restartingProfile === row.profile || startingProfile === row.profile}
                              title={installed.length === 0
                                ? "请先在「版本与安装」页安装 dsh"
                                : !canStart
                                ? `${targetMeta.desc}——当前仅支持启动 Web 类型 profile`
                                : row.phase === "failed"
                                ? "重新启动该 profile"
                                : `基于当前版本（${settings.activeVersion}）启动 ${row.profile}`}
                              onClick={() => doStartProfile(row.profile)}
                            >
                              {startingProfile === row.profile ? (
                                <Loader2 className="animate-spin" />
                              ) : (
                                <Play />
                              )}{" "}
                              启动
                            </Button>
                          )}
                          {row.logFile && (
                            <Button
                              variant="ghost"
                              size="icon"
                              title={`打开日志文件夹：${row.logFile}`}
                              onClick={() => revealPath(row.logFile!.replace(/[\\/][^\\/]*$/, ""))}
                            >
                              <FileText />
                            </Button>
                          )}
                          {row.profile && (
                            <Button
                              variant="ghost"
                              size="icon"
                              title="复制实例：把该 profile 的配置目录拷贝为新实例"
                              onClick={() => setCopySource(row.profile)}
                            >
                              <CopyPlus />
                            </Button>
                          )}
                          {/* dsh 内置保留 profile（headless/web/desktop）不提供改名与删除 */}
                          {row.profile && !row.reserved && (
                            <>
                              <Button
                                variant="ghost"
                                size="icon"
                                title="重命名 profile（只改目录名，配置原样保留）"
                                onClick={() => setRenameTarget(row.profile)}
                              >
                                <Pencil />
                              </Button>
                              <Button
                                variant="ghost"
                                size="icon"
                                title="删除 profile（移入 ~/.dsh-launcher/deleted-profiles，可找回）"
                                onClick={() => setDeleteTarget(row.profile)}
                              >
                                <Trash2 />
                              </Button>
                            </>
                          )}
                          <Button
                            variant="ghost"
                            size="icon"
                            title="打开「插件管理」并选中该 profile"
                            onClick={() => {
                              setPluginsSeed(row.profile);
                              setView("plugins");
                            }}
                          >
                            <Puzzle />
                          </Button>
                          {/* 无名实例（终端 `dsh web` 没带 --profile）没有对应 profile，不提供配置面板 */}
                          {row.profile && (
                            <CollapsibleTrigger asChild>
                              <Button
                                variant="ghost"
                                size="icon"
                                title={expanded ? "收起配置面板" : "展开配置面板（快捷配置 / cordis.patch.yml / package.json）"}
                              >
                                <ChevronDown className={`h-4 w-4 transition-transform ${expanded ? "rotate-180" : ""}`} />
                              </Button>
                            </CollapsibleTrigger>
                          )}
                        </div>
                        {row.profile && (
                          <CollapsibleContent>
                            <div className="border-t border-border px-3 pb-4 pt-3">
                              <ProfileConfigPanel profile={row.profile} target={row.target} onToast={addToast} />
                            </div>
                          </CollapsibleContent>
                        )}
                      </Collapsible>
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

          {view === "plugins" && (
            <PluginsView
              profiles={profiles.map((p) => p.name)}
              initialProfile={pluginsSeed}
              onToast={addToast}
            />
          )}
          {view === "models" && <ModelConfigView onToast={addToast} />}
          {view === "config" && <ConfigView onToast={addToast} />}
          {view === "credentials" && <CredentialsView onToast={addToast} />}
        </div>

        {/* 状态栏：窄窗口只留官方源，其余按断点逐级收起 */}
        <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-border bg-card px-3 text-[11px] text-muted-foreground lg:px-4">
          <span className="truncate">官方源 {env.registry}</span>
          <span className="hidden truncate font-mono lg:inline" title={env.dshHome}>{env.dshHome}</span>
          <span className="ml-auto hidden shrink-0 xl:inline">退出启动器会结束所有内嵌 dsh 进程；关闭窗口最小化到托盘</span>
        </footer>
      </SidebarInset>

      <ProcessSidePanel
        procs={panelProcs}
        activeId={activeProc}
        open={drawerOpen}
        onClose={() => setDrawerOpen(false)}
        onSelect={setActiveProc}
        onStop={doStopProc}
        onReadLog={readInstanceLog}
        onReveal={revealPath}
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

      <SettingsDrawer
        open={showSettings}
        initial={settings}
        env={env}
        onSave={doSaveSettings}
        onClose={() => setShowSettings(false)}
        onReveal={(p) => api.reveal(p).catch((e) => addToast("err", String(e)))}
      />

      {/* Toasts（sonner） */}
      <Toaster position="bottom-right" richColors closeButton />

      {/* 复制 profile 实例 */}
      <CopyProfileDialog
        source={copySource}
        existing={profiles.map((p) => p.name)}
        onClose={() => setCopySource(null)}
        onToast={addToast}
        onCopied={() => refreshProfiles()}
      />

      {/* 回收站 */}
      <TrashDialog
        open={trashOpen}
        onClose={() => setTrashOpen(false)}
        onToast={addToast}
        onChanged={() => { refreshProfiles(); refreshInstances(); }}
      />

      {/* 重命名 profile */}
      <RenameProfileDialog
        name={renameTarget}
        existing={profiles.map((p) => p.name)}
        onClose={() => setRenameTarget(null)}
        onToast={addToast}
        onRenamed={() => {
          refreshProfiles();
          refreshInstances();
          api.getSettings().then(setSettings).catch(() => undefined);
        }}
      />

      {/* 删除 profile 确认（内置保留 profile 不会走到这里） */}
      <AlertDialog open={deleteTarget != null} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除 profile「{deleteTarget}」？</AlertDialogTitle>
            <AlertDialogDescription>
              配置目录会被移动到 <span className="font-mono">~/.dsh-launcher/deleted-profiles/</span>
              （不会直接销毁，可手动找回）。dsh 内置保留 profile 已受保护、不会出现在这里；
              实例运行中会先被拒绝。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={confirmDeleteProfile}>
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 创建恢复模式确认 */}
      <AlertDialog open={recoveryOpen} onOpenChange={(o) => !o && setRecoveryOpen(false)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>创建恢复模式「{RECOVERY_PROFILE}」？</AlertDialogTitle>
            <AlertDialogDescription>
              让 dsh 用它自带 <span className="font-mono">web</span> 模板新建一份独立 profile
              （<span className="font-mono">--from-default-profile web</span>，不复制你当前的 web）：
              内置插件只有官方 <span className="font-mono">base</span> 与{" "}
              <span className="font-mono">web-app</span>，随后自动写入快捷配置并换一个邻近的空闲端口。
              相当于「原版 web 换个端口运行」，用于排查第三方插件把 web 跑挂的情况。
              不会改动官方 web 本身，也不会自动创建。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction variant="default" disabled={recoveryBusy} onClick={confirmCreateRecovery}>
              {recoveryBusy && <Loader2 className="animate-spin" />} 创建
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 卸载确认 */}
      <AlertDialog open={pendingUninstall != null} onOpenChange={(o) => !o && setPendingUninstall(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>卸载 dsh {pendingUninstall}？</AlertDialogTitle>
            <AlertDialogDescription>将删除该版本的安装目录，此操作不可撤销。</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={confirmUninstall}>
              卸载
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SidebarProvider>
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


