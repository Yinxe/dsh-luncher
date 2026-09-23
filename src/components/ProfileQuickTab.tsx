import { useCallback, useEffect, useRef, useState } from "react";
import { Loader2, Save } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { api } from "../api";
import type { WebQuickConfig, WebQuickConfigInput } from "../types";

interface Props {
  profile: string;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

interface QuickForm {
  host: string;
  port: string;
  openBrowser: boolean;
  surfaceContext: boolean;
  cookieMaxAgeDays: string;
}

/** 条目缺失时的预填值 = 插件/bundle 层的生效默认值（保存后整块接管） */
function formFromQuick(q: WebQuickConfig | null): QuickForm {
  return {
    host: q?.host ?? "127.0.0.1",
    port: String(q?.port ?? 3080),
    openBrowser: q?.openBrowser ?? true,
    surfaceContext: q?.surfaceContext ?? true,
    cookieMaxAgeDays: String(q?.cookieMaxAgeDays ?? 30),
  };
}

/** 条目接管状态徽标：已在 patch 中 = 已接管；否则保存后写入 */
function PresentBadge({ present }: { present: boolean }) {
  return present ? (
    <Badge variant="success" className="text-[10px]">已接管</Badge>
  ) : (
    <Badge variant="outline" className="text-[10px]">未配置 · 保存后写入</Badge>
  );
}

/**
 * 快捷配置 Tab（仅 Web target）：整块接管 webserver / web-runtime / connection 三个
 * patch 条目；patch 会整体替换该行 config，所以每块键成套写全，trustedHosts 固定
 * !!js 联动 webStartup → webRuntime 信任链。基于 cordis.patch.yml，新旧版本通用。
 */
export default function ProfileQuickTab({ profile, onToast }: Props) {
  const [loading, setLoading] = useState(true);
  const [quick, setQuick] = useState<WebQuickConfig | null>(null);
  const [form, setForm] = useState<QuickForm>(formFromQuick(null));
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const q = await api.getWebQuickConfig(profile).catch(() => null);
      setQuick(q);
      setForm(formFromQuick(q));
    } finally {
      setLoading(false);
    }
  }, [profile]);

  useEffect(() => {
    load();
  }, [load]);

  const save = useCallback(async () => {
    const host = form.host.trim();
    const port = Number(form.port);
    const days = Number(form.cookieMaxAgeDays);
    if (!host) return onToast("err", "host 不能为空");
    if (!Number.isInteger(port) || port < 1 || port > 65535)
      return onToast("err", "port 需为 1-65535 的整数");
    if (!Number.isInteger(days) || days < 1)
      return onToast("err", "cookie 有效期需为 ≥ 1 的整数（天）");
    const input: WebQuickConfigInput = {
      host,
      port,
      openBrowser: form.openBrowser,
      surfaceContext: form.surfaceContext,
      cookieMaxAgeDays: days,
    };
    if (savingRef.current) return; // 在途守卫（AGENTS）
    savingRef.current = true;
    setSaving(true);
    try {
      await api.setWebQuickConfig(profile, input);
      onToast("ok", "快捷配置已写入 cordis.patch.yml（原文件已备份；live 模式即时生效，startup 模式需重启实例）");
      await load();
    } catch (e) {
      onToast("err", String(e));
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [profile, form, onToast, load]);

  if (loading) {
    return (
      <div className="flex items-center justify-center gap-2 py-8 text-xs text-muted-foreground">
        <Loader2 className="h-3.5 w-3.5 animate-spin" /> 读取快捷配置…
      </div>
    );
  }

  return (
    <div className="space-y-3">
      <p className="text-[11px] leading-relaxed text-muted-foreground">
        保存后启动器在 <span className="font-mono">cordis.patch.yml</span> 中整块接管
        webserver / web-runtime / connection 三个条目（自动备份）。patch 条目会整体替换该行
        config，所以键必须成套写全——漏写会退回 schema 默认值（例如 --no-open
        失效）；两条 <span className="font-mono">trustedHosts</span> 固定用
        <span className="font-mono"> !!js </span>表达式联动 webStartup → webRuntime 信任链
        （局域网信任随绑定网卡与 --trusted-host 自动推导，无需手改）。
      </p>

      {/* webserver：Web 服务器监听 */}
      <div className="space-y-2.5 rounded-lg border border-border bg-background/50 p-3">
        <div className="flex items-center gap-2">
          <span className="font-mono text-[12.5px] font-semibold">webserver</span>
          <span className="text-[11px] text-muted-foreground">Web 服务器监听</span>
          <PresentBadge present={quick?.webserverPresent ?? false} />
        </div>
        <div className="flex flex-wrap items-end gap-3">
          <div className="space-y-1">
            <Label htmlFor={`${profile}-host`} className="text-[11px] text-muted-foreground">host</Label>
            <Input
              id={`${profile}-host`}
              className="h-7 w-44 font-mono text-xs"
              value={form.host}
              placeholder="127.0.0.1 或 0.0.0.0"
              onChange={(e) => setForm((f) => ({ ...f, host: e.target.value }))}
            />
          </div>
          <div className="space-y-1">
            <Label htmlFor={`${profile}-port`} className="text-[11px] text-muted-foreground">port</Label>
            <Input
              id={`${profile}-port`}
              className="h-7 w-24 font-mono text-xs"
              value={form.port}
              placeholder="3080"
              onChange={(e) => setForm((f) => ({ ...f, port: e.target.value }))}
            />
          </div>
          <span className="text-[10.5px] leading-snug text-muted-foreground">
            0.0.0.0 = 允许局域网访问（请仅在可信网络使用）
          </span>
        </div>
      </div>

      {/* web-runtime：启动行为 */}
      <div className="space-y-2.5 rounded-lg border border-border bg-background/50 p-3">
        <div className="flex items-center gap-2">
          <span className="font-mono text-[12.5px] font-semibold">web-runtime</span>
          <span className="text-[11px] text-muted-foreground">启动行为</span>
          <PresentBadge present={quick?.webRuntimePresent ?? false} />
        </div>
        {([
          ["openBrowser", "启动后自动打开浏览器", "关闭后等价于每次都加 --no-open，且 --open 无法反向覆盖"],
          ["surfaceContext", "Web GUI 上下文告知", "默认开：模型知道自己在 GUI 里、shell 有 $DSH_WEB_URL；仅影响 Web 会话"],
        ] as const).map(([key, label, desc]) => (
          <div key={key} className="flex items-center gap-3">
            <Switch
              id={`${profile}-${key}`}
              checked={form[key]}
              onCheckedChange={(v) => setForm((f) => ({ ...f, [key]: v }))}
            />
            <Label htmlFor={`${profile}-${key}`} className="text-xs">{label}</Label>
            <span className="text-[10.5px] text-muted-foreground">{desc}</span>
          </div>
        ))}
        {/* printUrl 不开放配置：启动器从启动日志 `dsh web: <url>` 识别访问地址，恒为 true */}
        <div className="flex items-center gap-3 opacity-80">
          <Switch id={`${profile}-printUrl`} checked disabled />
          <Label htmlFor={`${profile}-printUrl`} className="text-xs">终端打印访问地址</Label>
          <span className="text-[10.5px] text-muted-foreground">
            固定开启，不可关闭——启动器依赖启动日志识别访问地址
          </span>
        </div>
      </div>

      {/* connection：会话凭证 */}
      <div className="space-y-2.5 rounded-lg border border-border bg-background/50 p-3">
        <div className="flex items-center gap-2">
          <span className="font-mono text-[12.5px] font-semibold">connection</span>
          <span className="text-[11px] text-muted-foreground">浏览器会话凭证</span>
          <PresentBadge present={quick?.connectionPresent ?? false} />
        </div>
        <div className="flex flex-wrap items-end gap-3">
          <div className="space-y-1">
            <Label htmlFor={`${profile}-days`} className="text-[11px] text-muted-foreground">
              cookie 有效期（天）
            </Label>
            <Input
              id={`${profile}-days`}
              className="h-7 w-28 font-mono text-xs"
              value={form.cookieMaxAgeDays}
              placeholder="30"
              onChange={(e) => setForm((f) => ({ ...f, cookieMaxAgeDays: e.target.value }))}
            />
          </div>
          <span className="text-[10.5px] leading-snug text-muted-foreground">
            插件默认 30 天；填 36500 ≈ 100 年实现「永久」（实际受浏览器 400 天上限钳制）
          </span>
        </div>
      </div>

      <div className="flex items-center gap-2">
        <Button size="sm" disabled={saving} onClick={save}>
          {saving ? <Loader2 className="animate-spin" /> : <Save />} 保存快捷配置（自动备份）
        </Button>
        <span className="text-[10.5px] text-muted-foreground">
          patchReload = live 时保存即时生效；否则需重启实例
        </span>
      </div>
    </div>
  );
}
