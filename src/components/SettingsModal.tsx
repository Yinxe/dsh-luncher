import { useEffect, useState } from "react";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogDescription, DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import type { EnvironmentInfo, Settings } from "../types";

interface Props {
  initial: Settings;
  env: EnvironmentInfo | null;
  onSave: (s: Settings) => void;
  onClose: () => void;
  onReveal: (path: string) => void;
}

function Field({ label, hint, children }: { label: string; hint?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="space-y-1.5">
      <label className="block text-xs text-muted-foreground">{label}</label>
      {children}
      {hint && <div className="text-[11px] leading-relaxed text-muted-foreground">{hint}</div>}
    </div>
  );
}

function CheckRow({
  checked, onChange, label,
}: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <label className="flex cursor-pointer items-center gap-2.5 py-0.5 text-[13px]">
      <Switch checked={checked} onCheckedChange={onChange} />
      {label}
    </label>
  );
}

export default function SettingsModal({ initial, env, onSave, onClose, onReveal }: Props) {
  const [draft, setDraft] = useState<Settings>(initial);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) =>
    setDraft((d) => ({ ...d, [k]: v }));

  return (
    <div onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <Dialog open onOpenChange={(o) => !o && onClose()}>
        <DialogContent>
          <DialogTitle>设置</DialogTitle>
          <DialogDescription>启动器自身的配置，实时保存到 ~/.dsh-launcher/settings.json</DialogDescription>

          <div className="space-y-4">
            <Field label="npm Registry（拉取版本列表与安装源，可换镜像加速）">
              <Input
                value={draft.registry}
                placeholder="https://registry.npmjs.org"
                onChange={(e) => set("registry", e.target.value)}
              />
              <div className="pt-1 text-[11px] text-muted-foreground">国内可用 https://registry.npmmirror.com</div>
            </Field>

            <Field label="Node 路径（留空自动探测；dsh 是 Node 程序，需要 Node.js）">
              <Input
                value={draft.nodePath}
                placeholder="自动探测 PATH / 内置运行时 / 常见位置 / nvm"
                onChange={(e) => set("nodePath", e.target.value)}
              />
              <div className="pt-1 text-[11px] text-muted-foreground">
                {env?.nodePath ? `当前探测到: ${env.nodePath} (v${env.node})` : "尚未探测到 Node"}
              </div>
            </Field>

            <Field label="Node 运行时镜像站（「一键安装 Node」下载用）">
              <Input
                value={draft.nodeMirror}
                placeholder="https://npmmirror.com/mirrors/node"
                onChange={(e) => set("nodeMirror", e.target.value)}
              />
              <div className="pt-1 text-[11px] text-muted-foreground">
                国内推荐 https://npmmirror.com/mirrors/node 或 https://mirrors.aliyun.com/nodejs-release
              </div>
            </Field>

            <Field label="终端模拟器（留空自动选择；Linux 下生效）">
              <Input
                value={draft.terminal}
                placeholder="auto"
                onChange={(e) => set("terminal", e.target.value)}
              />
              <div className="pt-1 text-[11px] text-muted-foreground">
                例如 /usr/bin/gnome-terminal。「启动」为内嵌运行，该终端仅用于独立窗口模式
              </div>
            </Field>

            <Field label="启动 dsh 的默认附加参数">
              <Input
                value={draft.defaultArgs}
                placeholder="例如 --preset qqbot"
                onChange={(e) => set("defaultArgs", e.target.value)}
              />
              <div className="pt-1 text-[11px] text-muted-foreground">
                Profile 实例在「Profile 实例」页选择与启停，选中即保存为默认
              </div>
            </Field>

            <Field label="启动器更新清单地址（返回 {version, notes, url} 的 JSON）">
              <Input
                type="text"
                value={draft.updateManifestUrl}
                placeholder="https://example.com/dsh-launcher/latest.json"
                onChange={(e) => set("updateManifestUrl", e.target.value)}
              />
              <div className="pt-1 text-[11px] text-muted-foreground">
                配置后启动器自动检查自身更新；正式发布也可启用 Tauri updater 签名更新
              </div>
            </Field>

            <div className="space-y-1.5 pt-1">
              <CheckRow
                checked={draft.autoCheckVersions}
                onChange={(v) => set("autoCheckVersions", v)}
                label="启动时自动刷新官方版本列表"
              />
              <CheckRow
                checked={draft.autoCheckUpdate}
                onChange={(v) => set("autoCheckUpdate", v)}
                label="启动时自动检查启动器更新"
              />
              <CheckRow
                checked={draft.closeToTray}
                onChange={(v) => set("closeToTray", v)}
                label="关闭窗口时最小化到托盘（否则直接退出并结束所有实例）"
              />
            </div>

            <Field label="数据目录（各版本安装在 versions/ 子目录下）">
              <div className="font-mono text-[11.5px] text-muted-foreground">{env?.versionsDir ?? "…"}</div>
              <Button size="sm" variant="outline" className="mt-1.5" onClick={() => env && onReveal(env.versionsDir)}>
                <FolderOpen /> 打开数据目录
              </Button>
            </Field>
          </div>

          <div className="flex justify-end gap-2.5 pt-1">
            <Button variant="outline" onClick={onClose}>取消</Button>
            <Button onClick={() => onSave(draft)}>保存</Button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
