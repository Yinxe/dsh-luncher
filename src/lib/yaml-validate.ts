import { useCallback, useMemo, useRef } from "react";
import { parseDocument } from "yaml";

/** 一处 YAML 语法错误（行列均 1 起始，供「第 X 行」文案与编辑器标红共用） */
export interface YamlIssue {
  line: number;
  column: number;
  message: string;
}

/**
 * YAML 语法前置校验（保存闸门 + 编辑器行内标红的唯一来源）。
 *
 * 后端写入前仍会做权威校验（serde_yaml），这里的作用是：
 * - 错误在保存按钮被按下之前就可见、并禁用保存，不让坏内容落盘后再靠报错兜底；
 * - tab 缩进：`yaml` 包只给 warning，而 serde_yaml 直接拒收，这里升级为 error 对齐后端口径。
 */
export function validateYaml(text: string): YamlIssue[] {
  if (!text.trim()) return [];
  const issues: YamlIssue[] = [];
  const tabRe = /^ *\t/gm;
  let m: RegExpExecArray | null;
  while ((m = tabRe.exec(text)) !== null) {
    const tabAt = m.index + m[0].length - 1;
    issues.push({
      line: text.slice(0, tabAt).split("\n").length,
      column: m[0].length,
      message: "使用了 Tab 缩进（YAML 只允许空格）",
    });
    if (issues.length >= 8) return issues;
  }
  let doc;
  try {
    doc = parseDocument(text);
  } catch (e) {
    return [...issues, { line: 1, column: 1, message: `解析失败：${e instanceof Error ? e.message : String(e)}` }];
  }
  for (const err of doc.errors) {
    // tab 缩进已由上面的行首扫描报过（yaml 包会把它同时塞进 errors），跳过以免重复
    if (/tab/i.test(err.message)) continue;
    const from = Array.isArray(err.linePos) ? err.linePos[0] : undefined;
    // 取首行、去掉尾部的「at line X, column Y:」（行列由展示方统一加前缀）
    const msg = err.message.split(/\r?\n/)[0].replace(/\s+at line \d+, column \d+:?\s*$/, "");
    issues.push({ line: from?.line ?? 1, column: from?.col ?? 1, message: msg });
    if (issues.length >= 8) break;
  }
  return issues;
}

/** 随内容变化的校验结果 */
export function useYamlIssues(text: string): YamlIssue[] {
  return useMemo(() => validateYaml(text), [text]);
}

/** Ctrl/Cmd+S 保存快捷键：只在可保存时拦截浏览器默认行为；用 ref 避免 keydown 时的过期闭包 */
export function useSaveHotkey(enabled: boolean, save: () => void) {
  const state = useRef({ enabled, save });
  state.current = { enabled, save };
  return useCallback((e: React.KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "s") {
      if (state.current.enabled) {
        e.preventDefault();
        state.current.save();
      }
    }
  }, []);
}
