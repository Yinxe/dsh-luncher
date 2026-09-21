import { useEffect, useRef, useState } from "react";
import { Activity, FolderOpen, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Sheet, SheetClose, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle,
} from "@/components/ui/sheet";
import {
  Field, FieldDescription, FieldLabel,
} from "@/components/ui/field";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { toast } from "sonner";
import { api } from "../api";
import type { ChannelProbe, EnvironmentInfo, GhAccel, GitHubRateLimit, Settings } from "../types";

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

/** Radix Select 不允许空字符串 value：用它代表「自动（最快）」 */
const AUTO = "__auto__";

/** 设置侧边抽屉：分区展示；ESC / 遮罩点击关闭 */
/** 通道内部名 → 展示名 */
const CHANNEL_LABEL: Record<string, string> = {
  "github-refs": "github.com",
  jsdelivr: "jsDelivr",
  raw: "raw.gh",
  "github-api": "api.github",
};

export default function SettingsDrawer({ open, initial, env, onSave, onClose, onReveal }: Props) {
  const [draft, setDraft] = useState<Settings>(initial);
  const [rate, setRate] = useState<GitHubRateLimit | null>(null);
  const [probes, setProbes] = useState<ChannelProbe[] | null>(null);
  const [checking, setChecking] = useState(false);
  const [accel, setAccel] = useState<GhAccel | null>(null);
  const [accelBusy, setAccelBusy] = useState(false);
  const [accelErr, setAccelErr] = useState<string | null>(null);
  // 测速请求序号：open 时的缓存读取与手动「重新测速」会并发，慢的那个不能覆盖快的结果
  const accelSeq = useRef(0);

  const runCheck = async () => {
    setChecking(true);
    try {
      setProbes(await api.checkChannels());
    } catch {
      setProbes(null);
    } finally {
      setChecking(false);
    }
  };

  /** 拉 GitHub520 hosts 并测速（force=true 绕过 6 小时缓存） */
  const runAccel = async (force: boolean) => {
    const my = ++accelSeq.current;
    setAccelBusy(true);
    setAccelErr(null);
    try {
      const r = await api.getGithubAccel(force);
      if (accelSeq.current === my) setAccel(r);
    } catch (e) {
      if (accelSeq.current === my) setAccelErr(String(e));
    } finally {
      if (accelSeq.current === my) setAccelBusy(false);
    }
  };

  useEffect(() => {
    if (!open) return;
    let alive = true;
    const my = ++accelSeq.current;
    api
      .getGithubRateLimit()
      .then((r) => {
        if (alive) setRate(r);
      })
      .catch(() => {
        if (alive) setRate(null);
      });
    api
      .getGithubAccel(false)
      .then((a) => {
        // 抽屉关闭、或期间用户点了「重新测速」（序号被顶掉）时丢弃这次缓存结果
        if (alive && accelSeq.current === my) setAccel(a);
      })
      .catch(() => {
        if (alive && accelSeq.current === my) setAccel(null);
      });
    return () => {
      alive = false;
    };
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
          <SheetDescription>保存到 ~/.dsh-starter/settings.json</SheetDescription>
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
                  插件安装与更新检测走 git / registry，<strong>不填也能正常用</strong>；
                  也可用 GITHUB_TOKEN / GH_TOKEN 环境变量代替。{rlText}
                </span>
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel>通道自检</FieldLabel>
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  disabled={checking}
                  onClick={() => void runCheck()}
                >
                  <Activity /> {checking ? "探测中…" : "检测各通道"}
                </Button>
                <span className="text-[10.5px] text-muted-foreground">
                  并发探测 github.com / jsDelivr / raw / api，看当时哪条通、多快
                </span>
              </div>
              {probes && (
                <div className="mt-1.5 space-y-1">
                  {probes.map((p) => (
                    <div key={p.name} className="flex items-center gap-2 text-[11px]">
                      <span className="w-[104px] shrink-0 font-mono">{CHANNEL_LABEL[p.name] ?? p.name}</span>
                      <Badge variant={p.ok ? "success" : "destructive"}>
                        {p.ok ? "可用" : "不可用"}
                      </Badge>
                      <span className="font-mono text-muted-foreground">{p.ms}ms</span>
                      <span className="min-w-0 flex-1 truncate text-muted-foreground" title={p.detail ?? ""}>
                        {p.detail ?? ""}
                      </span>
                    </div>
                  ))}
                </div>
              )}
              <FieldDescription>
                探测与更新检测优先走<strong>免额度</strong>通道（github.com refs / jsDelivr），
                连续失败的通道会被临时跳过 5 分钟，避免每次都白等一个超时；api 只用于元数据。
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel>GitHub 加速</FieldLabel>
              <SwitchRow
                id="set-ghaccel"
                checked={draft.githubAccel}
                onChange={(v) => set("githubAccel", v)}
                label="把 github 链接拼到测速最快的代理前缀上"
                hint="内置一组常用前缀代理（gh-proxy.com、gh.xxooo.cf、gh.dpik.top…）。首次使用会各测一次：下载测速用小文件 GET，git 能力用一次真实的浅克隆（有些代理只放行文件下载，clone 会 403，所以分开测），结果缓存 6 小时。用法就是「前缀 + 原 github 链接」；git 走 url.<前缀>.insteadOf，已有克隆的 git pull 也生效，releases / raw 直链在安装时直接改写。"
              />
              <div className="flex flex-wrap items-end gap-2">
                <Field className="min-w-[240px] flex-1">
                  <FieldLabel htmlFor="set-ghproxy">使用哪个前缀</FieldLabel>
                  <Select value={draft.githubProxy || AUTO} onValueChange={(v) => set("githubProxy", v === AUTO ? "" : v)}>
                    <SelectTrigger id="set-ghproxy" className="w-full font-mono text-xs">
                      <SelectValue placeholder="自动（最快的一个）" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={AUTO} className="text-muted-foreground">自动（最快的一个）</SelectItem>
                      {(accel?.nodes ?? []).map((n) => (
                        <SelectItem key={n.prefix} value={n.prefix} className="font-mono text-xs">
                          {n.prefix}（{n.ms}ms）
                        </SelectItem>
                      ))}
                      {draft.githubProxy &&
                        !(accel?.nodes ?? []).some((n) => n.prefix === draft.githubProxy) && (
                          <SelectItem value={draft.githubProxy} className="font-mono text-xs">
                            {draft.githubProxy}（未测速）
                          </SelectItem>
                        )}
                    </SelectContent>
                  </Select>
                </Field>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  className="h-8"
                  disabled={accelBusy}
                  onClick={() => void runAccel(true)}
                >
                  <Zap /> {accelBusy ? "测速中…" : "重新测速"}
                </Button>
              </div>
              <Field>
                <FieldLabel htmlFor="set-ghproxy-extra">额外代理前缀（可选）</FieldLabel>
                <Textarea
                  id="set-ghproxy-extra"
                  rows={2}
                  className="min-h-14 font-mono text-xs"
                  placeholder="自建/其它前缀，一行一个，如 https://gh.example.com/"
                  value={draft.githubProxyExtra}
                  onChange={(e) => set("githubProxyExtra", e.target.value)}
                />
                <FieldDescription>
                  会与内置清单一起参与测速；用法就是「前缀 + 原 github 链接」。
                </FieldDescription>
              </Field>
              {accelErr && (
                <p className="text-[10.5px] leading-relaxed text-amber-600 dark:text-amber-400">{accelErr}</p>
              )}
              {accel && accel.nodes.length > 0 && (
                <div className="mt-1.5 space-y-1">
                  {accel.nodes.map((n) => (
                    <div key={n.prefix} className="flex items-center gap-2 text-[11px]">
                      <span className="min-w-0 flex-1 truncate font-mono">{n.prefix}</span>
                      <Badge
                        variant={n.prefix === (draft.githubProxy || accel.nodes[0].prefix) ? "success" : "outline"}
                        className="font-mono text-[10px]"
                      >
                        下载 {n.ms}ms
                      </Badge>
                      <Badge variant={n.gitMs == null ? "warning" : "outline"} className="font-mono text-[10px]">
                        {n.gitMs == null ? "不支持 git" : `git ${n.gitMs}ms`}
                      </Badge>
                    </div>
                  ))}
                  <p className="pt-0.5 text-[10.5px] text-muted-foreground">
                    来源 {accel.source}
                    {accel.updatedAt > 0 && ` · ${new Date(accel.updatedAt * 1000).toLocaleString()}`}
                    {accel.cached && "（缓存）"}
                  </p>
                </div>
              )}
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
              <FieldDescription>
                可填相对家目录的写法（如 <span className="font-mono">~/.bun/bin/node</span>、
                <span className="font-mono">~/.nvm/versions/node/v24/bin/node</span>），启动器会自行展开
                <span className="font-mono"> ~ </span>
                —— Windows 的 cmd 不认 <span className="font-mono">~</span>，在终端里验证时请用
                PowerShell 或 <span className="font-mono">%USERPROFILE%</span>
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
                placeholder="https://example.com/dsh-starter/latest.json"
                onChange={(e) => set("updateManifestUrl", e.target.value)}
              />
              <FieldDescription>
                返回 {`{version, notes, url}`} 的 JSON；留空则改用内置 Tauri updater 检查（后者支持应用内「下载并安装」）
              </FieldDescription>
            </Field>
            <Field>
              <FieldLabel>更新下载源</FieldLabel>
              <Select
                value={draft.updateSource === "github" ? "github" : "r2"}
                onValueChange={(v) => set("updateSource", v)}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="r2">Cloudflare R2（推荐 · 国内更快）</SelectItem>
                  <SelectItem value="github">GitHub 官方源</SelectItem>
                </SelectContent>
              </Select>
              <FieldDescription>
                默认走自建 R2 源（CDN 加速、无需翻墙）；R2 暂时不可用会自动回落到 GitHub。
                地址写死在程序里、不提供手填 —— 填错的后果是「更新不了」且无从排查。
                若 R2 上的清单没跟上最新版本，可临时切到 GitHub 源。
              </FieldDescription>
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
            <SwitchRow
              id="set-auto-install-update"
              checked={draft.autoInstallUpdate}
              onChange={(v) => set("autoInstallUpdate", v)}
              label="发现新版本后自动下载安装并重启"
              hint="默认关闭：只提示、由你决定是否升级。开启后仅在安装无需提权的形态生效（AppImage / Windows / macOS）；deb、rpm 必然弹系统密码框，因此仍只提示"
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
            <Field>
              <FieldLabel>日志目录</FieldLabel>
              <div className="flex items-center gap-2">
                <span
                  className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-muted-foreground"
                  title={env?.logsDir}
                >
                  {env?.logsDir ?? "…"}
                </span>
                <Button size="sm" variant="outline" onClick={() => env && onReveal(env.logsDir)}>
                  <FolderOpen /> 打开
                </Button>
              </div>
              <FieldDescription>
                按子系统分类：app / instance / install / runtime / plugin / profile / network / ui /
                panic；遇到问题把这里的文件发来即可定位
              </FieldDescription>
              <div className="pt-1">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={async () => {
                    try {
                      const path = await api.exportDiagnostics();
                      toast.success("诊断包已生成", {
                        description: `${path.split(/[\\/]/).pop()} —— 已帮你打开日志目录`,
                      });
                      onReveal(env?.logsDir ?? path);
                    } catch (e) {
                      toast.error(`生成诊断包失败: ${e}`);
                    }
                  }}
                >
                  <Activity /> 生成诊断包
                </Button>
              </div>
              <FieldDescription>
                一键汇总环境、设置（**凭据已脱敏**）与全部日志，报 bug 时发这一个 txt 就够
              </FieldDescription>
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
