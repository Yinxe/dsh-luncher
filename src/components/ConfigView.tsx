import { useCallback, useEffect, useRef, useState } from "react";
import { RotateCcw, Save } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import YamlEditor from "@/components/YamlEditor";
import YamlIssueBanner, { YamlDirtyBadge } from "@/components/YamlIssueBanner";
import { useSaveHotkey, useYamlIssues } from "@/lib/yaml-validate";
import { api } from "../api";

interface Props {
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

export default function ConfigView({ onToast }: Props) {
  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [missing, setMissing] = useState(false);
  const issues = useYamlIssues(draft);
  const canSave = dirty && issues.length === 0 && !busy;

  const reload = useCallback(async () => {
    try {
      setDraft(await api.readGlobalConfig());
      setMissing(false);
      setDirty(false);
    } catch (e) {
      setMissing(true);
      setDraft("");
      onToast("info", `尚未创建全局配置（${e}）`);
    }
  }, [onToast]);

  useEffect(() => {
    reload();
  }, [reload]);

  const busyRef = useRef(false);
  const save = useCallback(async () => {
    if (busyRef.current) return; // 在途守卫（AGENTS）：快捷键连发/连点不能并发写同一文件
    busyRef.current = true;
    setBusy(true);
    try {
      await api.writeGlobalConfig(draft);
      onToast("ok", "全局配置已保存（原文件已备份）");
      setDirty(false);
    } catch (e) {
      onToast("err", String(e));
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [draft, onToast]);

  const onKeyDown = useSaveHotkey(canSave, save);

  return (
    <div className="space-y-3" onKeyDown={onKeyDown}>
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="text-base font-semibold">全局配置</h2>
        <Badge variant="outline" className="font-mono">~/.dsh/settings.yaml</Badge>
        <YamlDirtyBadge dirty={dirty} issueCount={issues.length} />
        <span className="flex-1" />
        <Button
          size="sm"
          disabled={!canSave}
          title={issues.length > 0 ? "存在语法错误，修正后才能保存" : "Ctrl/Cmd+S"}
          onClick={save}
        >
          <Save /> 保存（自动备份）
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          title="丢弃本地修改，重新读取磁盘上的文件"
          onClick={reload}
        >
          <RotateCcw /> 还原
        </Button>
      </div>
      <div className="rounded-lg border border-amber-500/25 bg-amber-500/5 p-2.5 text-[11.5px] leading-relaxed text-muted-foreground">
        dsh 的全局配置（$DSH_HOME/settings.yaml）：按插件 id
        分节的自由配置，如 LLM providers、UI 偏好等。编辑保留注释，YAML
        语法有错误时行内标红并禁止保存（写入前后端还会做一次权威校验），原文件自动备份为
        *.starter-bak（仅保留一份，每次保存覆盖）。
      </div>
      {missing ? (
        <Card className="p-10 text-center text-muted-foreground">
          配置文件不存在——dsh 首次运行后会自动创建。
          <div className="mt-3">
            <Button size="sm" variant="outline" disabled={busy} onClick={() => { setDraft("# dsh 全局配置\n"); setDirty(true); setMissing(false); }}>
              创建初始内容
            </Button>
          </div>
        </Card>
      ) : (
        <>
          <YamlEditor value={draft} onChange={(v) => { setDraft(v); setDirty(true); }} />
          <YamlIssueBanner issues={issues} dirty={dirty} />
        </>
      )}
    </div>
  );
}
