import { useState } from "react";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogTitle,
} from "@/components/ui/dialog";
import {
  Field, FieldDescription, FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
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

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) =>
    setDraft((d) => ({ ...d, [k]: v }));

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-md">
        <DialogTitle>设置</DialogTitle>
        <DialogDescription>启动器自身的配置，实时保存到 ~/.dsh-launcher/settings.json</DialogDescription>

        <div className="space-y-4">
          <Field>
            <FieldLabel htmlFor="set-registry">npm Registry（拉取版本列表与安装源，可换镜像加速）</FieldLabel>
            <Input
              id="set-registry"
              value={draft.registry}
              placeholder="https://registry.npmjs.org"
              onChange={(e) => set("registry", e.target.value)}
            />
            <FieldDescription>国内可用 https://registry.npmmirror.com</FieldDescription>
          </Field>

          <Field>
            <FieldLabel htmlFor="set-nodepath">Node 路径（留空自动探测；dsh 是 Node 程序，需要 Node.js）</FieldLabel>
            <Input
              id="set-nodepath"
              value={draft.nodePath}
              placeholder="自动探测 PATH / 内置运行时 / 常见位置 / nvm"
              onChange={(e) => set("nodePath", e.target.value)}
            />
            <FieldDescription>
              {env?.nodePath ? `当前探测到: ${env.nodePath} (v${env.node})` : "尚未探测到 Node"}
            </FieldDescription>
          </Field>

          <Field>
            <FieldLabel htmlFor="set-mirror">Node 运行时镜像站（「一键安装 Node」下载用）</FieldLabel>
            <Input
              id="set-mirror"
              value={draft.nodeMirror}
              placeholder="https://npmmirror.com/mirrors/node"
              onChange={(e) => set("nodeMirror", e.target.value)}
            />
            <FieldDescription>
              国内推荐 https://npmmirror.com/mirrors/node 或 https://mirrors.aliyun.com/nodejs-release
            </FieldDescription>
          </Field>

          <Field>
            <FieldLabel htmlFor="set-terminal">终端模拟器（留空自动选择；Linux 下生效）</FieldLabel>
            <Input
              id="set-terminal"
              value={draft.terminal}
              placeholder="auto"
              onChange={(e) => set("terminal", e.target.value)}
            />
            <FieldDescription>
              例如 /usr/bin/gnome-terminal。「启动」为内嵌运行，该终端仅用于独立窗口模式
            </FieldDescription>
          </Field>

          <Field>
            <FieldLabel htmlFor="set-args">启动 dsh 的默认附加参数</FieldLabel>
            <Input
              id="set-args"
              value={draft.defaultArgs}
              placeholder="例如 --preset qqbot"
              onChange={(e) => set("defaultArgs", e.target.value)}
            />
            <FieldDescription>
              Profile 实例在「Profile 实例」页选择与启停，选中即保存为默认
            </FieldDescription>
          </Field>

          <Field>
            <FieldLabel htmlFor="set-manifest">启动器更新清单地址（返回 {`{version, notes, url}`} 的 JSON）</FieldLabel>
            <Input
              id="set-manifest"
              type="text"
              value={draft.updateManifestUrl}
              placeholder="https://example.com/dsh-launcher/latest.json"
              onChange={(e) => set("updateManifestUrl", e.target.value)}
            />
            <FieldDescription>
              配置后启动器自动检查自身更新；正式发布也可启用 Tauri updater 签名更新
            </FieldDescription>
          </Field>

          <div className="space-y-2.5 pt-1">
            <div className="flex items-center gap-2.5">
              <Switch
                id="set-auto-versions"
                checked={draft.autoCheckVersions}
                onCheckedChange={(v) => set("autoCheckVersions", v)}
              />
              <Label htmlFor="set-auto-versions" className="text-[13px] font-normal">
                启动时自动刷新官方版本列表
              </Label>
            </div>
            <div className="flex items-center gap-2.5">
              <Switch
                id="set-auto-update"
                checked={draft.autoCheckUpdate}
                onCheckedChange={(v) => set("autoCheckUpdate", v)}
              />
              <Label htmlFor="set-auto-update" className="text-[13px] font-normal">
                启动时自动检查启动器更新
              </Label>
            </div>
            <div className="flex items-center gap-2.5">
              <Switch
                id="set-tray"
                checked={draft.closeToTray}
                onCheckedChange={(v) => set("closeToTray", v)}
              />
              <Label htmlFor="set-tray" className="text-[13px] font-normal">
                关闭窗口时最小化到托盘（否则直接退出并结束所有实例）
              </Label>
            </div>
          </div>

          <Field>
            <FieldLabel>数据目录（各版本安装在 versions/ 子目录下）</FieldLabel>
            <FieldDescription className="font-mono">{env?.versionsDir ?? "…"}</FieldDescription>
            <Button size="sm" variant="outline" className="w-fit" onClick={() => env && onReveal(env.versionsDir)}>
              <FolderOpen /> 打开数据目录
            </Button>
          </Field>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>取消</Button>
          <Button onClick={() => onSave(draft)}>保存</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
