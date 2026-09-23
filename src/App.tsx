import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  RefreshCw, Settings as SettingsIcon,
  ExternalLink, Play, Square, CheckCircle2, XCircle, Loader2, Sun, Moon,
  TriangleAlert, SlidersHorizontal, CopyPlus, Info, RotateCw, FileText, ScrollText,
  Pencil, Trash2, ShieldPlus, Rocket, Wand2,
  Home, Package, KeyRound, BarChart3, Terminal, Monitor, Download,
} from "lucide-react";
import { toast } from "sonner";
import { api, events } from "./api";
import { useTheme } from "@/lib/theme";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Toaster } from "@/components/ui/sonner";
import { cmpVer } from "@/lib/version";
import { SidebarInset, SidebarProvider, SidebarTrigger } from "@/components/ui/sidebar";
import { useSidebarOpen } from "@/hooks/use-layout";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert";
import CopyProfileDialog from "./components/CopyProfileDialog";
import RenameProfileDialog from "./components/RenameProfileDialog";
import TrashDialog from "./components/TrashDialog";
import { Progress } from "@/components/ui/progress";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogMedia, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import InstallCard from "./components/InstallCard";
import CredentialsView from "./components/CredentialsView";
import StatsView from "./components/StatsView";
import LogsView from "./components/LogsView";
import ProfileWorkspace from "./components/ProfileWorkspace";
import QuickActionsView from "./components/QuickActionsView";
import TerminalPanel from "./components/TerminalPanel";
import { useTerminalInline } from "./hooks/use-terminal-host";
import VersionRow from "./components/VersionRow";
import SettingsDrawer from "./components/SettingsDrawer";
import UpdateBanner from "./components/UpdateBanner";
import AppSidebar from "./components/AppSidebar";
import DshChangelogDialog from "./components/DshChangelogDialog";
import { CommandPalette, type PaletteCommand } from "./components/CommandPalette";
import { useTerminalJobs } from "./hooks/use-terminal-jobs";
import { usePluginJobs } from "./hooks/use-plugin-jobs";
import type {
  EnvironmentInfo, InstalledVersion, StarterUpdateStatus, ProcEntry,
  ProcExitEvent, ProcLogEvent, ProfileInfo, ProfileInstance, ProfileTarget,
  RegistryInfo, Settings as SettingsT, View, VersionChange, ProfileVersionInfo,
  InstallFinishedEvent, RuntimeFinishedEvent, TerminalTaskRef,
} from "./types";

/** 恢复模式 profile 名（与后端 profile_cfg::RECOVERY_PROFILE 保持一致） */
const RECOVERY_PROFILE = "web-Recovery";

/**
 * 各 Target 的标签展示与启动支持状态；新增 Target（如 CLI）在此扩展。
 * Target 由后端按 profile 的 package.json name（桌面运行时）与 dsh.profile.bundles 识别。
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
    desc: "未识别 Target（package.json 的 name 与 bundles 中无已知 Target）",
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
  const [update, setUpdate] = useState<StarterUpdateStatus | null>(null);
  const [updateApplying, setUpdateApplying] = useState(false);
  const [updateProgress, setUpdateProgress] = useState<{ received: number; total: number } | null>(null);
  const [pendingUninstall, setPendingUninstall] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [profiles, setProfiles] = useState<ProfileInfo[]>([]);
  const [procs, setProcs] = useState<Record<number, ProcEntry>>({});
  const [activeProc, setActiveProc] = useState<number | null>(null);
  const [terminalOpen, setTerminalOpen] = useState(false);
  /** 通用终端面板当前聚焦的任务；null = 未选（面板显示引导占位） */
  const [terminalTask, setTerminalTask] = useState<TerminalTaskRef | null>(null);
  const terminalInline = useTerminalInline();
  const runtimeBusy = useRef(false);
  const [instances, setInstances] = useState<ProfileInstance[]>([]);
  const [view, setView] = useState<View>("quick");
  /** 凭据页首次访问后保持挂载（keep-alive）：切页不丢未保存的编辑（见视图区底部） */
  const [credSeen, setCredSeen] = useState(false);
  useEffect(() => {
    if (view === "credentials") setCredSeen(true);
  }, [view]);
  const [verScope, setVerScope] = useState<"all" | "installed">("all");
  /** 更新日志对话框当前定位的 dsh 版本（null = 关闭） */
  const [notesVersion, setNotesVersion] = useState<string | null>(null);
  const [verType, setVerType] = useState<"all" | "stable" | "pre">("all");
  /** 工作台右列当前打开的 profile；null = 未选中（左列点「配置」进入） */
  const [selectedProfile, setSelectedProfile] = useState<string | null>(null);
  /** 正在重启的 profile（停止→等待消失→再拉起） */
  const [restartingProfile, setRestartingProfile] = useState<string | null>(null);
  /** 启动请求在途的 profile：按钮转圈并禁用，避免连点重复启动 */
  const [startingProfile, setStartingProfile] = useState<string | null>(null);
  /** 启动/重启在途的 profile 集合（ref 版）：连点时 state 还没重渲染，
   *  只靠 state 判断会放第二次请求过去 */
  const startingProfilesRef = useRef<Set<string>>(new Set());
  const restartingRef = useRef(false);
  /** 各 profile 的绑定启动版本（上次真正把它跑起来的 dsh 版本）：profile → version */
  const [boundVersions, setBoundVersions] = useState<Record<string, string>>({});
  /** 被版本变化闸门拦下的启动，等待用户在确认框里取舍；null = 关闭 */
  const [versionWarn, setVersionWarn] = useState<VersionChange | null>(null);
  /** 复制实例对话框的源 profile；null = 关闭 */
  const [copySource, setCopySource] = useState<string | null>(null);
  /** 本次复制来自版本风险框的「复制试用」：复制成功后直接启动副本 */
  const [copyAndStart, setCopyAndStart] = useState(false);
  /** 重命名对话框的目标 profile；null = 关闭 */
  const [renameTarget, setRenameTarget] = useState<string | null>(null);
  /** 删除确认的目标 profile；null = 关闭 */
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  /** 回收站（删除的 profile 可还原 / 彻底删除） */
  const [trashOpen, setTrashOpen] = useState(false);
  /** 恢复模式创建确认 */
  const [recoveryOpen, setRecoveryOpen] = useState(false);
  const [recoveryBusy, setRecoveryBusy] = useState(false);
  /** 命令面板（⌘K / Ctrl+K）是否打开 */
  const [paletteOpen, setPaletteOpen] = useState(false);

  const procsRef = useRef<Record<number, ProcEntry>>({});
  procsRef.current = procs;
  const settingsRef = useRef<SettingsT | null>(null);
  settingsRef.current = settings;
  const instancesRef = useRef<ProfileInstance[]>([]);
  instancesRef.current = instances;

  const { theme, resolved, setTheme } = useTheme();

  /** 侧栏开合：默认跟随窗口档位（宽展开 / 窄收成图标栏 / 超窄走抽屉），手动可覆盖 */
  const [sidebarOpen, setSidebarOpen] = useSidebarOpen();

  const addToast = useCallback((kind: "ok" | "err" | "info", text: string) => {
    if (kind === "ok") toast.success(text);
    else if (kind === "err") {
      toast.error(text, { duration: 7000 });
      // 展示给用户的报错同时落到 logs/ui.log：用户截图往往只有一句话，
      // 日志里有完整上下文，排查时不用反过来追问
      api.logUi("error", text).catch(() => undefined);
    } else toast.info(text);
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
    try {
      // 绑定版本跟着实例一起刷：新起的实例活过后端才会落库，行内小字随轮询自然跟上。
      // 版本清单失败不能连累实例列表刷新（否则 3s 轮询整体卡死且无提示）
      const [list, versions] = await Promise.all([
        api.listProfileInstances(),
        api.listProfileVersions().catch(() => [] as ProfileVersionInfo[]),
      ]);
      setInstances(list);
      setBoundVersions(Object.fromEntries(versions.map((v) => [v.profile, v.version])));
    } catch { /* ignore */ }
  }, []);

  // ── 系统任务（dsh 版本安装 / Node 安装）：事件流统一进 useTerminalJobs，
  //    日志与终态记录供通用终端面板展示；toast/设当前版本等副作用留在下面回调里 ──
  const onInstallFinished = useCallback(async (e: InstallFinishedEvent) => {
    if (!e.success) {
      addToast("err", `dsh ${e.version} 安装失败：${e.message.split("\n")[0]}`);
      return;
    }
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
  }, [addToast, refreshInstalled]);
  const onRuntimeFinished = useCallback((e: RuntimeFinishedEvent) => {
    addToast(e.ok ? "ok" : "err", e.message);
  }, [addToast]);
  const {
    tasks: sysTasks, installTask, nodeTask,
    startInstall, startNode,
    fail: failSystemTask, clearFinished: clearFinishedSystemTasks,
  } = useTerminalJobs({ onInstallFinished, onRuntimeFinished });

  // ── 插件任务（安装/卸载/升级/clone）：原「插件页内置终端」的数据源上提到 App，
  //    通用终端面板与插件页状态条共用同一份；任务结束后 PluginsTab 靠 tick 刷新列表 ──
  const [pluginJobsTick, setPluginJobsTick] = useState(0);
  const {
    jobs: pluginJobs, activeId: pluginActiveId,
    runningCount: pluginRunningCount, cancel: cancelPluginJob, clear: clearPluginJobs,
  } = usePluginJobs({
    onStarted: (e) => {
      setTerminalTask({ kind: "plugin", id: String(e.jobId) });
      setTerminalOpen(true);
    },
    onFinished: (e) => {
      const tag = e.cancelled ? "已取消" : e.ok ? "完成" : "失败";
      // 失败时优先显示对症建议（供应链策略 / 构建脚本 / 鉴权 / 404…），而不是笼统的「看终端」
      const detail = e.ok ? "" : e.hint ? `：${e.hint.split("\n")[0]}` : "：在终端面板查看输出";
      addToast(e.cancelled ? "info" : e.ok ? "ok" : "err", `${e.label} ${tag}${detail}`);
      setPluginJobsTick((t) => t + 1);
    },
  });
  /** 插件页「在终端中查看」：聚焦最近一个插件任务并打开面板 */
  const openPluginTerminal = useCallback(() => {
    const id = pluginActiveId ?? pluginJobs[0]?.id ?? null;
    if (id != null) setTerminalTask({ kind: "plugin", id: String(id) });
    setTerminalOpen(true);
  }, [pluginActiveId, pluginJobs]);
  const doRetryPluginJob = useCallback((jobId: number) => {
    // 新任务开始时 onStarted 事件会把面板聚焦过去，这里只报个信
    api.retryPluginJob(jobId)
      .then(() => addToast("info", "已重试：输出见终端面板"))
      .catch((e) => addToast("err", String(e)));
  }, [addToast]);
  const doApprovePluginBuilds = useCallback((jobId: number) => {
    api.approvePluginBuilds(jobId)
      .then(() => addToast("info", "已写入 allowBuilds，正在重跑（输出见终端面板）"))
      .catch((e) => addToast("err", String(e)));
  }, [addToast]);

  // ── 首次初始化（dsh 还没生成 $DSH_HOME 时） ────────────────
  // dsh 的数据目录是「第一次运行 dsh」才生成的；在那之前没有任何 profile，
  // 基于 profile 的能力（启动实例、快捷配置、插件管理）全都无从下手。
  // 这里给出一条明确的引导：跑一次 `dsh web`（等价 --profile web）把 dsh 初始化出来。
  const [initBusy, setInitBusy] = useState(false);
  const initBusyRef = useRef(false);
  const doInitDsh = useCallback(async () => {
    if (initBusyRef.current) return;
    initBusyRef.current = true;
    setInitBusy(true);
    try {
      const info = await api.initDsh();
      addToast(
        "ok",
        `已启动 dsh 首次初始化（PID ${info.id}）：正在生成数据目录与内置 web profile…`
      );
      await refreshInstances();
      // dsh 写出 profiles/ 需要一两秒：这里补两次重扫，另外空列表期间还有 3 秒轮询兜底
      window.setTimeout(() => void refreshProfiles(), 1500);
      window.setTimeout(() => void refreshProfiles(), 4000);
    } catch (e) {
      addToast("err", `初始化失败：${e}`);
    } finally {
      initBusyRef.current = false;
      setInitBusy(false);
    }
  }, [addToast, refreshInstances, refreshProfiles]);

  // 还没发现任何 profile 时每 3 秒重扫一次：用户可能在终端里自己跑过 `dsh web`，
  // 或刚点了上面的初始化 —— 界面自己长出来，不必手动点「重扫目录」。
  useEffect(() => {
    if (profiles.length > 0) return;
    const t = setInterval(() => { void refreshProfiles(); }, 3000);
    return () => clearInterval(t);
  }, [profiles.length, refreshProfiles]);

  // 首次初始化完成（profiles 从空变有内容）后补上默认 profile：
  // 否则「启动」按钮没有目标，用户还得自己去下拉框里挑一次。
  useEffect(() => {
    if (profiles.length === 0) return;
    const cur = settingsRef.current;
    if (!cur || cur.defaultProfile) return;
    const next = {
      ...cur,
      defaultProfile: profiles.find((p) => p.target === "web")?.name ?? profiles[0].name,
    };
    setSettings(next);
    api.saveSettings(next).catch(() => undefined);
  }, [profiles]);


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
          api.checkStarterUpdate().then((u) => {
            if (u.available || u.mode === "error") setUpdate(u);
          }).catch(() => undefined);
        }
      } catch (e) {
        addToast("err", `初始化失败: ${e}`);
      }
      try {
        const running = await api.installRunning();
        if (running) startInstall(running);
      } catch { /* ignore */ }
    })();
  }, [refreshRemote, refreshInstances, addToast, startInstall]);

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
    track(events.onStarterUpdate?.((s) => {
      setUpdate(s);
      // 静默自动更新：后端已经在下载了，这里把进度条顶起来。
      // 必须和后端 emit_startup_checks 的门禁一致（builtin + autoInstall + 不需要提权），
      // 否则 deb/rpm 需要管理员授权时后端不会自动装，这里却把按钮卡在「下载安装中…」再也点不动。
      if (s.available && settingsRef.current?.autoInstallUpdate && s.mode === "builtin" && !s.needsElevation) {
        setUpdateApplying(true);
        setUpdateProgress({ received: 0, total: 0 });
      }
    }));
    track(events.onStarterUpdateProgress?.((p) => setUpdateProgress(p)));
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
      // 面板还没有聚焦任何实例时，自动跟上新来的日志
      setTerminalTask((cur) => (cur?.kind === "instance" ? cur : { kind: "instance", id: String(e.id) }));
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
    track(events.onToast?.((text) => addToast("err", text)));
    return () => { for (const u of unlisteners) u(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [addToast, refreshInstalled]);

  // ── 动作 ───────────────────────────────────
  const doInstall = useCallback(async (version: string, force: boolean) => {
    startInstall(version);
    setTerminalTask({ kind: "dshInstall", id: version });
    setTerminalOpen(true);
    try {
      await api.install(version, force);
    } catch (e) {
      failSystemTask("dshInstall", version, String(e));
      addToast("err", `安装失败: ${e}`);
    }
  }, [addToast, startInstall, failSystemTask]);

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

  /** 面板任务选中：实例任务同步回 activeProc（导出/打开 Web 的闭包以它为键） */
  const selectTerminalTask = useCallback((ref: TerminalTaskRef | null) => {
    setTerminalTask(ref);
    if (ref?.kind === "instance") setActiveProc(Number(ref.id));
  }, []);
  /** 清理面板里的全部已结束任务 */
  const clearFinishedTerminalTasks = useCallback(() => {
    setProcs((m) => {
      const next = Object.fromEntries(Object.entries(m).filter(([, p]) => !p.exited)) as Record<number, ProcEntry>;
      setActiveProc((a) => (a != null && next[a] ? a : (Object.values(next)[0]?.id ?? null)));
      return next;
    });
    clearFinishedSystemTasks();
    clearPluginJobs();
    setTerminalTask((cur) => {
      if (!cur) return cur;
      if (cur.kind === "instance") {
        const p = procsRef.current[Number(cur.id)];
        return p && !p.exited ? cur : null;
      }
      if (cur.kind === "plugin") {
        const j = pluginJobs.find((x) => String(x.id) === cur.id);
        return j?.running ? cur : null;
      }
      const t = sysTasks.find((x) => x.kind === cur.kind && x.id === cur.id);
      return t?.running ? cur : null;
    });
  }, [clearFinishedSystemTasks, clearPluginJobs, sysTasks, pluginJobs]);

  const doInstallRuntime = useCallback(async () => {
    if (runtimeBusy.current) return;
    runtimeBusy.current = true;
    startNode();
    setTerminalTask({ kind: "nodeInstall", id: "node" });
    setTerminalOpen(true);
    try {
      const msg = await api.installRuntime();
      addToast("ok", msg);
      setEnv(await api.getEnvironment());
    } catch (e) {
      // 后端失败路径也会 emit runtime-finished；fail 只动在途任务，双保险
      failSystemTask("nodeInstall", "node", String(e));
      addToast("err", String(e));
    } finally {
      runtimeBusy.current = false;
    }
  }, [addToast, startNode, failSystemTask]);

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

  /** 单个 profile 的启动方式：未单独设置时跟随全局默认（settings.launchMode）。只负责落盘 */
  const applyProfileLaunchMode = useCallback(async (profile: string, v: "child" | "detached") => {
    const s = settingsRef.current;
    if (!s || (s.profileLaunchMode?.[profile] ?? s.launchMode) === v) return;
    const next = { ...s, profileLaunchMode: { ...s.profileLaunchMode, [profile]: v } };
    setSettings(next);
    // 同步写 ref：紧接着的「保存并重启」会在同一次事件里读到它，不能等下一帧渲染
    settingsRef.current = next;
    try {
      await api.saveSettings(next);
      addToast(
        "ok",
        `「${profile}」启动方式已切换为${v === "detached" ? "独立进程（后台常驻，日志写入 ~/.dsh-starter/instance-logs）" : "子进程（随启动器退出结束）"}`
      );
    } catch (e) {
      setSettings(s);
      settingsRef.current = s;
      addToast("err", `切换失败: ${e}`);
    }
  }, [addToast]);

  /** 运行中的 profile 改启动方式要先确认：确认后保存并立即重启，取消则什么都不变 */
  const [launchModeAsk, setLaunchModeAsk] = useState<{ profile: string; mode: "child" | "detached" } | null>(null);

  /** 单个 profile 的界面打开方式（仅 Web 类型有意义）：未单独设置时跟随全局默认 */
  const doSetProfileWebOpenMode = useCallback(async (profile: string, v: "window" | "browser") => {
    const s = settingsRef.current;
    if (!s || (s.profileWebOpenMode?.[profile] ?? s.webOpenMode) === v) return;
    const next = { ...s, profileWebOpenMode: { ...s.profileWebOpenMode, [profile]: v } };
    setSettings(next);
    try {
      await api.saveSettings(next);
      addToast("ok", `「${profile}」界面已改为在${v === "window" ? "应用内独立窗口" : "系统默认浏览器"}打开`);
    } catch (e) {
      setSettings(s);
      addToast("err", `切换失败: ${e}`);
    }
  }, [addToast]);

  /** 日志级别：保存后后端运行期立即生效；DSH_STARTER_LOG 环境变量存在时以环境变量为准 */
  const doSetLogLevel = useCallback(async (v: string) => {
    const s = settingsRef.current;
    if (!s || s.logLevel === v) return;
    const next = { ...s, logLevel: v };
    setSettings(next);
    try {
      await api.saveSettings(next);
      const e = await api.getEnvironment();
      setEnv(e);
      addToast(
        "ok",
        e.logLevelPinned
          ? `级别「${v}」已保存，但被 DSH_STARTER_LOG 环境变量覆盖，实际仍是 ${e.logLevel}`
          : `日志级别已切换为 ${v}，立即生效（无需重启）`,
      );
    } catch (err) {
      setSettings(s);
      addToast("err", `切换日志级别失败: ${err}`);
    }
  }, [addToast]);

  /** 按「打开方式」打开实例 Web UI：优先该 profile 自己的覆盖，未设置走全局默认 */
  const openDshWeb = useCallback((url: string, title?: string, profile?: string | null) => {
    const s = settingsRef.current;
    const mode = (profile ? s?.profileWebOpenMode?.[profile] : undefined) ?? s?.webOpenMode;
    const p = mode === "browser"
      ? api.openUrl(url)
      : api.openWebWindow(url, title, resolved);
    p.catch((e) => addToast("err", String(e)));
  }, [addToast, resolved]);

  /** 首页 DeepSeek 入口：始终用应用内独立窗口打开官方对话，每次点击新开一个窗口；
   *  在途守卫只防误触连发，不限制用户有意地多次打开 */
  const deepSeekOpeningRef = useRef(false);
  const doOpenDeepSeek = useCallback(() => {
    if (deepSeekOpeningRef.current) return;
    deepSeekOpeningRef.current = true;
    api.openWebWindow("https://chat.deepseek.com/", "DeepSeek", resolved, true)
      .catch((e) => addToast("err", String(e)))
      .finally(() => { deepSeekOpeningRef.current = false; });
  }, [addToast, resolved]);

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
      const status = await api.checkStarterUpdate();
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
      const msg = await api.installStarterUpdate();
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

  /**
   * 启动一个 profile。`ackVersionChange=true` 表示用户已经在风险确认框里
   * 认了「dsh 版本和上次不一样」这件事，后端才会真正拉起实例。
   */
  const doStartProfile = useCallback(async (profile: string, ackVersionChange = false) => {
    // 连点/重复触发守卫：同一 profile 的启动请求还在途时直接忽略。
    // 后端 ensure_profile_free 依赖进程枚举（外部实例还有扫描/缓存延迟），
    // 挡不住同一瞬间挤进来的两次请求 —— 那样会起两个 dsh 抢同一个 web 端口。
    if (startingProfilesRef.current.has(profile)) return;
    startingProfilesRef.current.add(profile);
    setStartingProfile(profile);
    const s0 = settingsRef.current;
    const detached = ((profile ? s0?.profileLaunchMode?.[profile] : undefined) ?? s0?.launchMode ?? "detached") === "detached";
    try {
      const res = await api.startEmbedded(null, profile, undefined, detached, ackVersionChange);
      // 版本变化闸门：这次没启动，把风险摆给用户，确认后再带 ack 重来一遍
      if (res.versionChange) {
        setVersionWarn(res.versionChange);
        return;
      }
      const info = res.proc;
      if (!info) {
        addToast("err", `启动失败：profile「${profile}」没有返回实例信息，请重试`);
        return;
      }
      // 旧版 dsh（<0.1.7）只认全局 settings.yaml：后端在拉起前把它从 .imported 还原了回来
      if (res.legacyRestored) {
        addToast("info", `旧版 dsh：全局配置已从 settings.yaml.imported 还原到 ${res.legacyRestored}`);
      }
      if (detached) {
        // 独立进程没有日志管道：先刷新实例列表把它带进「实例终端」，再选中并展开，
        // 否则用户启动完看不到任何反馈（只能去 Profile 实例页找）
        addToast(
          "ok",
          `profile「${info.profile}」已以独立进程启动（PID ${info.id}），日志：~/.dsh-starter/instance-logs`
        );
        await refreshInstances();
        setActiveProc(info.id);
        setTerminalTask({ kind: "instance", id: String(info.id) });
        setTerminalOpen(true);
      } else {
        setProcs((m) => ({
          ...m,
          [info.id]: {
            id: info.id, version: info.version, profile: info.profile,
            startedAt: info.startedAt, lines: [], exited: false, code: null, webUrl: null,
          },
        }));
        setActiveProc(info.id);
        setTerminalTask({ kind: "instance", id: String(info.id) });
        setTerminalOpen(true);
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

  /** 重启确认框的「保存并重启」：先落盘新启动方式，再按新模式重启该实例 */
  const confirmLaunchModeSwitch = useCallback(async () => {
    const ask = launchModeAsk;
    setLaunchModeAsk(null);
    if (!ask) return;
    await applyProfileLaunchMode(ask.profile, ask.mode);
    void doRestartProfile(ask.profile);
  }, [launchModeAsk, applyProfileLaunchMode, doRestartProfile]);

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
        // RC 属于正式版；alpha/beta 及其它预发布后缀（next/canary/…→"pre"）视为预发布
        const isPre = r.channel === "alpha" || r.channel === "beta" || r.channel === "pre";
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
    type Row = { key: string; profile: string; phase: Phase; pid: number | null; source: string | null; version: string | null; boundVersion: string | null; webUrl: string | null; code: number | null; target: ProfileTarget; port: number | null; logFile: string | null; reserved: boolean };
    // 行主键：有 profile 名就用名字；没有 profile（终端 `dsh web` 没带 --profile）就用
    // PID/端口，否则多个无名实例会互相覆盖，界面上只剩一个
    const keyOf = (profile: string, pid: number | null, port: number | null) =>
      profile || (pid != null ? `pid:${pid}` : port != null ? `port:${port}` : "unknown");
    const map = new Map<string, Row>();
    for (const p of profiles) {
      map.set(p.name, { key: p.name, profile: p.name, phase: "stopped", pid: null, source: null, version: null, boundVersion: boundVersions[p.name] ?? null, webUrl: null, code: null, target: p.target, port: null, logFile: null, reserved: p.reserved });
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
        boundVersion: known?.boundVersion ?? boundVersions[i.profile] ?? null,
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
      map.set(key, { key, profile: p.profile, phase, pid: p.id, source: "embedded", version: p.version, boundVersion: map.get(key)?.boundVersion ?? boundVersions[p.profile] ?? null, webUrl: p.webUrl, code: p.code, target: map.get(key)?.target ?? "unknown", port: null, logFile: null, reserved: map.get(key)?.reserved ?? false });
    }
    const label = (r: Row) => r.profile || `:${r.port ?? "?"}`;
    return [...map.values()].sort((a, b) => label(a).localeCompare(label(b)));
  }, [profiles, instances, procs, boundVersions]);

  /** 恢复模式 profile 是否已存在（存在就不再显示创建入口） */
  const recoveryExists = useMemo(
    () => profiles.some((p) => p.name === RECOVERY_PROFILE),
    [profiles]
  );

  // 选中的 profile 被删除/改名后自动收起工作台右列（重扫是异步的，用 effect 收口）
  useEffect(() => {
    if (selectedProfile && !profiles.some((p) => p.name === selectedProfile)) {
      setSelectedProfile(null);
    }
  }, [profiles, selectedProfile]);

  // ── 命令面板（⌘K / Ctrl+K） ─────────────────
  /** 统一导航：侧栏与命令面板共用 */
  const navigate = useCallback((v: View) => setView(v), []);

  // 开合快捷键监听（模式同侧栏 Ctrl+B，见 ui/sidebar.tsx）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat) return; // 按住不放会按连发速率反复开合面板
      if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((v) => !v);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const paletteCommands = useMemo<PaletteCommand[]>(() => {
    const web = liveWebProcs[0];
    const cmds: PaletteCommand[] = [
      { id: "nav-quick", group: "导航", label: "首页", icon: Home, keywords: "快捷操作", run: () => navigate("quick") },
      { id: "nav-versions", group: "导航", label: "版本与安装", icon: Package, keywords: "dsh 版本", run: () => navigate("versions") },
      { id: "nav-profiles", group: "导航", label: "Profiles（实例与配置）", icon: Rocket, keywords: "实例 插件 模型 配置文件 工作台", run: () => navigate("profiles") },
      { id: "nav-credentials", group: "导航", label: "凭据管理", icon: KeyRound, keywords: "key token", run: () => navigate("credentials") },
      { id: "nav-stats", group: "导航", label: "统计", icon: BarChart3, keywords: "token 用量", run: () => navigate("stats") },
      { id: "nav-logs", group: "导航", label: "系统日志", icon: ScrollText, keywords: "log 排查 诊断 级别", run: () => navigate("logs") },
      {
        id: "open-web", group: "操作", label: "打开 DSH 主界面", icon: ExternalLink,
        hint: web?.webUrl ?? "无运行实例", disabled: !web?.webUrl, keywords: "ui web",
        run: () => web?.webUrl && openDshWeb(web.webUrl, undefined, web.profile),
      },
    ];
    for (const p of profiles) {
      const inst = instances.find((i) => i.profile === p.name && i.running);
      const busy = startingProfile === p.name || restartingProfile != null;
      cmds.push(
        { id: `config-${p.name}`, group: "操作", label: `打开 ${p.name} 工作台`, icon: SlidersHorizontal, keywords: "profile 插件 模型 配置", run: () => { setSelectedProfile(p.name); navigate("profiles"); } },
      );
      if (inst) {
        cmds.push(
          { id: `restart-${p.name}`, group: "操作", label: `重启 ${p.name}`, icon: RotateCw, hint: "运行中", disabled: busy, keywords: "profile", run: () => void doRestartProfile(p.name) },
          { id: `stop-${p.name}`, group: "操作", label: `停止 ${p.name}`, icon: Square, hint: "运行中", disabled: busy, keywords: "profile", run: () => void doStopInstance({ profile: p.name, pid: inst.pid }) },
        );
      } else {
        cmds.push(
          { id: `start-${p.name}`, group: "操作", label: `启动 ${p.name}`, icon: Play, disabled: busy, keywords: "profile 运行", run: () => void doStartProfile(p.name) },
        );
      }
    }
    cmds.push(
      { id: "refresh-instances", group: "操作", label: "刷新实例列表", icon: RefreshCw, keywords: "实例", run: () => void refreshInstances() },
      { id: "refresh-profiles", group: "操作", label: "重扫 Profile 目录", icon: RefreshCw, keywords: "profile 扫描", run: () => void refreshProfiles() },
      { id: "refresh-remote", group: "操作", label: "刷新 dsh 版本清单", icon: Package, keywords: "远端 registry", run: () => void refreshRemote() },
      { id: "check-update", group: "操作", label: "检查启动器更新", icon: Download, keywords: "升级 新版本", run: () => void doCheckUpdate() },
      {
        id: "toggle-terminal", group: "操作", label: terminalOpen ? "收起终端" : "打开终端", icon: Terminal,
        keywords: "日志 面板 任务 安装 实例", run: () => setTerminalOpen((v) => !v),
      },
      { id: "open-settings", group: "操作", label: "打开设置", icon: SettingsIcon, keywords: "偏好", run: () => setShowSettings(true) },
      { id: "theme-dark", group: "外观", label: "深色模式", icon: Moon, hint: theme === "dark" ? "当前" : undefined, run: () => setTheme("dark") },
      { id: "theme-light", group: "外观", label: "浅色模式", icon: Sun, hint: theme === "light" ? "当前" : undefined, run: () => setTheme("light") },
      { id: "theme-system", group: "外观", label: "跟随系统", icon: Monitor, hint: theme === "system" ? "当前" : undefined, run: () => setTheme("system") },
    );
    return cmds;
  }, [
    liveWebProcs, profiles, instances, startingProfile, restartingProfile, terminalOpen, theme,
    navigate, openDshWeb, doStartProfile, doRestartProfile, doStopInstance,
    refreshInstances, refreshProfiles, refreshRemote, doCheckUpdate, setTheme,
  ]);

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
        onNavigate={navigate}
        onOpenPalette={() => setPaletteOpen(true)}
        env={env}
        settings={settings}
        runningInstanceCount={runningInstanceCount}
        upgradableCount={upgradableCount}
        terminalOpen={terminalOpen}
        onToggleTerminal={() => setTerminalOpen((v) => !v)}
        onCheckUpdate={doCheckUpdate}
        onToast={addToast}
      />

      {/* 内容侧：顶栏 / 视图放在 SidebarInset 内，随侧栏收放一起让位。
          顶栏只留「折叠侧栏 + 拖拽区 + 高频动作」：环境状态、实例终端、检查更新
          与仓库/下载页入口都收在侧栏底部的卡片里（见 AppSidebar）。 */}
      <SidebarInset className="min-w-0 overflow-hidden bg-transparent">
        <header className="flex h-11 shrink-0 items-center gap-1.5 border-b border-border bg-background/80 px-2 lg:gap-2 lg:px-4">
          <SidebarTrigger title="折叠 / 展开侧栏（Ctrl+B）" />
          {/* 抽屉态（< md）侧栏不可见，顶栏补上品牌标识 */}
          <img src="/dsh-logo.svg" alt="DSH" className="h-6 w-6 shrink-0 md:hidden" draggable={false} />
          {/* 顶栏空白处也能拖窗口（data-tauri-drag-region 只管自己那一层，
              按钮之类的可点元素不受影响） */}
          <div className="flex-1" data-tauri-drag-region />
          {liveWebProcs.length > 0 && (
            <Button
              size="sm"
              onClick={() => liveWebProcs[0].webUrl && openDshWeb(liveWebProcs[0].webUrl, undefined, liveWebProcs[0].profile)}
              title={liveWebProcs.length === 1
                ? `打开 dsh 主界面：${liveWebProcs[0].webUrl}`
                : `${liveWebProcs.length} 个实例运行中，点击打开最新一个`}
            >
              <ExternalLink /> <span className="hidden lg:inline">打开 DSH 界面</span>
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon"
            title={terminalOpen ? "收起终端面板：实例日志与安装任务" : "打开终端面板：实例日志与安装任务"}
            className={terminalOpen ? "bg-muted" : undefined}
            onClick={() => setTerminalOpen((v) => !v)}
          >
            <Terminal className="h-4 w-4" />
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
            className="shrink-0 animate-in gap-1.5 rounded-none border-x-0 border-t-0 border-red-500/30 bg-red-500/10 px-4 py-2 pr-40 text-[13px] fade-in slide-in-from-top-2 duration-300"
          >
            <XCircle />
            {nodeTask ? (
              <>
                <AlertTitle className="font-normal">
                  正在安装内置 Node…
                  {nodeTask.total > 0 &&
                    ` ${Math.round((nodeTask.received / nodeTask.total) * 100)}% (${(nodeTask.received / 1048576).toFixed(1)}/${(nodeTask.total / 1048576).toFixed(1)} MB)`}
                </AlertTitle>
                {nodeTask.total > 0 && (
                  <AlertDescription>
                    <Progress
                      value={(nodeTask.received / nodeTask.total) * 100}
                      className="h-1.5 max-w-md bg-red-500/20"
                    />
                  </AlertDescription>
                )}
                <AlertDescription>实时输出在右侧终端面板</AlertDescription>
                <AlertAction>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => {
                      setTerminalTask({ kind: "nodeInstall", id: "node" });
                      setTerminalOpen(true);
                    }}
                  >
                    在终端中查看
                  </Button>
                </AlertAction>
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

        {/* 内容行：滚动内容区 + 通用终端右栏（宽屏并排占位；窄屏面板自渲染为 Sheet，不占列） */}
        <div className="flex min-h-0 flex-1">
        {/* 内容区（key 随视图变化：切换时重新挂载并播放入场动画）
            超窄窗口收紧内边距，把横向空间尽量留给表格与表单 */}
        <div
          key={view}
          className="min-h-0 min-w-0 flex-1 animate-in fade-in slide-in-from-bottom-2 overflow-y-auto p-3 duration-200 sm:p-4 lg:p-5"
        >
          {view === "quick" && (
            <QuickActionsView
              row={instanceRows.find((r) => r.profile === "web") ?? null}
              activeVersion={settings.activeVersion}
              hasNode={!!env.node}
              hasInstalled={installed.length > 0}
              starting={startingProfile === "web"}
              restarting={restartingProfile === "web"}
              initBusy={initBusy}
              onStart={(p) => void doStartProfile(p)}
              onStop={(row) => void doStopInstance(row)}
              onRestart={(p) => void doRestartProfile(p)}
              onOpenWeb={(u) => openDshWeb(u, undefined, "web")}
              onOpenDeepSeek={doOpenDeepSeek}
              onNavigate={navigate}
              onInitDsh={() => void doInitDsh()}
            />
          )}

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
                        {env.nodePath?.includes(".dsh-starter") ? "（隔离 · 仅本软件使用）" : "（系统级）"}
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
                  {nodeTask && (
                    <div className="mt-2.5 space-y-1.5 text-xs text-muted-foreground">
                      {nodeTask.total > 0 && (
                        <Progress value={(nodeTask.received / nodeTask.total) * 100} className="h-1.5" />
                      )}
                      <div className="flex items-center gap-1.5">
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        {nodeTask.total > 0
                          ? `${Math.round((nodeTask.received / nodeTask.total) * 100)}% (${(nodeTask.received / 1048576).toFixed(1)}/${(nodeTask.total / 1048576).toFixed(1)} MB)`
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

              {installTask && (
                <InstallCard
                  version={installTask.id}
                  onCancel={doCancelInstall}
                  onOpenTerminal={() => {
                    setTerminalTask({ kind: "dshInstall", id: installTask.id });
                    setTerminalOpen(true);
                  }}
                />
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
                <Button
                  variant="outline"
                  size="sm"
                  disabled={rows.length === 0}
                  title="逐个查看各版本 dsh 改了什么（官方 GitHub Release 正文）"
                  onClick={() => setNotesVersion(rows[0]?.version ?? null)}
                >
                  <ScrollText /> 更新日志
                </Button>
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

              {/* 默认落地页是版本页：装完版本但 dsh 还没初始化时，在这里就把下一步说清楚，
                  否则用户会停在「装好了却什么都点不了」的状态 */}
              {installed.length > 0 && profiles.length === 0 && (
                <Alert>
                  <TriangleAlert />
                  <AlertTitle>首次使用还差一步：初始化 dsh</AlertTitle>
                  <AlertDescription>
                    dsh 的数据目录（<span className="font-mono">{env?.dshNativeHome ?? "~/.dsh"}</span>
                    ）是第一次运行 dsh 时才生成的，在那之前 profile、快捷配置、模型与插件都不可用。
                    到「Profiles」页点一下「初始化 dsh（首次启动 web）」即可（等价于跑一次{" "}
                    <span className="font-mono">dsh web</span>）。
                  </AlertDescription>
                  <AlertAction>
                    <Button size="sm" variant="outline" onClick={() => setView("profiles")}>
                      去初始化
                    </Button>
                  </AlertAction>
                </Alert>
              )}

              {/* 版本表 */}
              {rows.length > 0 && (
                /* @container：版本行的响应式按「这张卡片实际有多宽」算（侧栏收放会改变它），
                   而不是按视口宽度 —— 窄的时候就收起次要操作与低优先级列 */
                <Card className="@container py-0">
                  <Table className="min-w-[20rem]">
                    <TableHeader>
                      <TableRow className="bg-muted/40 hover:bg-muted/40">
                        {/* 列的显隐阈值必须与 VersionRow 里对应单元格一致（容器查询按卡片宽度算） */}
                        <TableHead className="h-9 pl-4 text-[10.5px] uppercase tracking-wider">版本</TableHead>
                        <TableHead className="hidden h-9 text-[10.5px] uppercase tracking-wider @[34rem]:table-cell">
                          发布日期
                        </TableHead>
                        <TableHead className="hidden h-9 text-[10.5px] uppercase tracking-wider @[40rem]:table-cell">
                          大小
                        </TableHead>
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
                          busy={installTask !== null}
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
                          onShowNotes={(v) => setNotesVersion(v)}
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
            /* @container：双栏判定按「内容区实际多宽」算而不是视口 ——
               右侧终端面板打开时（尤其侧栏展开）内容会被挤掉 ~420px，视口口径会误判成还能双栏 */
            <div className="@container flex flex-col items-start gap-4 @[60rem]:flex-row">
              {/* 左列：实例列表（内容够宽时双栏并排；窄屏选中 profile 后隐藏，让位给全屏工作台） */}
              <div className={`min-w-0 grow space-y-3 @[60rem]:w-[480px] @[60rem]:shrink-0 @[60rem]:grow-0 ${selectedProfile ? "hidden @[60rem]:block" : ""}`}>
              <div className="flex items-center gap-2">
                <h2 className="text-base font-semibold">Profile 实例</h2>
                <span className="text-xs text-muted-foreground">
                  基于「当前版本」<b className="font-mono">{settings.activeVersion || "（未选择）"}</b>；
                  不同 profile 可并行，同一 profile 同时只能运行一个
                </span>
                <span className="flex-1" />
                <Button
                  size="sm"
                  variant={terminalOpen ? "secondary" : "outline"}
                  onClick={() => setTerminalOpen((v) => !v)}
                  title="终端：实例日志与插件/Node/dsh 安装任务统一在右侧面板显示"
                >
                  <Terminal /> 终端
                </Button>
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
              {profiles.length === 0 && (
                <Card className="gap-3 border-primary/25 bg-primary/[0.04] p-4 ring-primary/20">
                  <div className="flex items-start gap-3">
                    <span className="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-primary/10 text-primary">
                      <Wand2 className="h-4 w-4" />
                    </span>
                    <div className="min-w-0 flex-1 space-y-2">
                      <div className="flex flex-wrap items-center gap-2">
                        <h3 className="text-sm font-semibold">首次使用 dsh：先做一次初始化</h3>
                        <Badge variant="outline">还没有 profile</Badge>
                      </div>
                      <p className="text-[11.5px] leading-relaxed text-muted-foreground">
                        启动器里的实例、快捷配置、模型与插件都建立在{" "}
                        <span className="font-mono">$DSH_HOME/profiles</span> 之上，而这个目录是{" "}
                        <strong className="font-medium text-foreground">dsh 第一次运行时</strong>才生成的
                        —— 也就是说需要先跑一次 <span className="font-mono">dsh web</span>。
                        它会在 <span className="font-mono">{env?.dshNativeHome ?? "~/.dsh"}</span> 下写出
                        内置 <span className="font-mono">web</span> profile、配置与凭据文件，之后这里的
                        所有功能才会就绪。
                      </p>
                      <div className="flex flex-wrap items-center gap-1.5 text-[11px]">
                        <span className="text-muted-foreground">环境</span>
                        <Badge variant={env?.node ? "info" : "destructive"}>
                          Node {env?.node ? `v${env.node}` : "未就绪"}
                        </Badge>
                        <Badge variant={installed.length > 0 ? "info" : "destructive"}>
                          dsh {installed.length > 0 ? (settings.activeVersion || installed[0].version) : "未安装"}
                        </Badge>
                        <Badge variant={env?.npm ? "secondary" : "outline"}>
                          npm {env?.npm ?? "未探测到"}
                        </Badge>
                      </div>
                      <div className="flex flex-wrap items-center gap-2 pt-0.5">
                        {installed.length === 0 ? (
                          <Button size="sm" onClick={() => setView("versions")}>
                            <Rocket /> 先去安装 dsh 版本
                          </Button>
                        ) : !env?.node ? (
                          <Button size="sm" onClick={() => setShowSettings(true)}>
                            <Rocket /> 先准备 Node 运行时
                          </Button>
                        ) : (
                          <Button size="sm" disabled={initBusy} onClick={() => void doInitDsh()}>
                            {initBusy ? <Loader2 className="animate-spin" /> : <Rocket />}
                            {initBusy ? "初始化中…" : "初始化 dsh（首次启动 web）"}
                          </Button>
                        )}
                        <Button
                          size="sm"
                          variant="outline"
                          onClick={() => {
                            void refreshProfiles();
                            void refreshInstances();
                          }}
                        >
                          <RefreshCw /> 我在终端里跑过了，重新检测
                        </Button>
                      </div>
                    </div>
                  </div>
                </Card>
              )}
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
                  const selected = !!row.profile && selectedProfile === row.profile;
                  return (
                    <Card
                      key={row.key}
                      className={`gap-0 py-0 transition-colors ${selected ? "border-primary/60 ring-1 ring-primary/20" : ""}`}
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
                          <span className={`h-2.5 w-2.5 shrink-0 rounded-full ${row.phase === "external" ? "bg-teal-500" : "bg-muted-foreground/30"}`} />
                        )}
                        <div className="min-w-0 grow basis-[calc(100%-1.75rem)]">
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
                            {/* 没在跑的行：把它上次真正跑起来的版本亮出来，
                                也正是这条记录决定换版本启动时要不要先确认风险 */}
                            {!row.version && row.boundVersion ? ` · 上次 dsh ${row.boundVersion}` : ""}
                          </div>
                        </div>
                        {/* 无名实例（终端 `dsh web` 没带 --profile）没有对应 profile，不提供配置工作台 */}
                        {row.profile && (
                          <Button
                            size="sm"
                            variant={selected ? "secondary" : "outline"}
                            className="shrink-0"
                            title="打开该 profile 的配置工作台（快捷配置 / 模型 / 插件 / 配置文件）"
                            onClick={() => setSelectedProfile(row.profile)}
                          >
                            <SlidersHorizontal /> 配置
                          </Button>
                        )}
                        {canOpen && (
                          <Button
                            size="sm"
                            onClick={() => row.webUrl && openDshWeb(row.webUrl, `DSH · ${row.profile}`, row.profile)}
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
                              : row.boundVersion && row.boundVersion !== settings.activeVersion
                              ? `上次用 dsh ${row.boundVersion} 跑起来，本次将用 ${settings.activeVersion}；版本变化可能导致该 profile 起不来，会先让你确认风险`
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
                              title="删除 profile（移入 ~/.dsh-starter/deleted-profiles，可找回）"
                              onClick={() => setDeleteTarget(row.profile)}
                            >
                              <Trash2 />
                            </Button>
                          </>
                        )}
                      </div>
                      {/* 每个 profile 独立控制启动方式与打开方式（打开方式仅 Web 界面有意义） */}
                      {row.profile && (
                        <div className="flex flex-wrap items-center gap-x-2 gap-y-1.5 border-t px-3 py-2">
                          <Tabs
                            value={settings.profileLaunchMode?.[row.profile] ?? settings.launchMode}
                            onValueChange={(v) => {
                              const mode = v as "child" | "detached";
                              const running = row.phase === "starting" || row.phase === "ready" || row.phase === "external";
                              if (running) setLaunchModeAsk({ profile: row.profile, mode });
                              else void applyProfileLaunchMode(row.profile, mode);
                            }}
                            title={(settings.profileLaunchMode?.[row.profile] ?? settings.launchMode) === "detached"
                              ? "独立进程：关闭启动器后 DSH 继续运行，日志写入 ~/.dsh-starter/instance-logs（下次启动/重启生效）"
                              : "子进程：日志实时进「实例终端」，退出启动器即结束该 DSH（下次启动/重启生效）"}
                          >
                            <TabsList className="h-6">
                              <TabsTrigger value="child" className="h-5 px-2 text-[11px]">子进程</TabsTrigger>
                              <TabsTrigger value="detached" className="h-5 px-2 text-[11px]">独立进程</TabsTrigger>
                            </TabsList>
                          </Tabs>
                          {row.target === "web" && (
                            <Tabs
                              className="ml-2"
                              value={settings.profileWebOpenMode?.[row.profile] ?? settings.webOpenMode}
                              onValueChange={(v) => void doSetProfileWebOpenMode(row.profile, v as "window" | "browser")}
                              title={(settings.profileWebOpenMode?.[row.profile] ?? settings.webOpenMode) === "browser"
                                ? "点「打开」跳系统默认浏览器"
                                : "在应用内独立窗口打开 DSH 界面（无地址栏），同一地址复用一个窗口"}
                            >
                              <TabsList className="h-6">
                                <TabsTrigger value="window" className="h-5 px-2 text-[11px]">独立窗口</TabsTrigger>
                                <TabsTrigger value="browser" className="h-5 px-2 text-[11px]">浏览器</TabsTrigger>
                              </TabsList>
                            </Tabs>
                          )}
                        </div>
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
              <div className="rounded-lg border border-amber-500/25 bg-amber-500/5 p-2.5 text-[11px] leading-relaxed text-muted-foreground">
                <span className="text-foreground">Target 标签</span>
                {" "}按各 profile 的 <span className="font-mono">package.json</span> 中
                <span className="font-mono"> name </span>与
                <span className="font-mono"> dsh.profile.bundles </span>
                识别运行形态：
                <Badge variant="info">Web</Badge>
                含 <span className="font-mono">@deepseek-ai/dsh-web-app</span>
                插件，启动后从日志识别地址，按卡片上的「打开方式」在独立窗口或浏览器打开界面；
                <Badge variant="secondary">Desktop</Badge>
                为桌面应用外壳；<Badge variant="outline">未识别</Badge>
                暂无可用的启动方式。启动方式与打开方式都在各 profile 卡片上独立设置，未设置过的跟随默认值；
                若插件导致启动异常，点该 profile 的「配置」进工作台，在「插件」Tab 停用可疑插件后重启实例。
              </div>
              </div>

              {/* 右列：选中 profile 的配置工作台（快捷配置 / 模型 / 插件 / 配置文件 / package.json）。
                  未选中时宽屏显示引导占位、窄屏整列隐藏（左列即列表页） */}
              <div className={`min-w-0 grow @[60rem]:basis-0 ${selectedProfile ? "" : "hidden @[60rem]:block"}`}>
                <ProfileWorkspace
                  profile={selectedProfile}
                  profiles={profiles.map((p) => p.name)}
                  target={profiles.find((p) => p.name === selectedProfile)?.target ?? "unknown"}
                  onToast={addToast}
                  onBack={selectedProfile ? () => setSelectedProfile(null) : undefined}
                  pluginRunningCount={pluginRunningCount}
                  pluginJobsTick={pluginJobsTick}
                  onOpenPluginTerminal={openPluginTerminal}
                />
              </div>
            </div>
          )}

          {view === "stats" && <StatsView onToast={addToast} appVersion={env?.appVersion ?? ""} />}
          {view === "logs" && (
            <LogsView
              onToast={addToast}
              settings={settings}
              onSaveLogLevel={doSetLogLevel}
              onReveal={revealPath}
            />
          )}
          {/* 凭据页 keep-alive：首次访问后保持挂载，切页只隐藏 —— 未保存草稿、
              搜索词与弹窗状态都保留；放最后避免隐藏节点影响 space-y 间距 */}
          {(credSeen || view === "credentials") && (
            <div className={view === "credentials" ? "" : "hidden"}>
              <CredentialsView onToast={addToast} active={view === "credentials"} />
            </div>
          )}
        </div>

          <TerminalPanel
            open={terminalOpen}
            inline={terminalInline}
            onOpenChange={setTerminalOpen}
            task={terminalTask}
            onSelectTask={selectTerminalTask}
            procs={panelProcs}
            pluginJobs={pluginJobs}
            pluginProfile={selectedProfile ?? ""}
            onCancelPluginJob={cancelPluginJob}
            onRetryPluginJob={doRetryPluginJob}
            onApprovePluginBuilds={doApprovePluginBuilds}
            sysTasks={sysTasks}
            onStop={doStopProc}
            onReadLog={readInstanceLog}
            onReveal={revealPath}
            onToast={addToast}
            onCancelInstall={doCancelInstall}
            onOpenWeb={(u) => openDshWeb(u, undefined, activeProc != null ? procsRef.current[activeProc]?.profile : undefined)}
            onExport={() => {
              const p = activeProc != null ? procsRef.current[activeProc] : null;
              if (!p) return;
              api.exportProcLog(p.profile || "default", p.id, p.lines.join("\n"))
                .then((path) => addToast("ok", `日志已导出：${path}`))
                .catch((e) => addToast("err", `导出失败: ${e}`));
            }}
            onClearFinished={clearFinishedTerminalTasks}
          />
        </div>

        {/* dsh 更新日志：版本表里的「更新日志」按钮与工具栏按钮都从这里打开 */}
        <DshChangelogDialog
          open={notesVersion !== null}
          initialVersion={notesVersion}
          versions={rows.map((r) => r.version)}
          onClose={() => setNotesVersion(null)}
          onOpenUrl={(u) => api.openUrl(u).catch((e) => addToast("err", String(e)))}
        />

        {/* 全局命令面板（⌘K / Ctrl+K，侧栏也有入口按钮） */}
        <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} commands={paletteCommands} />
      </SidebarInset>

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
        initialName={copyAndStart && copySource ? `${copySource}-try` : ""}
        existing={profiles.map((p) => p.name)}
        onClose={() => { setCopySource(null); setCopyAndStart(false); }}
        onToast={addToast}
        onCopied={(name) => {
          void refreshProfiles();
          if (copyAndStart) {
            setCopyAndStart(false);
            void doStartProfile(name);
          }
        }}
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

      {/* 运行中切换启动方式：确认后才保存并重启，取消则配置不变 */}
      <AlertDialog open={launchModeAsk != null} onOpenChange={(o) => !o && setLaunchModeAsk(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>切换「{launchModeAsk?.profile}」的启动方式需要重启实例</AlertDialogTitle>
            <AlertDialogDescription>
              该 profile 正在运行，启动方式在进程拉起时决定，改完要重启才生效。
              确认后将保存为{launchModeAsk?.mode === "detached" ? "「独立进程」（后台常驻，日志写入 ~/.dsh-starter/instance-logs）" : "「子进程」（随启动器退出结束）"}并立即重启；
              取消则保持原样、不做任何更改。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction onClick={() => void confirmLaunchModeSwitch()}>保存并重启</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 删除 profile 确认（内置保留 profile 不会走到这里） */}
      <AlertDialog open={deleteTarget != null} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除 profile「{deleteTarget}」？</AlertDialogTitle>
            <AlertDialogDescription>
              配置目录会被移动到 <span className="font-mono">~/.dsh-starter/deleted-profiles/</span>
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

      {/* 版本变化提示：说清风险即可，给出三条出路——取消 / 复制 profile 无损试用 / 认风险强制启动 */}
      <AlertDialog
        open={versionWarn != null}
        onOpenChange={(o) => { if (!o) setVersionWarn(null); }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogMedia className="bg-warning/15">
              <TriangleAlert className="text-warning" />
            </AlertDialogMedia>
            <AlertDialogTitle>dsh 启动版本与上次不一致</AlertDialogTitle>
            <AlertDialogDescription>
              profile「{versionWarn?.profile}」要换用不同的 dsh 版本启动，
              可能因插件或配置不兼容导致启动失败。推荐先复制一份试用，
              跑不起来原实例也不受影响。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="grid gap-px overflow-hidden rounded-lg border border-border/60 bg-border/60">
            <div className="grid grid-cols-[6.5rem_1fr] items-center gap-2 bg-popover px-3 py-1.5 text-sm">
              <span className="text-xs text-muted-foreground">上次成功启动</span>
              <span className="truncate font-mono text-[13px]">dsh {versionWarn?.fromVersion ?? "-"}</span>
            </div>
            <div className="grid grid-cols-[6.5rem_1fr] items-center gap-2 bg-popover px-3 py-1.5 text-sm">
              <span className="text-xs text-muted-foreground">本次将启动</span>
              <span className="flex min-w-0 items-center gap-2">
                <span className="truncate font-mono text-[13px]">dsh {versionWarn?.toVersion ?? "-"}</span>
                <Badge variant={versionWarn?.direction === "downgrade" ? "destructive" : "warning"}>
                  {versionWarn?.direction === "downgrade" ? "降级" : "升级"}
                </Badge>
              </span>
            </div>
          </div>
          {/* 三档纵向排布：推荐动作在最上，取消收底（flex-col-reverse 下 DOM 序与视觉相反） */}
          <AlertDialogFooter className="sm:flex-col-reverse">
            <AlertDialogCancel className="w-full">取消</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              className="w-full"
              onClick={() => {
                const w = versionWarn;
                setVersionWarn(null);
                if (w) void doStartProfile(w.profile, true);
              }}
            >
              <TriangleAlert /> 同意风险，强制启动
            </AlertDialogAction>
            <AlertDialogAction
              className="w-full"
              onClick={() => {
                const w = versionWarn;
                setVersionWarn(null);
                if (w) {
                  setCopyAndStart(true);
                  setCopySource(w.profile);
                }
              }}
            >
              <CopyPlus /> 复制 profile 试用启动
              <Badge variant="secondary" className="ml-1">推荐</Badge>
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
  if (pre) return "pre";
  return "stable";
}


