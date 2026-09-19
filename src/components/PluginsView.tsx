import { useCallback, useEffect, useState } from "react";
import { RotateCcw, Save } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api } from "../api";
import type { ProfileDetail, PluginEntryInfo } from "../types";

interface Props {
  profiles: string[];
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

type FileKey = "cordis.patch.yml" | "package.json";

export default function PluginsView({ profiles, onToast }: Props) {
  const [profile, setProfile] = useState<string>(profiles[0] ?? "");
  const [detail, setDetail] = useState<ProfileDetail | null>(null);
  const [plugins, setPlugins] = useState<PluginEntryInfo[]>([]);
  const [reloadMode, setReloadMode] = useState<string>("live");
  const [editFile, setEditFile] = useState<FileKey>("cordis.patch.yml");
  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    // 默认选中 web（与启动器整体默认一致）；不存在才取第一个
    if (!profile && profiles.length > 0) {
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
      setEditFile((f) => {
        setDraft(f === "package.json" ? d.packageRaw : d.patchRaw);
        return f;
      });
      setDirty(false);
    } catch (e) {
      onToast("err", `读取 profile 配置失败: ${e}`);
    }
  }, [profile, onToast]);

  useEffect(() => {
    reload();
  }, [reload]);

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

  const switchFile = useCallback(
    async (f: FileKey) => {
      if (dirty && !window.confirm("当前编辑未保存，确定切换文件？")) return;
      try {
        const text = await api.readProfileFile(profile, f);
        setEditFile(f);
        setDraft(text);
        setDirty(false);
      } catch (e) {
        onToast("err", String(e));
      }
    },
    [profile, dirty, onToast]
  );

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
        <div className="mb-2.5 text-[13px] font-semibold">
          插件包
          <span className="ml-2 text-xs font-normal text-muted-foreground">
            dsh.profile.bundles，启停需重启实例
          </span>
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
                title="从 bundles 与依赖声明中移除"
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
          <div className="text-[13px] font-semibold">原始配置编辑</div>
          <Select value={editFile} onValueChange={(v) => switchFile(v as FileKey)}>
            <SelectTrigger className="h-7 w-44 font-mono text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="cordis.patch.yml">cordis.patch.yml</SelectItem>
              <SelectItem value="package.json">package.json</SelectItem>
            </SelectContent>
          </Select>
          {dirty && <Badge variant="warning">未保存</Badge>}
          <span className="flex-1" />
          <Button size="sm" disabled={busy || !dirty} onClick={saveFile}>
            <Save /> 保存（自动备份）
          </Button>
          <Button size="sm" variant="outline" disabled={busy || !dirty} onClick={reload}>
            <RotateCcw /> 还原
          </Button>
        </div>
        <Textarea rows={14} spellCheck={false} value={draft} onChange={(e) => { setDraft(e.target.value); setDirty(true); }} />
        <div className="mt-1.5 text-[11px] text-muted-foreground">
          编辑保留全部注释；保存前做语法校验，原文件自动备份为 *.launcher-bak-*
        </div>
      </Card>
    </div>
  );
}
