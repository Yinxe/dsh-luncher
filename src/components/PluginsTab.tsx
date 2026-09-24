import { useCallback, useEffect, useRef, useState } from "react";
import {
  Cloud, FileArchive, GitBranch, Link2, Loader2, PackageX, Plus, RefreshCw,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Switch } from "@/components/ui/switch";
import ClonedReposCard from "@/components/ClonedReposCard";
import InstallPluginDialog from "@/components/InstallPluginDialog";
import { api } from "../api";
import type { ClonedPlugin, PluginUpdateInfo, ProfileDetail } from "../types";

interface Props {
  /** 工作台选中的 profile：本页所有操作只作用于它 */
  profile: string;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
  /** App 层 usePluginJobs 的运行中任务数（busy 锁与状态条数据源，终端面板已收走输出） */
  pluginRunningCount: number;
  /** 每次有插件任务结束 +1：触发本页 reload（原内置终端的 onFinished→reload） */
  pluginJobsTick: number;
  /** 「在终端中查看」：聚焦最近插件任务并打开右侧终端面板 */
  onOpenPluginTerminal: () => void;
}

/** 来源徽标：决定「怎么更新」的视觉线索 */
const SOURCE_META: Record<string, { label: string; icon: typeof Cloud; tip: string }> = {
  npm: { label: "npm", icon: Cloud, tip: "registry 包：按版本号检测更新（dsh plugin add 包名@latest）" },
  git: { label: "git 仓库", icon: GitBranch, tip: "GitHub 规格安装：按提交哈希检测更新" },
  "git-clone": { label: "clone+link", icon: GitBranch, tip: "本地克隆 + link：更新方式 git pull" },
  tarball: { label: "tgz 直链", icon: FileArchive, tip: "打包产物直链：无版本渠道，只能手动重装同一链接" },
  link: { label: "本地 link", icon: Link2, tip: "本地软链：改代码即时生效，无版本渠道" },
  file: { label: "本地 file", icon: Link2, tip: "本地文件依赖：无版本渠道" },
  workspace: { label: "workspace", icon: Link2, tip: "workspace 依赖：跟随工作区" },
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

/**
 * 工作台「插件」Tab：profile 由外层工作台选定（props 注入），
 * bundle 启停、其他依赖、克隆仓库、检查更新/升级都在这里。
 * 安装/卸载/升级的实时输出统一进右侧「通用终端面板」（App 层持有任务流），
 * 本页只在有任务时显示进度条 + 「在终端中查看」跳转。
 * cordis.patch.yml 的编辑统一在「配置文件」Tab，本页不再嵌一份编辑器。
 */
export default function PluginsTab({
  profile, onToast, pluginRunningCount, pluginJobsTick, onOpenPluginTerminal,
}: Props) {
  const [detail, setDetail] = useState<ProfileDetail | null>(null);
  const [reloadMode, setReloadMode] = useState<string>("live");
  const [bundleBusy, setBundleBusy] = useState(false);
  const [installOpen, setInstallOpen] = useState(false);
  const [updates, setUpdates] = useState<Record<string, PluginUpdateInfo> | null>(null);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [pendingUninstall, setPendingUninstall] = useState<
    { name: string; isBundle: boolean; cloneName: string | null } | null
  >(null);
  const [purgeClone, setPurgeClone] = useState(false);
  /** 卸载时顺带清理 cordis.patch.yml 里该插件的条目（0.1.7+；官方 remove 不管这部分） */
  const [cleanPatch, setCleanPatch] = useState(true);

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
      // 快速切换 profile 时，旧请求可能后返回：不能覆盖新 profile 的 detail
      if (latestProfileRef.current !== profile) return;
      setDetail(d);
      setReloadMode(mode);
      setUpdates(null);
    } catch (e) {
      if (latestProfileRef.current !== profile) return;
      onToast("err", `读取 profile 配置失败: ${e}`);
    }
  }, [profile, onToast]);

  useEffect(() => {
    reload();
  }, [reload]);

  // 插件任务结束（App 层任务流 tick 递增）→ 刷新本页列表。
  // 首次挂载时 reload 已由上面的 effect 跑过，这里只响应挂载后的增量。
  const lastTickRef = useRef(pluginJobsTick);
  useEffect(() => {
    if (lastTickRef.current === pluginJobsTick) return;
    lastTickRef.current = pluginJobsTick;
    reload();
  }, [pluginJobsTick, reload]);

  /** 有插件任务在跑或正在切换 bundle 时，禁用会互踩的操作 */
  const busy = pluginRunningCount > 0 || bundleBusy;

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
    (name: string, cloneDir: string | null, cleanConfig: boolean) => {
      api
        .pluginUninstall(profile, name, cloneDir, cleanConfig)
        .then(() => onToast("info", `已开始卸载 ${name}（输出见终端面板）`))
        .catch((e) => onToast("err", String(e)));
    },
    [profile, onToast]
  );

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
            `${mode === "upgrade" ? "升级" : "安装"} ${specs.length} 个包：输出见终端面板`
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
        .then(() => onToast("info", "已开始 clone + link 安装：进度见右侧终端面板"))
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
        .then(() => onToast("info", `已开始 git pull 更新 ${name}：输出见终端面板`))
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
          .then(() => onToast("info", `已开始 git pull 升级 ${u.name}：输出见终端面板`))
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

  return (
    <div className="space-y-4">
      {/* profile 由工作台选定，这里只留一行生效模式提示（原「工作区卡 + 下拉」收进左侧列表） */}
      <div className="flex flex-wrap items-center gap-2 text-[11px] text-muted-foreground">
        <span>安装 / 卸载 / 升级 / 克隆只作用于当前 profile：</span>
        <span className="font-mono text-[11.5px] font-semibold text-foreground">{profile}</span>
        <Badge
          variant={reloadMode === "live" ? "success" : "warning"}
          title={
            reloadMode === "live"
              ? "该 profile 的 patchReload = live：保存 cordis.patch.yml 后即时生效"
              : "该 profile 的 patchReload = startup：改完需要重启实例才生效"
          }
        >
          {reloadMode === "live" ? "live 即时生效" : "startup 需重启"}
        </Badge>
        {detail && (
          <span>
            {detail.bundles.length} 个插件 · {detail.packages.length} 个依赖
          </span>
        )}
      </div>

      {/* 插件任务进行中：输出已统一到右侧通用终端面板，这里只留进度条与跳转 */}
      {pluginRunningCount > 0 && (
        <Card className="flex items-center gap-2 px-4 py-2.5 text-[12px]">
          <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-teal-500" />
          <span className="min-w-0 truncate">
            {pluginRunningCount} 个插件任务进行中…输出正流入右侧终端面板
          </span>
          <span className="flex-1" />
          <Button size="sm" variant="outline" onClick={onOpenPluginTerminal}>
            在终端中查看
          </Button>
        </Card>
      )}

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
                  setCleanPatch(true);
                  setPendingUninstall({
                    name: b.name,
                    isBundle: true,
                    cloneName: managedCloneName(updates?.[b.name]),
                  });
                }}
                title={
                  b.official
                    ? `${b.name} 是 dsh 宿主自带的插件包，卸载会让这个 profile 起不来；要折腾请用「恢复模式」新建一个实例`
                    : "通过官方 dsh plugin 命令卸载（remove），输出进终端面板"
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
                  setCleanPatch(true);
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
                ? `将执行 dsh plugin remove，从 profile「${profile}」同时移除 bundles 与依赖声明（本地 link 插件目录不会被删除）。输出会实时显示在右侧终端面板。`
                : `「${pendingUninstall?.name}」在 package.json 依赖中但未声明为插件 bundle，可能是误装或残留依赖。将从 profile「${profile}」中移除，其他插件若依赖它则会受影响。`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <div className="flex items-start gap-2.5 rounded-lg border border-border px-3 py-2.5">
            <Switch
              id="clean-patch"
              checked={cleanPatch}
              onCheckedChange={setCleanPatch}
              className="mt-0.5"
            />
            <div className="min-w-0 flex-1">
              <label htmlFor="clean-patch" className="text-[11.5px] font-medium">
                同时清理插件在 cordis.patch.yml 中的配置
              </label>
              <p className="text-[10.5px] leading-relaxed text-muted-foreground">
                官方卸载命令只维护 package.json 依赖与 bundles，不会清掉补丁里该插件的数据
                （禁用块、配置条目、insert 挂载）。开启后卸载成功会按行移除这些条目
                （原件备份为 cordis.patch.starter-bak）；仅影响 0.1.7+ 的 profile 配置，
                旧版的全局 settings.yaml 不会被触碰。
              </p>
            </div>
          </div>
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
                const clean = cleanPatch;
                setPendingUninstall(null);
                if (target) uninstall(target.name, purgeClone ? target.cloneName : null, clean);
              }}
            >
              卸载
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

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
