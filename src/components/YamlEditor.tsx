import { useEffect, useMemo, useRef, useState } from "react";
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

/** 编辑器最小高度：内容再少也留一块能正常编辑的地方（约 8 行） */
const MIN_HEIGHT = 160;
/**
 * 封顶高度的下限。编辑器在页面很靠下的位置（插件页的 cordis.patch.yml 就在一长串卡片之后）
 * 或窗口很矮时，「到容器底边还剩多少」会算出一个没意义的小值 —— 这时退回一个固定可用高度，
 * 免得一个几百行的配置文件被压成一条缝。
 */
const MAX_HEIGHT_FLOOR = 320;
/**
 * 与滚动容器底边留的距离：内容区的 padding 也算在滚动高度里（`p-3 sm:p-4 lg:p-5` 最大 20px），
 * 再加一点取整余量 —— 留少了就会因为多出几像素而冒出页面滚动条。
 */
const BOTTOM_GAP = 28;

/**
 * 最近的「可滚动祖先」——应用里滚动发生在内容区（`overflow-y-auto`）而不是 window，
 * 量可用高度必须以它为准，否则算出来的是整页高度。
 */
function findScrollParent(el: HTMLElement | null): HTMLElement | null {
  let node = el?.parentElement ?? null;
  while (node) {
    const style = window.getComputedStyle(node);
    if (/(auto|scroll)/.test(style.overflowY) && node.clientHeight > 0) return node;
    node = node.parentElement;
  }
  return null;
}

/**
 * 专业 YAML 编辑器：CodeMirror 6 + 语法高亮 + 行内错误校验。
 *
 * 高度是「自适应」的，不再是写死的 420px：
 * - **内容多高就多高**（不传 height）—— 只改两行的配置文件不会再顶着一大片空白；
 * - 上方封顶在**滚动容器里它上方还剩多少高度**（编辑器上沿到容器底边），
 *   所以内容长时它能吃满窗口、在编辑器内部滚动，而不是把整页顶长、push 出双滚动条；
 * - 上沿位置按「距滚动容器内容顶部的距离」算，与当前滚动位置无关 ——
 *   用 getBoundingClientRect().top 会在滚动时算出越来越大的可用高度，编辑器随之抽动。
 */
export default function YamlEditor({ value, onChange, readOnly }: Props) {
  const { resolved } = useTheme();
  const extensions = useMemo(() => [yaml(), yamlLinter()], []);
  const wrapRef = useRef<HTMLDivElement>(null);
  const [maxHeight, setMaxHeight] = useState<number | null>(null);

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const measure = () => {
      const scroller = findScrollParent(el);
      const viewportH = scroller ? scroller.clientHeight : window.innerHeight;
      const topInContent = scroller
        ? el.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop
        : el.getBoundingClientRect().top;
      const avail = Math.max(MAX_HEIGHT_FLOOR, Math.round(viewportH - topInContent - BOTTOM_GAP));
      // 抖动守卫：滚动条出现/消失会让可用高度差一两像素，不值得重排
      setMaxHeight((prev) => (prev != null && Math.abs(prev - avail) < 4 ? prev : avail));
    };
    measure();
    window.addEventListener("resize", measure);
    const ro = new ResizeObserver(measure);
    const scroller = findScrollParent(el);
    if (scroller) ro.observe(scroller);
    // 上方标题 / 说明块换行会改变编辑器上沿，也要重算
    if (el.parentElement) ro.observe(el.parentElement);
    return () => {
      window.removeEventListener("resize", measure);
      ro.disconnect();
    };
  }, []);

  return (
    <div
      ref={wrapRef}
      className="dsh-code overflow-hidden rounded-lg border border-border bg-background/50 text-[12px]"
    >
      <CodeMirror
        value={value}
        minHeight={`${MIN_HEIGHT}px`}
        maxHeight={maxHeight != null ? `${maxHeight}px` : undefined}
        theme={resolved === "dark" ? "dark" : "light"}
        extensions={extensions}
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
