import { useCallback, useEffect, useState } from "react";
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
    if (!profile && profiles.length > 0) setProfile(profiles[0]);
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

  const guard = useCallback(async () => {
    if (!profile) {
      onToast("err", "请先在 Profile 实例中选择一个 profile");
      return false;
    }
    return true;
  }, [profile, onToast]);

  const togglePlugin = useCallback(
    async (p: PluginEntryInfo) => {
      if (!(await guard())) return;
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
    [profile, reloadMode, reload, guard, onToast]
  );

  const toggleBundle = useCallback(
    async (name: string, enabled: boolean) => {
      if (!(await guard())) return;
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
    [profile, reload, guard, onToast]
  );

  const uninstall = useCallback(
    async (name: string) => {
      if (
        !window.confirm(
          `从 profile「${profile}」卸载插件 ${name}？\n将同时移除 bundles 与依赖声明（本地 link 插件目录不会被删除）。`
        )
      )
        return;
      if (!(await guard())) return;
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
    [profile, reload, guard, onToast]
  );

  const saveFile = useCallback(async () => {
    if (!(await guard())) return;
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
  }, [profile, editFile, draft, guard, onToast, reload]);

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
    return <div className="page-empty">未找到任何 profile（$DSH_HOME/profiles 为空）</div>;
  }

  return (
    <div className="page">
      <div className="page-head">
        <h2>插件管理</h2>
        <div className="prof-chip">
          <span className="prof-label">Profile</span>
          <span className="prof-value">{profile}</span>
          <span className="prof-caret">▾</span>
          <select
            className="prof-native"
            value={profile}
            onChange={(e) => setProfile(e.target.value)}
          >
            {profiles.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        </div>
        <span className="reload-hint">
          patchReload = <b>{reloadMode}</b>
          {reloadMode === "live"
            ? "（保存后即时生效）"
            : "（需重启 Profile 实例生效）"}
        </span>
      </div>

      <section className="panel">
        <h3>插件服务（来自 cordis.patch，可动态启停）</h3>
        <div className="plugin-grid">
          {plugins.map((p) => (
            <div key={p.id} className={`plugin-row${p.disabled ? " off" : ""}`}>
              <span className={`inst-dot ${p.disabled ? "stopped" : "ready"}`} />
              <div className="inst-info">
                <div className="inst-name">{p.id}</div>
                <div className="inst-meta">
                  {p.bundle ? `来自 ${p.bundle}` : "来自用户 patch 层"}
                  {p.disabled && !p.managed ? " · 手动禁用" : ""}
                </div>
              </div>
              <button
                className={`sm ${p.disabled ? "" : "ghost"}`}
                disabled={busy}
                onClick={() => togglePlugin(p)}
              >
                {p.disabled ? "启用" : "停用"}
              </button>
            </div>
          ))}
          {plugins.length === 0 && <div className="inst-meta">未读取到插件清单</div>}
        </div>
      </section>

      <section className="panel">
        <h3>插件包（dsh.profile.bundles，启停需重启实例）</h3>
        <div className="plugin-grid">
          {(detail?.bundles ?? []).map((b) => (
            <div key={b.name} className={`plugin-row${b.enabled ? "" : " off"}`}>
              <span className={`inst-dot ${b.enabled ? "ready" : "stopped"}`} />
              <div className="inst-info">
                <div className="inst-name">{b.name}</div>
                <div className="inst-meta">
                  {b.version ?? "—"} · {b.source}
                </div>
              </div>
              <button
                className="sm ghost danger"
                disabled={busy}
                onClick={() => uninstall(b.name)}
                title="从 bundles 与依赖声明中移除"
              >
                卸载
              </button>
              <button
                className={`sm ${b.enabled ? "ghost" : ""}`}
                disabled={busy}
                onClick={() => toggleBundle(b.name, !b.enabled)}
              >
                {b.enabled ? "停用" : "启用"}
              </button>
            </div>
          ))}
          {(!detail || detail.bundles.length === 0) && (
            <div className="inst-meta">未读取到 bundle 列表</div>
          )}
        </div>
      </section>

      <section className="panel">
        <div className="panel-head">
          <h3>原始配置编辑</h3>
          <div className="tag-group">
            {(["cordis.patch.yml", "package.json"] as FileKey[]).map((f) => (
              <button
                key={f}
                className={`tag${editFile === f ? " active" : ""}`}
                onClick={() => switchFile(f)}
              >
                {f}
              </button>
            ))}
          </div>
          <span style={{ flex: 1 }} />
          <button className="sm primary" disabled={busy || !dirty} onClick={saveFile}>
            {dirty ? "保存（自动备份）" : "已保存"}
          </button>
        </div>
        <textarea
          className="code-editor"
          spellCheck={false}
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value);
            setDirty(true);
          }}
        />
        <div className="hint-line">
          编辑保留全部注释；保存前做语法校验，原文件自动备份为 *.launcher-bak-*
        </div>
      </section>
    </div>
  );
}
