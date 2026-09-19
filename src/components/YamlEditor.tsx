import { useMemo } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { yaml } from "@codemirror/lang-yaml";
import { linter, type Diagnostic } from "@codemirror/lint";
import { parseDocument } from "yaml";
import { useTheme } from "@/lib/theme";

/** YAML 语法实时校验（专业配置编辑器：错误行内标红） */
function yamlLinter() {
  return linter((view) => {
    const diagnostics: Diagnostic[] = [];
    const text = view.state.doc.toString();
    if (!text.trim()) return diagnostics;
    const doc = parseDocument(text);
    for (const error of doc.errors) {
      const pos = Array.isArray(error.pos) ? error.pos : [0, 0];
      const from = Math.max(0, Math.min(pos[0], text.length));
      const to = Math.max(from + 1, Math.min(pos[1] || from + 1, text.length));
      diagnostics.push({
        from,
        to,
        severity: "error",
        message: error.message,
        source: "yaml",
      });
    }
    return diagnostics;
  });
}

interface Props {
  value: string;
  onChange: (v: string) => void;
  /** 只读（例如查看未保存前的备份） */
  readOnly?: boolean;
}

/** 专业 YAML 编辑器：CodeMirror 6 + 语法高亮 + 行内错误校验 */
export default function YamlEditor({ value, onChange, readOnly }: Props) {
  const { resolved } = useTheme();
  const extensions = useMemo(() => [yaml(), yamlLinter()], []);

  return (
    <div className="overflow-hidden rounded-md border border-border text-[12px]">
      <CodeMirror
        value={value}
        height="420px"
        theme={resolved === "dark" ? "dark" : "light"}
        extensions={[...extensions, ...(readOnly ? [] : [])]}
        editable={!readOnly}
        basicSetup={{
          lineNumbers: true,
          highlightActiveLine: true,
          bracketMatching: true,
          foldGutter: true,
          autocompletion: false,
        }}
        onChange={onChange}
      />
    </div>
  );
}
