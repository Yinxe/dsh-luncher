import { useCallback, useEffect, useRef, useState } from "react";
import {
  Braces, Cloud, FileArchive, GitBranch, Layers, Link2, PackageX, Plus, RefreshCw, RotateCcw,
  Save, ShieldCheck,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import ClonedReposCard from "@/components/ClonedReposCard";
import InstallPluginDialog from "@/components/InstallPluginDialog";
import PluginTerminal from "@/components/PluginTerminal";
import YamlEditor from "@/components/YamlEditor";
import { usePluginJobs } from "../hooks/use-plugin-jobs";
import { api } from "../api";
import type { ClonedPlugin, PluginUpdateInfo, ProfileDetail } from "../types";

interface Props {
  profiles: string[];
  /** 从 Profile 实例页跳转过来时预选中的 profile（仅挂载时生效） */
  initialProfile?: string | null;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

/** 来源徽标：决定「怎么更新」的视觉线索 */
const SOURCE_META: Record<string, { label: string; icon: typeof Cloud; tip: string }> = {
  npm: { label: "npm", icon: Cloud, tip: "registry 包：按版本号检测更新（dsh plugin add 包名@latest）" },
  git: { label: "git 仓库", icon: GitBranch, tip: "GitHub 规格安装：按提交哈希检测更新" },
  "git-clone": { label: "clone+link", icon: GitBranch, tip: "本地克隆 + link：更新方式 git pull" },
  tarball: { label: "tgz 直链", icon: FileArchive, tip: "打包产物直链：无版本渠道，只能手动重装同一链接" },
  link: { label: "本地 link", icon: Link2, tip: "本地软链：改代码即时生效，无版本渠道" },
  file: { label: "本地 file", icon: Link2, tip: "本地文件依赖：无版本渠道" },
  workspace: { label: "workspace", icon: Braces, tip: "workspace 依赖：跟随工作区" },
};

function SourceBadge({ source }: { source: string }) {
  const meta = SOURCE_META[source] ?? { label: source, icon: Cloud, tip: source };
  const Icon = meta.icon;
  return (
    <Badge variant="outline" className="gap-1 text-[10px]" title={meta.tip}>
      <Icon /> {meta.label}
    </Badge>
  );
}

/** git-clone 依赖的克隆目录名（仅启动器托管的克隆可一键删除） */
function managedCloneName(u: PluginUpdateInfo | undefined): string | null {
  if (!u?.cloneDir || !u.managedClone) return null;
  const parts = u.cloneDir.replace(/[\\/]+$/, "").split(/[\\/]/);
  return parts[parts.length - 1] || null;
}

export default function PluginsView({ profiles, initialProfile, onToast }: Props) {
  // 优先用跳转种子；否则默认选中 web（与启动器整体默认一致），不存在才取第一个
  const [profile, setProfile] = useState<string>(() =>
    initialProfile ?? (profiles.includes("web") ? "web" : (profiles[0] ?? ""))
  );
  const [detail, setDetail] = useState<ProfileDetail | null>(null);
  const [reloadMode, setReloadMode] = useState<string>("live");
  const [editFile] = useState<string>("cordis.patch.yml");
  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [bundleBusy, setBundleBusy] = useState(false);
  const [installOpen, setInstallOpen] = useState(false);
  const [updates, setUpdates] = useState<Record<string, PluginUpdateInfo> | null>(null);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [pendingUninstall, setPendingUninstall] = useState<
    { name: string; isBundle: boolean; cloneName: string | null } | null
  >(null);
  const [purgeClone, setPurgeClone] = useState(false);

  useEffect(() => {
    // profiles 晚于挂载加载时，同样默认选中 web
    if (profiles.length > 0 && !profiles.includes(profile)) {
      setProfile(profiles.includes("web") ? "web" : profiles[0]);
    }
  }, [profiles, profile]);

  // 始终指向「当前选中」的 profile，用于丢弃过期的 reload 响应
  const latestProfileRef = useRef(profile);
  latestProfileRef.current = profile;

  const reload = useCallback(async () => {
    if (!profile) return;
    try {
      const [d, mode] = await Promise.all([
        api.getProfileDetail(profile),
        api.getPatchReload(profile),
      ]);
      // 快速切换 profile 时，旧请求可能后返回：不能覆盖新 profile 的 detail/draft，否则 saveFile 会写错文件
      if (latestProfileRef.current !== profile) return;
      setDetail(d);
      setReloadMode(mode);
      setDraft(d.patchRaw);
      setDirty(false);
      setUpdates(null);
    } catch (e) {
      if (latestProfileRef.current !== profile) return;
      onToast("err", `读取 profile 配置失败: ${e}`);
    }
  }, [profile, onToast]);

  useEffect(() => {
    reload();
  }, [reload]);

  // 插件任务流（内置终端数据源）；任务结束自动刷新插件列表
  const { jobs, activeId, setActiveId, runningCount, cancel, clear } = usePluginJobs({
    onFinished: (e) => {
      const tag = e.cancelled ? "已取消" : e.ok ? "完成" : "失败";
      // 失败时优先显示对症建议（供应链策略 / 构建脚本 / 鉴权 / 404…），而不是笼统的「看终端」
      const detail = e.ok
        ? ""
        : e.hint
        ? `：${e.hint.split("\n")[0]}`
        : "：展开内置终端查看输出";
      onToast(e.cancelled ? "info" : e.ok ? "ok" : "err", `${e.label} ${tag}${detail}`);
      reload();
    },
  });

  /** 有插件任务在跑、正在保存配置、或正在切换 bundle 时，禁用会互踩的操作 */
  const busy = runningCount > 0 || saving || bundleBusy;

  const toggleBundle = useCallback(
    async (name: string, enabled: boolean) => {
      if (bundleBusy) return;
      setBundleBusy(true);
      try {
        await api.setBundleEnabled(profile, name, enabled);
        onToast(
          "ok",
          `插件包 ${name} 已${enabled ? "启用" : "停用"}${
            reloadMode === "live" ? "（live 模式已即时生效）" : "（startup 模式需重启实例）"
          }`
        );
        await reload();
      } catch (e) {
        onToast("err", String(e));
      } finally {
        setBundleBusy(false);
      }
    },
    [profile, reloadMode, reload, onToast, bundleBusy]
  );

  const uninstall = useCallback(
    (name: string, cloneDir: string | null) => {
      api
        .pluginUninstall(profile, name, cloneDir)
        .then(() => onToast("info", `已开始卸载 ${name}（输出见内置终端）`))
        .catch((e) => onToast("err", String(e)));
    },
    [profile, onToast]
  );

  const saveFile = useCallback(async () => {
    setSaving(true);
    try {
      await api.writeProfileFile(profile, editFile, draft);
      onToast("ok", `${editFile} 已保存（原文件已备份）`);
      await reload();
    } catch (e) {
      onToast("err", String(e));
    } finally {
      setSaving(false);
    }
  }, [profile, editFile, draft, onToast, reload]);

  const doCheckUpdates = useCallback(async () => {
    const target = profile; // 检查在途时可切 profile，旧结果不能写进新 profile 的视图
    setCheckingUpdates(true);
    try {
      const list = await api.checkPluginUpdates(target);
      if (latestProfileRef.current !== target) return;
      const map: Record<string, PluginUpdateInfo> = {};
      for (const u of list) map[u.name] = u;
      setUpdates(map);
      const n = list.filter((u) => u.hasUpdate).length;
      const skipped = list.filter((u) => !u.checked).length;
      onToast(
        n > 0 ? "info" : "ok",
        n > 0
          ? `${n} 个包有新版本可用${skipped > 0 ? `（${skipped} 个无更新渠道/无法检测已跳过）` : ""}`
          : skipped > 0
          ? `全部已最新（${skipped} 个无更新渠道/无法检测已跳过）`
          : "全部已最新"
      );
    } catch (e) {
      if (latestProfileRef.current !== target) return;
      onToast("err", `检查更新失败: ${e}`);
    } finally {
      setCheckingUpdates(false);
    }
  }, [profile, onToast]);

  /** 批量安装（一个任务里顺序执行多条 dsh plugin add） */
  const installMany = useCallback(
    (specs: string[], mode: "install" | "upgrade") => {
      api
        .pluginInstall(profile, specs, mode)
        .then(() =>
          onToast(
            "info",
            `${mode === "upgrade" ? "升级" : "安装"} ${specs.length} 个包：输出见内置终端`
          )
        )
        .catch((e) => onToast("err", String(e)));
    },
    [profile, onToast]
  );

  /** clone 仓库 + 本地 link 安装（accel=false 时本次不走 GitHub 加速） */
  const cloneInstall = useCallback(
    (
      input: { url: string; gitRef: string | null; subPath: string | null; build: boolean },
      accel: boolean
    ) => {
      api
        .pluginCloneInstall(profile, input, accel)
        .then(() => onToast("info", "已开始 clone + link 安装：进度见内置终端"))
        .catch((e) => onToast("err", String(e)));
    },
    [profile, onToast]
  );

  /** git pull 更新本地克隆（再重新 link） */
  const pullClone = useCallback(
    (repo: ClonedPlugin, subPath: string | null, build: boolean) => {
      const name = repo.candidates.find((c) => c.path === subPath)?.name ?? repo.dirName;
      api
        .pluginPullUpdate(profile, name, repo.path, subPath, build)
        .then(() => onToast("info", `已开始 git pull 更新 ${name}：输出见内置终端`))
        .catch((e) => onToast("err", String(e)));
    },
    [profile, onToast]
  );

  /** 升级按钮：npm/git 源走 add，clone 源走 git pull */
  const runUpgrade = useCallback(
    (u: PluginUpdateInfo) => {
      setUpdates(null);
      if (u.updateKind === "git-pull" && u.cloneDir) {
        api
          .pluginPullUpdate(profile, u.name, u.cloneDir, u.subPath, u.libOk === false)
          .then(() => onToast("info", `已开始 git pull 升级 ${u.name}：输出见内置终端`))
          .catch((e) => onToast("err", String(e)));
        return;
      }
      if (u.updateSpec) installMany([u.updateSpec], "upgrade");
    },
    [profile, installMany, onToast]
  );

  const updateBadge = (name: string) => {
    const u = updates?.[name];
    if (!u) return null;
    if (!u.checked) {
      if (!u.note) return null;
      // 私有仓库（远端不可匿名访问）单独标出来：这是被明确拒绝的场景，可手动 clone/pull
      const privateRepo = u.blocked === "private-repo";
      return (
        <Badge
          variant={privateRepo ? "warning" : "outline"}
          className="max-w-[220px] truncate text-[10px]"
          title={u.note}
        >
          {privateRepo ? "私有仓库 · 不支持" : "无更新渠道"}
        </Badge>
      );
    }
    if (!u.hasUpdate) return null;
    const tip =
      u.source === "git-clone"
        ? `本地 ${u.installedCommit?.slice(0, 7) ?? "?"} → 远端 ${u.remoteCommit?.slice(0, 7) ?? "?"}${
            u.dirty ? "（本地有未提交改动，pull 前请留意）" : ""
          }`
        : u.source === "git"
        ? `远端提交 ${u.remoteCommit?.slice(0, 7) ?? "?"} ≠ 已装 ${u.installedCommit?.slice(0, 7) ?? "?"}`
        : `已装 ${u.installedVersion ?? "?"} → 最新 ${u.latestVersion ?? "?"}`;
    return (
      <div className="flex shrink-0 items-center gap-1.5">
        <Badge variant="warning" title={tip}>
          {u.source === "git" ? "有新提交" : u.source === "git-clone" ? "有新提交" : `可升级 ${u.latestVersion ?? ""}`}
        </Badge>
        <Button
          size="sm"
          variant="outline"
          className="h-6 px-2 text-[11px]"
          disabled={busy || checkingUpdates}
          title={
            u.updateKind === "git-pull"
              ? `git -C ${u.cloneDir} pull --ff-only && dsh plugin add ${u.updateSpec}`
              : `dsh plugin add ${u.updateSpec}`
          }
          onClick={() => runUpgrade(u)}
        >
          升级
        </Button>
      </div>
    );
  };

  if (profiles.length === 0) {
    return (
      <Card className="p-6 text-[12px] leading-relaxed text-muted-foreground">
        <div className="text-sm font-medium text-foreground">还没有可用的 profile</div>
        <p className="mt-1.5">
          插件是按 profile 隔离安装的，所以要先有 profile。dsh 的数据目录是
          <strong className="font-medium text-foreground"> 第一次运行 dsh 时 </strong>
          才生成的：到「Profile 实例」页点「初始化 dsh（首次启动 web）」，或在终端里自行跑一次{" "}
          <span className="font-mono">dsh web</span>，之后回到这里即可选择 profile。
        </p>
      </Card>
    );
  }

  return (
    <div className="space-y-4">
      {/* ── 工作区头：当前 profile 是这一页的**主语**，不是顺带的一个下拉 ──
          原来它只是标题旁边一个 w-52 的下拉框，很容易在操作别的东西时忘了自己是哪个
          profile，从而把插件装错地方。这里改成一张带主色的"工作区卡"：
          左边说清范围，右边是加大的选择器 + 该 profile 的关键状态。 */}
      <Card className="gap-3 border-primary/25 bg-primary/[0.04] p-4 ring-primary/20">
        <div className="flex flex-wrap items-start gap-3">
          <span className="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-primary/10 text-primary">
            <Layers className="h-4 w-4" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-base font-semibold">插件管理</h2>
              <Badge variant="outline" className="text-[10px]">按 profile 隔离</Badge>
              {detail && (
                <span className="text-[10.5px] text-muted-foreground">
                  {detail.bundles.length} 个插件 · {detail.packages.length} 个依赖
                </span>
              )}
            </div>
            <p className="mt-1 text-[11.5px] leading-relaxed text-muted-foreground">
              本页的安装 / 卸载 / 升级 / 克隆 / 终端输出<strong className="font-medium text-foreground">只作用于右侧选中的 profile</strong>；
              其他 profile 的依赖、<span className="font-mono">dsh.profile.bundles</span> 与
              <span className="font-mono"> cordis.patch.yml</span> 不会被改动。
            </p>
          </div>
          <div className="flex shrink-0 items-end gap-2">
            <div className="flex flex-col gap-1">
              <span className="text-[10.5px] font-medium text-muted-foreground">当前 profile</span>
              <Select value={profile} onValueChange={setProfile}>
                <SelectTrigger className="h-9 w-60 font-mono text-[13px] font-semibold">
                  <SelectValue placeholder="选择 profile" />
                </SelectTrigger>
                <SelectContent>
                  {profiles.map((p) => (
                    <SelectItem key={p} value={p} className="font-mono">
                      {p}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <Badge
              variant={reloadMode === "live" ? "success" : "warning"}
              className="mb-1.5"
              title={
                reloadMode === "live"
                  ? "该 profile 的 patchReload = live：保存 cordis.patch.yml 后即时生效"
                  : "该 profile 的 patchReload = startup：改完需要重启实例才生效"
              }
            >
              {reloadMode === "live" ? "live 即时生效" : "startup 需重启"}
            </Badge>
          </div>
        </div>
      </Card>

      {/* ── 隔离边界：以下所有区块都属于上面选中的 profile ──
          一条可见的左侧主色边框 + 一段极淡底色，把"作用范围"画出来，
          避免在长页面里滚动到一半就忘了自己正在操作哪个 profile。 */}
      <div className="space-y-4 rounded-xl border border-border/70 border-l-2 border-l-primary/60 bg-muted/20 p-3">
        <div className="flex flex-wrap items-center gap-2 px-0.5 text-[10.5px] text-muted-foreground">
          <ShieldCheck className="h-3.5 w-3.5 shrink-0 text-primary/80" />
          <span>隔离边界</span>
          <span className="font-mono text-[11px] font-semibold text-foreground">{profile}</span>
          <span className="hidden sm:inline">· 以下区块只读写这个 profile</span>
        </div>

      {/* 内置终端：安装/卸载/升级的实时输出 */}
      <PluginTerminal
        jobs={jobs}
        activeId={activeId}
        onSelect={setActiveId}
        onCancel={cancel}
        onClear={clear}
        profile={profile}
        onToast={onToast}
        onRetry={(jobId) =>
          api
            .retryPluginJob(jobId)
            .then((id) => {
              setActiveId(id);
              onToast("info", "已重试：输出见内置终端");
            })
            .catch((e) => onToast("err", String(e)))
        }
        onApproveBuilds={(jobId) =>
          api
            .approvePluginBuilds(jobId)
            .then((id) => {
              setActiveId(id);
              onToast("info", "已写入 allowBuilds，正在重跑（输出见内置终端）");
            })
            .catch((e) => onToast("err", String(e)))
        }
      />

      <Card className="p-4">
        <div className="mb-2.5 flex flex-wrap items-center gap-2.5">
          <div className="text-[13px] font-semibold">
            插件包
            <span className="ml-2 text-xs font-normal text-muted-foreground">
              dsh.profile.bundles · 启停按包内真实插件 ID 写入 cordis.patch 层
            </span>
          </div>
          <span className="flex-1" />
          {updates && (
            <Badge variant={Object.values(updates).some((u) => u.hasUpdate) ? "warning" : "outline"}>
              {Object.values(updates).filter((u) => u.hasUpdate).length} 个可更新
            </Badge>
          )}
          <Button
            size="sm"
            variant="outline"
            disabled={checkingUpdates || busy}
            onClick={doCheckUpdates}
            title="npm 包比对 registry 最新版本；GitHub 规格比对最新提交；本地克隆比对 git HEAD；纯 link/tgz 无渠道"
          >
            <RefreshCw className={checkingUpdates ? "animate-spin" : ""} /> 检查更新
          </Button>
          <Button
            size="sm"
            disabled={busy}
            onClick={() => setInstallOpen(true)}
            title="npm 包 / 链接直装 / Clone 仓库 三种安装方式"
          >
            <Plus /> 安装插件
          </Button>
        </div>
        <div className="space-y-1.5">
          {(detail?.bundles ?? []).map((b) => (
            <div
              key={b.name}
              className={`flex items-center gap-3 rounded-lg border border-border bg-background/50 px-3 py-1.5 ${
                b.enabled ? "" : "opacity-60"
              }`}
            >
              <span className={`h-2 w-2 shrink-0 rounded-full ${b.enabled ? "bg-emerald-500" : "bg-muted-foreground/40"}`} />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-1.5">
                  <span className="truncate font-mono text-[12.5px] font-medium">{b.name}</span>
                  <SourceBadge source={b.source} />
                  {b.official && (
                    <Badge variant="secondary" className="text-[10px]" title="dsh 宿主自带的插件包：不可卸载、不可停用">
                      官方
                    </Badge>
                  )}
                </div>
                <div className="truncate text-[10.5px] text-muted-foreground" title={b.version ?? ""}>
                  {b.version ?? "—"}
                  {b.pluginIds.length > 0 && ` · 插件 ID: ${b.pluginIds.join(", ")}`}
                </div>
              </div>
              {updateBadge(b.name)}
              <Button
                size="sm"
                variant="ghost"
                className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                disabled={busy || b.official}
                onClick={() => {
                  setPurgeClone(false);
                  setPendingUninstall({
                    name: b.name,
                    isBundle: true,
                    cloneName: managedCloneName(updates?.[b.name]),
                  });
                }}
                title={
                  b.official
                    ? `${b.name} 是 dsh 宿主自带的插件包，卸载会让这个 profile 起不来；要折腾请用「恢复模式」新建一个实例`
                    : "通过官方 dsh plugin 命令卸载（remove），输出进内置终端"
                }
              >
                卸载
              </Button>
              <Switch
                checked={b.enabled}
                disabled={busy || b.official}
                title={
                  b.official
                    ? `${b.name} 是 dsh 宿主自带的插件包，停用同样会让 profile 起不来（base 提供核心运行时，web-app 提供 Web GUI）`
                    : undefined
                }
                onCheckedChange={(v) => toggleBundle(b.name, v)}
              />
            </div>
          ))}
          {(!detail || detail.bundles.length === 0) && (
            <div className="py-3 text-center text-xs text-muted-foreground">未读取到 bundle 列表</div>
          )}
        </div>
      </Card>

      <Card className="p-4">
        <div className="mb-2.5 flex flex-wrap items-center gap-2.5">
          <div className="text-[13px] font-semibold">
            其他依赖
            <span className="ml-2 text-xs font-normal text-muted-foreground">
              在 dependencies 里但未声明为插件 bundle 的包——可能是误装或残留，可手动卸载
            </span>
          </div>
          <span className="flex-1" />
          <Badge variant="outline">
            {(detail?.packages ?? []).filter((p) => !p.isBundle).length}
          </Badge>
        </div>
        <div className="space-y-1.5">
          {(detail?.packages ?? [])
            .filter((p) => !p.isBundle)
            .map((p) => (
              <div
                key={p.name}
                className="flex items-center gap-3 rounded-lg border border-border bg-background/50 px-3 py-1.5"
              >
                <span className="h-2 w-2 shrink-0 rounded-full bg-muted-foreground/40" />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1.5">
                    <span className="truncate font-mono text-[12.5px] font-medium">{p.name}</span>
                    <SourceBadge source={p.source} />
                  </div>
                  <div className="truncate text-[10.5px] text-muted-foreground" title={p.version ?? ""}>
                    {p.version ?? "—"}
                  </div>
                </div>
                {updateBadge(p.name)}
                <Button
                  size="sm"
                  variant="ghost"
                  className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                  disabled={busy}
                  onClick={() => {
                    setPurgeClone(false);
                    setPendingUninstall({
                      name: p.name,
                      isBundle: false,
                      cloneName: managedCloneName(updates?.[p.name]),
                    });
                  }}
                  title="手动卸载（官方 dsh plugin remove 命令）"
                >
                  <PackageX /> 卸载
                </Button>
              </div>
            ))}
          {(!detail || detail.packages.filter((p) => !p.isBundle).length === 0) && (
            <div className="py-3 text-center text-xs text-muted-foreground">
              没有未声明为插件的依赖，很干净
            </div>
          )}
        </div>
      </Card>

      {/* clone + link 的本地仓库清单（git pull 更新入口） */}
      <ClonedReposCard
        profile={profile}
        busy={busy}
        onToast={onToast}
        onPull={pullClone}
        onLinkInstall={(spec) => installMany([spec], "install")}
      />

      <Card className="p-4">
        <div className="mb-2.5 flex flex-wrap items-center gap-3">
          <div className="text-[13px] font-semibold">
            cordis.patch.yml
            <span className="ml-2 text-xs font-normal text-muted-foreground">
              插件启停的权威配置（disabled: true / 恢复）
            </span>
          </div>
          {dirty && <Badge variant="warning">未保存</Badge>}
          <span className="flex-1" />
          <Button size="sm" disabled={busy || !dirty} onClick={saveFile}>
            <Save /> 保存（自动备份）
          </Button>
          <Button size="sm" variant="outline" disabled={busy || !dirty} onClick={reload}>
            <RotateCcw /> 还原
          </Button>
        </div>
        <YamlEditor value={draft} onChange={(v) => { setDraft(v); setDirty(true); }} />
        <div className="mt-1.5 text-[11px] text-muted-foreground">
          专业 YAML 编辑器：语法高亮 + 行内错误校验，编辑保留全部注释；保存前自动备份，live 模式下 dsh 热重载
        </div>
      </Card>

      {/* 卸载确认（bundle 插件 / 手动依赖共用；clone 源可顺带删除本地目录） */}
      <AlertDialog open={pendingUninstall != null} onOpenChange={(o) => !o && setPendingUninstall(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {pendingUninstall?.isBundle
                ? `卸载插件 ${pendingUninstall.name}？`
                : `手动卸载依赖 ${pendingUninstall?.name}？`}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {pendingUninstall?.isBundle
                ? `将执行 dsh plugin remove，从 profile「${profile}」同时移除 bundles 与依赖声明（本地 link 插件目录不会被删除）。输出会实时显示在内置终端。`
                : `「${pendingUninstall?.name}」在 package.json 依赖中但未声明为插件 bundle，可能是误装或残留依赖。将从 profile「${profile}」中移除，其他插件若依赖它则会受影响。`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          {pendingUninstall?.cloneName && (
            <div className="flex items-start gap-2.5 rounded-lg border border-border px-3 py-2.5">
              <Switch
                id="purge-clone"
                checked={purgeClone}
                onCheckedChange={setPurgeClone}
                className="mt-0.5"
              />
              <div className="min-w-0 flex-1">
                <label htmlFor="purge-clone" className="text-[11.5px] font-medium">
                  同时删除本地克隆目录 <span className="font-mono">{pendingUninstall.cloneName}</span>
                </label>
                <p className="text-[10.5px] leading-relaxed text-muted-foreground">
                  该依赖来自 clone+link 安装，删除后 ~/.dsh-starter/git-plugins 下的这份克隆
                  （含未提交改动）会一并移除；不勾选则保留，可在「本地克隆仓库」里单独管理。
                </p>
              </div>
            </div>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                const target = pendingUninstall;
                setPendingUninstall(null);
                if (target) uninstall(target.name, purgeClone ? target.cloneName : null);
              }}
            >
              卸载
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      </div>
      {/* ── 隔离边界结束 ── */}

      {/* 安装插件对话框：npm / GitHub / 链接 / Clone 四种方式 */}
      <InstallPluginDialog
        profile={profile}
        open={installOpen}
        onClose={() => setInstallOpen(false)}
        onInstallMany={installMany}
        onCloneInstall={cloneInstall}
      />
    </div>
  );
}
