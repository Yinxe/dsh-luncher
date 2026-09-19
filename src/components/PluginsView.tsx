import { useCallback, useEffect, useState } from "react";
import { PackageX, Plus, RefreshCw, RotateCcw, Save } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import InstallPluginDialog from "@/components/InstallPluginDialog";
import YamlEditor from "@/components/YamlEditor";
import { api, events } from "../api";
import type { PluginUpdateInfo, ProfileDetail } from "../types";

interface Props {
  profiles: string[];
  /** 从 Profile 实例页跳转过来时预选中的 profile（仅挂载时生效） */
  initialProfile?: string | null;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
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
  const [busy, setBusy] = useState(false);
  const [jobLine, setJobLine] = useState<string | null>(null);
  const [installOpen, setInstallOpen] = useState(false);
  const [updates, setUpdates] = useState<Record<string, PluginUpdateInfo> | null>(null);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [pendingUninstall, setPendingUninstall] = useState<{ name: string; isBundle: boolean } | null>(null);

  useEffect(() => {
    // profiles 晚于挂载加载时，同样默认选中 web
    if (profiles.length > 0 && !profiles.includes(profile)) {
      setProfile(profiles.includes("web") ? "web" : profiles[0]);
    }
  }, [profiles, profile]);

  const reload = useCallback(async () => {
    if (!profile) return;
    try {
      const [d, mode] = await Promise.all([
        api.getProfileDetail(profile),
        api.getPatchReload(profile),
      ]);
      setDetail(d);
      setReloadMode(mode);
      setDraft(d.patchRaw);
      setDirty(false);
      setUpdates(null);
    } catch (e) {
      onToast("err", `读取 profile 配置失败: ${e}`);
    }
  }, [profile, onToast]);

  useEffect(() => {
    reload();
  }, [reload]);

  // 后台插件命令（dsh plugin）输出流
  useEffect(() => {
    let un: (() => void) | undefined;
    events
      .onPluginLog?.((e) => {
        if (e.profile !== profile) return;
        if (e.done) {
          setBusy(false);
          setJobLine(null);
          onToast(e.ok ? "ok" : "err", e.ok ? "插件命令执行完成" : `插件命令失败：${e.line}`);
          reload();
        } else {
          setJobLine(e.line);
        }
      })
      ?.then((u) => (un = u))
      .catch(() => undefined);
    return () => un?.();
  }, [profile, reload, onToast]);

  const toggleBundle = useCallback(
    async (name: string, enabled: boolean) => {
      setBusy(true);
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
        setBusy(false);
      }
    },
    [profile, reloadMode, reload, onToast]
  );

  const uninstall = useCallback(
    async (name: string) => {
      setBusy(true);
      try {
        await api.uninstallBundle(profile, name);
        onToast("ok", `插件 ${name} 已卸载`);
        await reload();
      } catch (e) {
        onToast("err", String(e));
      } finally {
        setBusy(false);
      }
    },
    [profile, reload, onToast]
  );

  const saveFile = useCallback(async () => {
    setBusy(true);
    try {
      await api.writeProfileFile(profile, editFile, draft);
      onToast("ok", `${editFile} 已保存（原文件已备份）`);
      await reload();
    } catch (e) {
      onToast("err", String(e));
    } finally {
      setBusy(false);
    }
  }, [profile, editFile, draft, onToast, reload]);

  const doCheckUpdates = useCallback(async () => {
    setCheckingUpdates(true);
    try {
      const list = await api.checkPluginUpdates(profile);
      const map: Record<string, PluginUpdateInfo> = {};
      for (const u of list) map[u.name] = u;
      setUpdates(map);
      const n = list.filter((u) => u.hasUpdate).length;
      const skipped = list.filter((u) => !u.checked).length;
      onToast(
        n > 0 ? "info" : "ok",
        n > 0
          ? `${n} 个包有新版本可用${skipped > 0 ? `（${skipped} 个本地源/无法检测已跳过）` : ""}`
          : skipped > 0
          ? `全部已最新（${skipped} 个本地源/无法检测已跳过）`
          : "全部已最新"
      );
    } catch (e) {
      onToast("err", `检查更新失败: ${e}`);
    } finally {
      setCheckingUpdates(false);
    }
  }, [profile, onToast]);

  const install = useCallback(

    async (spec: string) => {
      const name = spec.trim();
      if (!name) return;
      setBusy(true);
      try {
        const tail = await api.installBundle(profile, name);
        onToast("ok", `插件 ${name} 已安装（官方 dsh plugin 命令）\n${tail}`);
        await reload();
      } catch (e) {
        onToast("err", String(e));
      } finally {
        setBusy(false);
      }
    },
    [profile, reload, onToast]
  );

  const updateBadge = (name: string) => {
    const u = updates?.[name];
    if (!u?.hasUpdate) return null;
    const tip =
      u.source === "git"
        ? `远端提交 ${u.remoteCommit?.slice(0, 7) ?? "?"} ≠ 已装 ${u.installedCommit?.slice(0, 7) ?? "?"}`
        : `已装 ${u.installedVersion ?? "?"} → 最新 ${u.latestVersion ?? "?"}`;
    return (
      <div className="flex shrink-0 items-center gap-1.5">
        <Badge variant="warning" title={tip}>
          {u.source === "git" ? "有新提交" : `可升级 ${u.latestVersion ?? ""}`}
        </Badge>
        {u.updateSpec && (
          <Button
            size="sm"
            variant="outline"
            className="h-6 px-2 text-[11px]"
            disabled={busy || checkingUpdates}
            title={`执行 dsh plugin add ${u.updateSpec}`}
            onClick={() => {
              setUpdates(null);
              install(u.updateSpec!);
            }}
          >
            升级
          </Button>
        )}
      </div>
    );
  };

  if (profiles.length === 0) {
    return (
      <Card className="p-10 text-center text-muted-foreground">
        未找到任何 profile（$DSH_HOME/profiles 为空）
      </Card>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="text-base font-semibold">插件管理</h2>
        {jobLine && (
          <Badge variant="info" className="max-w-md truncate font-mono" title={jobLine}>
            {jobLine}
          </Badge>
        )}
        <Select value={profile} onValueChange={setProfile}>
          <SelectTrigger className="w-52 font-mono">
            <SelectValue placeholder="选择 profile" />
          </SelectTrigger>
          <SelectContent>
            {profiles.map((p) => (
              <SelectItem key={p} value={p}>
                {p}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Badge variant={reloadMode === "live" ? "success" : "warning"}>
          patchReload = {reloadMode}
          {reloadMode === "live" ? "（保存后即时生效）" : "（需重启实例生效）"}
        </Badge>
      </div>

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
            title="npm 包比对 registry 最新版本；GitHub 仓库比对最新提交（本地 link 源跳过）"
          >
            <RefreshCw className={checkingUpdates ? "animate-spin" : ""} /> 检查更新
          </Button>
          <Button
            size="sm"
            disabled={busy}
            onClick={() => setInstallOpen(true)}
            title="搜索 npm / GitHub 仓库，看描述后安装"
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
                <div className="truncate font-mono text-[12.5px] font-medium">{b.name}</div>
                <div className="truncate text-[10.5px] text-muted-foreground">
                  {b.version ?? "—"} · {b.source}
                  {b.pluginIds.length > 0 && ` · 插件 ID: ${b.pluginIds.join(", ")}`}
                </div>
              </div>
              {updateBadge(b.name)}
              <Button
                size="sm"
                variant="ghost"
                className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                disabled={busy}
                onClick={() => setPendingUninstall({ name: b.name, isBundle: true })}
                title="通过官方 dsh plugin 命令卸载（remove）"
              >
                卸载
              </Button>
              <Switch
                checked={b.enabled}
                disabled={busy}
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
                  <div className="truncate font-mono text-[12.5px] font-medium">{p.name}</div>
                  <div className="truncate text-[10.5px] text-muted-foreground">
                    {p.version ?? "—"} · {p.source}
                  </div>
                </div>
                {updateBadge(p.name)}
                <Button
                  size="sm"
                  variant="ghost"
                  className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                  disabled={busy}
                  onClick={() => setPendingUninstall({ name: p.name, isBundle: false })}
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

      {/* 卸载确认（bundle 插件 / 手动依赖共用） */}
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
                ? `将从 profile「${profile}」同时移除 bundles 与依赖声明（本地 link 插件目录不会被删除）。`
                : `「${pendingUninstall?.name}」在 package.json 依赖中但未声明为插件 bundle，可能是误装或残留依赖。将从 profile「${profile}」中移除，其他插件若依赖它则会受影响。`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                const name = pendingUninstall?.name;
                setPendingUninstall(null);
                if (name) uninstall(name);
              }}
            >
              卸载
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* 安装插件对话框：搜索/预览 → 描述 → 安装 */}
      <InstallPluginDialog
        profile={profile}
        open={installOpen}
        onClose={() => setInstallOpen(false)}
        onInstall={install}
      />
    </div>
  );
}
