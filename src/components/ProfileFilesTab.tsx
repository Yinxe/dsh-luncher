import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { ArchiveRestore, Loader2, RotateCcw, Save } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import YamlEditor from "@/components/YamlEditor";
import YamlIssueBanner, { YamlDirtyBadge } from "@/components/YamlIssueBanner";
import { useSaveHotkey, useYamlIssues } from "@/lib/yaml-validate";
import { api } from "../api";
import type { ProfileConfigMode } from "../types";

interface Props {
  profile: string;
  mode: ProfileConfigMode;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

/** cordis.patch.yml / settings.yaml 的编辑器骨架（YAML 校验 + 自动备份保存） */
function EditorBody({
  value, onChange, onKeyDown, dirty, canSave, saving, onSave, onReload, footer,
}: {
  value: string;
  onChange: (v: string) => void;
  onKeyDown: (e: KeyboardEvent) => void;
  dirty: boolean;
  canSave: boolean;
  saving: boolean;
  onSave: () => void;
  onReload: () => void;
  footer: string;
}) {
  const issues = useYamlIssues(value);
  return (
    <div className="space-y-2" onKeyDown={onKeyDown}>
      <div className="flex flex-wrap items-center gap-2">
        <YamlDirtyBadge dirty={dirty} issueCount={issues.length} />
        <span className="flex-1" />
        <Button
          size="sm"
          disabled={!canSave}
          title={issues.length > 0 ? "存在语法错误，修正后才能保存" : "Ctrl/Cmd+S"}
          onClick={onSave}
        >
          {saving ? <Loader2 className="animate-spin" /> : <Save />} 保存（自动备份）
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={saving}
          title="丢弃本地修改，重新读取磁盘上的文件"
          onClick={onReload}
        >
          <RotateCcw /> 还原
        </Button>
      </div>
      <YamlEditor value={value} onChange={onChange} />
      <YamlIssueBanner issues={issues} dirty={dirty} />
      <p className="text-[11px] text-muted-foreground">{footer}</p>
    </div>
  );
}

/** 只读查看被 0.1.7 导入存档的旧全局配置（settings.yaml.imported） */
function ImportedSettingsDialog({
  open, onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const [content, setContent] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setContent(null);
    setErr(null);
    api.readImportedSettings().then(setContent).catch((e) => setErr(String(e)));
  }, [open]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="grid-cols-[minmax(0,1fr)] sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <ArchiveRestore className="h-4 w-4" /> settings.yaml.imported（只读）
          </DialogTitle>
          <DialogDescription>
            旧全局配置在首次以 0.1.7+ 启动 profile 时被一次性导入该 profile 的
            cordis.patch.yml，原文件改名为 settings.yaml.imported。
            <span className="font-medium">注意：这里只剩被拒绝、没导入成功的节，不是完整存档</span>
            ——完整快照在启动器每次保存前留下的 settings.starter-bak；
            缺失的节可从这里手动搬进 patch 条目。
          </DialogDescription>
        </DialogHeader>
        {err ? (
          <Card className="p-6 text-center text-xs text-muted-foreground">{err}</Card>
        ) : content == null ? (
          <div className="flex h-40 items-center justify-center gap-2 text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" /> 读取中…
          </div>
        ) : (
          <div className="max-h-[60vh] overflow-y-auto">
            <YamlEditor value={content} readOnly onChange={() => undefined} />
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

/**
 * 「配置文件」Tab：
 * - patch 模式（≥0.1.7）：编辑该 profile 的 cordis.patch.yml；旧全局配置只读查看
 * - legacy 模式（<0.1.7）：默认编辑全局 ~/.dsh/settings.yaml（可切到该 profile 的 patch），
 *   并提示全局配置能力将随 0.1.6 之前的支持一并弃用
 */
export default function ProfileFilesTab({ profile, mode, onToast }: Props) {
  const isPatch = mode.mode === "patch";
  // legacy 模式默认落在全局 settings.yaml——旧版 dsh 只认这个文件
  const [fileKey, setFileKey] = useState<"settings" | "patch">(isPatch ? "patch" : "settings");
  useEffect(() => {
    setFileKey(isPatch ? "patch" : "settings");
  }, [profile, isPatch]);

  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const [missing, setMissing] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const [importedOpen, setImportedOpen] = useState(false);
  const issues = useYamlIssues(draft);
  const canSave = !loading && dirty && issues.length === 0 && !saving;

  const load = useCallback(async () => {
    setLoading(true);
    try {
      if (fileKey === "settings") {
        setDraft(await api.readGlobalConfig());
      } else {
        setDraft(await api.readProfileFile(profile, "cordis.patch.yml").catch(() => ""));
      }
      setDirty(false);
      setMissing(false);
    } catch (e) {
      setDraft("");
      setMissing(true);
      onToast("info", `配置文件还不存在（${e}）`);
    } finally {
      setLoading(false);
    }
  }, [profile, fileKey, onToast]);

  useEffect(() => {
    load();
  }, [load]);

  const save = useCallback(async () => {
    if (savingRef.current) return; // 在途守卫（AGENTS）：快捷键连发/连点不能并发写同一文件
    savingRef.current = true;
    setSaving(true);
    try {
      if (fileKey === "settings") await api.writeGlobalConfig(draft);
      else await api.writeProfileFile(profile, "cordis.patch.yml", draft);
      onToast("ok", `${fileKey === "settings" ? "全局配置" : "cordis.patch.yml"} 已保存（原文件已备份）`);
      setDirty(false);
      setMissing(false);
    } catch (e) {
      onToast("err", String(e));
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [fileKey, profile, draft, onToast]);

  const onKeyDown = useSaveHotkey(canSave, save);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        {isPatch ? (
          <Badge variant="outline" className="font-mono" title="该 profile 绑定 0.1.7+，配置全部在这里">
            profiles/{profile}/cordis.patch.yml
          </Badge>
        ) : (
          <>
            <span className="text-xs text-muted-foreground">编辑文件</span>
            <Select value={fileKey} onValueChange={(v) => setFileKey(v as "settings" | "patch")}>
              <SelectTrigger className="h-8 w-64 font-mono text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="settings" className="font-mono text-xs">~/.dsh/settings.yaml（全局）</SelectItem>
                <SelectItem value="patch" className="font-mono text-xs">profiles/{profile}/cordis.patch.yml</SelectItem>
              </SelectContent>
            </Select>
          </>
        )}
        <span className="flex-1" />
        {isPatch && mode.imported && (
          <Button size="sm" variant="outline" onClick={() => setImportedOpen(true)} title="只读查看被导入存档的旧全局配置">
            <ArchiveRestore /> 旧全局配置（imported）
          </Button>
        )}
      </div>

      {isPatch ? (
        <div className="rounded-lg border border-amber-500/25 bg-amber-500/5 p-2.5 text-[11.5px] leading-relaxed text-muted-foreground">
          0.1.7 起该 profile 的全部设置按 <span className="font-mono text-foreground">cordis.patch.yml</span>{" "}
          隔离保存（顶层 <span className="font-mono">- id: …</span> 条目，含模型、插件与插件配置）。
          带 <span className="font-mono">dsh-starter</span> 标记注释的条目块由启动器管理，整块覆盖；
          其余内容编辑时逐字节保留。旧全局配置已导入存档，见右侧「旧全局配置（imported）」。
        </div>
      ) : (
        <div className="rounded-lg border border-amber-500/25 bg-amber-500/5 p-2.5 text-[11.5px] leading-relaxed text-muted-foreground">
          该 profile 绑定 dsh <span className="font-mono text-foreground">{mode.version || "?"}</span>（&lt; 0.1.7）：
          旧版只读全局 <span className="font-mono text-foreground">~/.dsh/settings.yaml</span>
          （若显示为 settings.yaml.imported，启动时会自动还原）。全局配置这一能力
          <strong className="font-medium text-foreground">将随 0.1.6 之前的版本支持一并弃用</strong>
          ——升级到 0.1.7+ 后配置按 profile 独立，不再共享这一份。
        </div>
      )}

      {loading ? (
        <div className="flex items-center justify-center gap-2 py-8 text-xs text-muted-foreground">
          <Loader2 className="h-3.5 w-3.5 animate-spin" /> 读取配置…
        </div>
      ) : missing ? (
        <Card className="p-10 text-center text-muted-foreground">
          {fileKey === "settings" ? "全局配置" : "cordis.patch.yml"} 不存在——dsh 首次运行后会自动创建。
          <div className="mt-3">
            <Button
              size="sm"
              variant="outline"
              disabled={saving}
              onClick={() => { setDraft("# dsh 配置\n"); setDirty(true); setMissing(false); }}
            >
              创建初始内容
            </Button>
          </div>
        </Card>
      ) : (
        <EditorBody
          value={draft}
          onChange={(v) => { setDraft(v); setDirty(true); }}
          onKeyDown={onKeyDown}
          dirty={dirty}
          canSave={canSave}
          saving={saving}
          onSave={save}
          onReload={load}
          footer={
            fileKey === "settings"
              ? "dsh 全局配置（$DSH_HOME/settings.yaml）：编辑保留注释，YAML 语法错误时行内标红并禁止保存（写入前后端还会做一次权威校验），原文件自动备份为 *.starter-bak（仅保留一份，每次保存覆盖）。"
              : "用户 patch 层（id 覆盖 / disabled / insert；支持 !!js 表达式）。编辑保留全部注释，语法错误时行内标红并禁止保存，保存前自动备份；live 模式下 dsh 热重载。注意 patch 会整体替换目标条目的 config，键需成套写全。"
          }
        />
      )}

      <ImportedSettingsDialog open={importedOpen} onOpenChange={setImportedOpen} />
    </div>
  );
}
