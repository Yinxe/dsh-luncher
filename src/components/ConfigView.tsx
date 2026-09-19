import { useCallback, useEffect, useState } from "react";
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
    <div className="page">
      <div className="page-head">
        <h2>全局配置</h2>
        <span className="reload-hint mono">~/.dsh/settings.yaml</span>
        <span style={{ flex: 1 }} />
        <button className="sm primary" disabled={busy || !dirty} onClick={save}>
          {dirty ? "保存（自动备份）" : "已保存"}
        </button>
      </div>
      <div className="inst-tip">
        这是 dsh 的全局配置（$DSH_HOME/settings.yaml）：按插件 id
        分节的自由配置，如 LLM providers、UI 偏好等。编辑保留注释，保存前做 YAML
        语法校验，原文件自动备份为 *.launcher-bak-*。
      </div>
      {missing ? (
        <div className="page-empty">
          配置文件不存在——dsh 首次运行后会自动创建；也可以直接点保存创建一份。
          <div style={{ marginTop: 10 }}>
            <button
              className="sm primary"
              disabled={busy}
              onClick={() => setDraft("# dsh 全局配置\n")}
            >
              创建初始内容
            </button>
          </div>
        </div>
      ) : (
        <textarea
          className="code-editor tall"
          spellCheck={false}
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value);
            setDirty(true);
          }}
        />
      )}
    </div>
  );
}
