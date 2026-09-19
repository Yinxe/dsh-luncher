import { useCallback, useEffect, useState } from "react";
import { Plus, RotateCcw, Save } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import YamlEditor from "@/components/YamlEditor";
import { api, events } from "../api";
import type { ProfileDetail, PluginEntryInfo } from "../types";

interface Props {
  profiles: string[];
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

export default function PluginsView({ profiles, onToast }: Props) {
  // 默认选中 web（与启动器整体默认一致）；不存在才取第一个
  const [profile, setProfile] = useState<string>(() =>
    profiles.includes("web") ? "web" : (profiles[0] ?? "")
  );
  const [detail, setDetail] = useState<ProfileDetail | null>(null);
  const [plugins, setPlugins] = useState<PluginEntryInfo[]>([]);
  const [reloadMode, setReloadMode] = useState<string>("live");
  const [editFile] = useState<string>("cordis.patch.yml");
  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [jobLine, setJobLine] = useState<string | null>(null);
  const [newPkg, setNewPkg] = useState("");

  useEffect(() => {
    // profiles 晚于挂载加载时，同样默认选中 web
    if (profiles.length > 0 && !profiles.includes(profile)) {
      setProfile(profiles.includes("web") ? "web" : profiles[0]);
    }
  }, [profiles, profile]);

  const reload = useCallback(async () => {
    if (!profile) return;
    try {
      const [d, p, mode] = await Promise.all([
        api.getProfileDetail(profile),
        api.listProfilePlugins(profile),
        api.getPatchReload(profile),
      ]);
      setDetail(d);
      setPlugins(p);
      setReloadMode(mode);
      setDraft(d.patchRaw);
      setDirty(false);
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

  const togglePlugin = useCallback(
    async (p: PluginEntryInfo) => {
      setBusy(true);
      try {
        await api.setProfilePlugin(profile, p.id, !p.disabled);
        onToast(
          "ok",
          `插件 ${p.id} 已${p.disabled ? "启用" : "停用"}${
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

  const toggleBundle = useCallback(
    async (name: string, enabled: boolean) => {
      setBusy(true);
      try {
        await api.setBundleEnabled(profile, name, enabled);
        onToast("ok", `插件包 ${name} 已${enabled ? "启用" : "停用"}，重启实例后生效`);
        await reload();
      } catch (e) {
        onToast("err", String(e));
      } finally {
        setBusy(false);
      }
    },
    [profile, reload, onToast]
  );

  const uninstall = useCallback(
    async (name: string) => {
      if (
        !window.confirm(
          `从 profile「${profile}」卸载插件 ${name}？\n将同时移除 bundles 与依赖声明（本地 link 插件目录不会被删除）。`
        )
      )
        return;
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

  const installNew = useCallback(async () => {
    const name = newPkg.trim();
    if (!name) return;
    setBusy(true);
    try {
      const tail = await api.installBundle(profile, name);
      onToast("ok", `插件 ${name} 已安装（官方 dsh plugin 命令）\n${tail}`);
      setNewPkg("");
      await reload();
    } catch (e) {
      onToast("err", String(e));
    } finally {
      setBusy(false);
    }
  }, [profile, newPkg, reload, onToast]);

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
        <div className="mb-2.5 text-[13px] font-semibold">
          插件服务
          <span className="ml-2 text-xs font-normal text-muted-foreground">
            来自 cordis.patch 层，可动态启停
          </span>
        </div>
        <div className="space-y-1.5">
          {plugins.map((p) => (
            <div
              key={p.id}
              className={`flex items-center gap-3 rounded-lg border border-border bg-background/50 px-3 py-1.5 ${
                p.disabled ? "opacity-60" : ""
              }`}
            >
              <span className={`h-2 w-2 shrink-0 rounded-full ${p.disabled ? "bg-muted-foreground/40" : "bg-emerald-500"}`} />
              <div className="min-w-0 flex-1">
                <div className="font-mono text-[12.5px] font-medium">{p.id}</div>
                <div className="text-[10.5px] text-muted-foreground">
                  {p.bundle ? `来自 ${p.bundle}` : "来自用户 patch 层"}
                  {p.disabled && !p.managed ? " · 手动禁用" : ""}
                </div>
              </div>
              <Switch
                checked={!p.disabled}
                disabled={busy}
                onCheckedChange={(v) => togglePlugin({ ...p, disabled: !v })}
              />
            </div>
          ))}
          {plugins.length === 0 && (
            <div className="py-3 text-center text-xs text-muted-foreground">未读取到插件清单</div>
          )}
        </div>
      </Card>

      <Card className="p-4">
        <div className="mb-2.5 flex flex-wrap items-center gap-2.5">
          <div className="text-[13px] font-semibold">
            插件包
            <span className="ml-2 text-xs font-normal text-muted-foreground">
              dsh.profile.bundles
            </span>
          </div>
          <span className="flex-1" />
          <Input
            className="h-7 w-56 font-mono text-xs"
            placeholder="包名，如 @dshp/mcwiki-search"
            value={newPkg}
            onChange={(e) => setNewPkg(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && installNew()}
          />
          <Button size="sm" variant="outline" disabled={busy || !newPkg.trim()} onClick={installNew}>
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
                <div className="text-[10.5px] text-muted-foreground">
                  {b.version ?? "—"} · {b.source}
                </div>
              </div>
              <Button
                size="sm"
                variant="ghost"
                className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                disabled={busy}
                onClick={() => uninstall(b.name)}
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
    </div>
  );
}
