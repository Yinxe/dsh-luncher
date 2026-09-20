import { useEffect, useState } from "react";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Sheet, SheetClose, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle,
} from "@/components/ui/sheet";
import {
  Field, FieldDescription, FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { api } from "../api";
import type { EnvironmentInfo, GitHubRateLimit, Settings } from "../types";

interface Props {
  open: boolean;
  initial: Settings;
  env: EnvironmentInfo | null;
  onSave: (s: Settings) => void;
  onClose: () => void;
  onReveal: (path: string) => void;
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="space-y-3.5">
      <div className="eyebrow">{title}</div>
      {children}
    </section>
  );
}

function SwitchRow({
  id, checked, onChange, label, hint,
}: { id: string; checked: boolean; onChange: (v: boolean) => void; label: string; hint?: string }) {
  return (
    <div className="space-y-1">
      <div className="flex items-center gap-2.5">
        <Switch id={id} checked={checked} onCheckedChange={onChange} />
        <Label htmlFor={id} className="text-[13px] font-normal">{label}</Label>
      </div>
      {hint && <div className="pl-9 text-[11px] leading-relaxed text-muted-foreground">{hint}</div>}
    </div>
  );
}

/** 设置侧边抽屉：分区展示；ESC / 遮罩点击关闭 */
export default function SettingsDrawer({ open, initial, env, onSave, onClose, onReveal }: Props) {
  const [draft, setDraft] = useState<Settings>(initial);
  const [rate, setRate] = useState<GitHubRateLimit | null>(null);

  useEffect(() => {
    if (!open) return;
    api.getGithubRateLimit().then(setRate).catch(() => setRate(null));
  }, [open]);

  // 每次打开时以当前设置重置草稿（取消即丢弃修改）
  useEffect(() => {
    if (open) setDraft(initial);
  }, [open, initial]);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) =>
    setDraft((d) => ({ ...d, [k]: v }));

  const rlText = rate
    ? rate.remaining == null
      ? ""
      : rate.exhausted
      ? ` 当前额度已用尽（${rate.remaining}/${rate.limit ?? 60}），${
          rate.reset ? new Date(rate.reset * 1000).toLocaleTimeString() : "稍后"
        } 重置。`
      : ` 当前剩余 ${rate.remaining}/${rate.limit ?? 60}${
          rate.reset ? `，${new Date(rate.reset * 1000).toLocaleTimeString()} 重置` : ""
        }。`
    : "";

  return (
    <Sheet open={open} onOpenChange={(o) => !o && onClose()}>
      <SheetContent
        side="right"
        className="w-full max-w-[560px] gap-0 border-l border-border bg-card p-0"
      >
        <SheetHeader className="border-b border-border">
          <SheetTitle>设置</SheetTitle>
          <SheetDescription>保存到 ~/.dsh-launcher/settings.json</SheetDescription>
        </SheetHeader>

        <div className="min-h-0 flex-1 space-y-6 overflow-y-auto px-5 py-5">
          <Section title="网络与镜像">
            <Field>
              <FieldLabel htmlFor="set-registry">npm Registry</FieldLabel>
              <Input
                id="set-registry"
                className="font-mono"
                value={draft.registry}
                placeholder="https://registry.npmjs.org"
                onChange={(e) => set("registry", e.target.value)}
              />
              <FieldDescription>拉取版本列表与安装源；国内可用 registry.npmmirror.com</FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor="set-ghtoken">GitHub Token（可选）</FieldLabel>
              <Input
                id="set-ghtoken"
                type="password"
                className="font-mono"
                value={draft.githubToken}
                placeholder="ghp_… / github_pat_…（留空即匿名）"
                onChange={(e) => set("githubToken", e.target.value)}
              />
              <FieldDescription>
                只用于提高 api.github.com 额度：匿名 60 次/小时 → 带 token 5000 次/小时。
                <span className="block">
                  仓库探测与更新检测走 jsDelivr / git 免额度通道，<strong>不填也能正常用</strong>；
                  也可用 GITHUB_TOKEN / GH_TOKEN 环境变量代替。{rlText}
                </span>
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor="set-mirror">Node 运行时镜像站</FieldLabel>
              <Input
                id="set-mirror"
                className="font-mono"
                value={draft.nodeMirror}
                placeholder="https://npmmirror.com/mirrors/node"
                onChange={(e) => set("nodeMirror", e.target.value)}
              />
              <FieldDescription>「一键安装 Node」下载用；国内推荐 npmmirror / 阿里云镜像</FieldDescription>
            </Field>
          </Section>

          <Section title="Node 运行时">
            <Field>
              <FieldLabel htmlFor="set-nodepath">Node 路径</FieldLabel>
              <Input
                id="set-nodepath"
                className="font-mono"
                value={draft.nodePath}
                placeholder="留空自动探测（PATH / 内置运行时 / nvm）"
                onChange={(e) => set("nodePath", e.target.value)}
              />
              <FieldDescription className="truncate">
                {env?.nodePath
                  ? <span className="font-mono" title={`${env.nodePath} (v${env.node})`}>探测到 {env.nodePath} (v{env.node})</span>
                  : "dsh 依赖 Node.js，留空将自动探测"}
              </FieldDescription>
            </Field>
          </Section>

          <Section title="dsh 启动">
            <div className="grid gap-3 sm:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="set-terminal">终端模拟器</FieldLabel>
                <Input
                  id="set-terminal"
                  value={draft.terminal}
                  placeholder="auto"
                  onChange={(e) => set("terminal", e.target.value)}
                />
                <FieldDescription>仅独立窗口模式，留空自动选择</FieldDescription>
              </Field>
              <Field>
                <FieldLabel htmlFor="set-args">默认附加参数</FieldLabel>
                <Input
                  id="set-args"
                  className="font-mono"
                  value={draft.defaultArgs}
                  placeholder="例如 --preset qqbot"
                  onChange={(e) => set("defaultArgs", e.target.value)}
                />
                <FieldDescription>启动 dsh 时追加</FieldDescription>
              </Field>
            </div>
          </Section>

          <Section title="更新">
            <Field>
              <FieldLabel htmlFor="set-manifest">更新清单地址</FieldLabel>
              <Input
                id="set-manifest"
                className="font-mono"
                type="text"
                value={draft.updateManifestUrl}
                placeholder="https://example.com/dsh-launcher/latest.json"
                onChange={(e) => set("updateManifestUrl", e.target.value)}
              />
              <FieldDescription>返回 {`{version, notes, url}`} 的 JSON；留空则不检查启动器更新</FieldDescription>
            </Field>
            <SwitchRow
              id="set-auto-versions"
              checked={draft.autoCheckVersions}
              onChange={(v) => set("autoCheckVersions", v)}
              label="启动时自动刷新官方版本列表"
            />
            <SwitchRow
              id="set-auto-update"
              checked={draft.autoCheckUpdate}
              onChange={(v) => set("autoCheckUpdate", v)}
              label="启动时自动检查启动器更新"
            />
          </Section>

          <Section title="窗口与数据">
            <SwitchRow
              id="set-tray"
              checked={draft.closeToTray}
              onChange={(v) => set("closeToTray", v)}
              label="关闭窗口时最小化到托盘"
              hint="关闭时否则将直接退出并结束所有内嵌 dsh 实例"
            />
            <Field>
              <FieldLabel>数据目录</FieldLabel>
              <div className="flex items-center gap-2">
                <span
                  className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-muted-foreground"
                  title={env?.versionsDir}
                >
                  {env?.versionsDir ?? "…"}
                </span>
                <Button size="sm" variant="outline" onClick={() => env && onReveal(env.versionsDir)}>
                  <FolderOpen /> 打开
                </Button>
              </div>
              <FieldDescription>各版本安装在 versions/ 子目录下</FieldDescription>
            </Field>
          </Section>
        </div>

        <SheetFooter className="flex-row justify-end gap-2 border-t border-border">
          <SheetClose asChild>
            <Button variant="outline">取消</Button>
          </SheetClose>
          <Button onClick={() => onSave(draft)}>保存</Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
