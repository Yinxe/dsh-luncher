import { useEffect, useState } from "react";
import type { EnvironmentInfo, Settings } from "../types";

interface Props {
  initial: Settings;
  env: EnvironmentInfo | null;
  onSave: (s: Settings) => void;
  onClose: () => void;
  onReveal: (path: string) => void;
}

export default function SettingsModal({ initial, env, onSave, onClose, onReveal }: Props) {
  const [draft, setDraft] = useState<Settings>(initial);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) =>
    setDraft((d) => ({ ...d, [k]: v }));

  return (
    <div className="modal-mask" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal">
        <h2>设置</h2>

        <div className="field">
          <label>npm Registry（拉取版本列表与安装源，可换镜像加速）</label>
          <input
            type="text"
            value={draft.registry}
            placeholder="https://registry.npmjs.org"
            onChange={(e) => set("registry", e.target.value)}
          />
          <div className="hint">国内可用 https://registry.npmmirror.com</div>
        </div>

        <div className="field">
          <label>Node 路径（留空自动探测；dsh 是 Node 程序，需要 Node.js）</label>
          <input
            type="text"
            value={draft.nodePath}
            placeholder="自动探测 PATH / 常见位置 / nvm"
            onChange={(e) => set("nodePath", e.target.value)}
          />
          <div className="hint">
            {env?.nodePath ? `当前探测到: ${env.nodePath} (v${env.node})` : "尚未探测到 Node"}
          </div>
        </div>

        <div className="field">
          <label>终端模拟器（留空自动选择；Linux 下生效）</label>
          <input
            type="text"
            value={draft.terminal}
            placeholder="auto"
            onChange={(e) => set("terminal", e.target.value)}
          />
          <div className="hint">
            例如 /usr/bin/gnome-terminal。dsh 是交互式 CLI，会在新终端窗口中运行
          </div>
        </div>

        <div className="field">
          <label>启动 dsh 的默认附加参数</label>
          <input
            type="text"
            value={draft.defaultArgs}
            placeholder="例如 --preset qqbot"
            onChange={(e) => set("defaultArgs", e.target.value)}
          />
        </div>

        <div className="field">
          <label>启动器更新清单地址（返回 {"{version, notes, url}"} 的 JSON）</label>
          <input
            type="text"
            value={draft.updateManifestUrl}
            placeholder="https://example.com/dsh-launcher/latest.json"
            onChange={(e) => set("updateManifestUrl", e.target.value)}
          />
          <div className="hint">
            配置后启动器会自动检查自身更新；正式发布也可启用 Tauri updater 签名更新
          </div>
        </div>

        <label className="check-row">
          <input
            type="checkbox"
            checked={draft.autoCheckVersions}
            onChange={(e) => set("autoCheckVersions", e.target.checked)}
          />
          启动时自动刷新官方版本列表
        </label>
        <label className="check-row">
          <input
            type="checkbox"
            checked={draft.autoCheckUpdate}
            onChange={(e) => set("autoCheckUpdate", e.target.checked)}
          />
          启动时自动检查启动器更新
        </label>
        <label className="check-row">
          <input
            type="checkbox"
            checked={draft.closeToTray}
            onChange={(e) => set("closeToTray", e.target.checked)}
          />
          关闭窗口时最小化到托盘（否则直接退出）
        </label>

        <div className="field" style={{ marginTop: 10 }}>
          <label>数据目录（各版本安装在 versions/ 子目录下）</label>
          <div className="hint" style={{ fontSize: 12 }}>
            {env?.versionsDir ?? "…"}
          </div>
          <button className="sm" onClick={() => env && onReveal(env.versionsDir)}>
            打开数据目录
          </button>
        </div>

        <div className="footer">
          <button onClick={onClose}>取消</button>
          <button className="primary" onClick={() => onSave(draft)}>
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
