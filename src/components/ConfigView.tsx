import { useCallback, useEffect, useState } from "react";
import { RotateCcw, Save } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import YamlEditor from "@/components/YamlEditor";
import { api } from "../api";

interface Props {
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

export default function ConfigView({ onToast }: Props) {
  const [draft, setDraft] = useState("");
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [missing, setMissing] = useState(false);

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

  const save = useCallback(async () => {
    setBusy(true);
    try {
      await api.writeGlobalConfig(draft);
      onToast("ok", "全局配置已保存（原文件已备份）");
      setDirty(false);
    } catch (e) {
      onToast("err", String(e));
    } finally {
      setBusy(false);
    }
  }, [draft, onToast]);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="text-base font-semibold">全局配置</h2>
        <Badge variant="outline" className="font-mono">~/.dsh/settings.yaml</Badge>
        {dirty && <Badge variant="warning">未保存</Badge>}
        <span className="flex-1" />
        <Button size="sm" disabled={busy || !dirty} onClick={save}>
          <Save /> 保存（自动备份）
        </Button>
        <Button size="sm" variant="outline" disabled={busy || !dirty} onClick={reload}>
          <RotateCcw /> 还原
        </Button>
      </div>
      <div className="rounded-lg border border-amber-500/25 bg-amber-500/5 p-2.5 text-[11.5px] leading-relaxed text-muted-foreground">
        dsh 的全局配置（$DSH_HOME/settings.yaml）：按插件 id
        分节的自由配置，如 LLM providers、UI 偏好等。编辑保留注释，保存前做 YAML
        语法校验，原文件自动备份为 *.starter-bak（仅保留一份，每次保存覆盖）。
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
        <YamlEditor value={draft} onChange={(v) => { setDraft(v); setDirty(true); }} />
      )}
    </div>
  );
}
