import { useMediaQuery } from "./use-layout";

/** ≥ 该宽度终端作为右侧常驻栏与内容并排；窄于此降级为 Sheet 覆盖式抽屉 */
export const TERMINAL_INLINE_MIN_WIDTH = 1280;

/** 终端面板宿主形态：true = 行内右栏；false = Sheet 抽屉（窄屏） */
export function useTerminalInline(): boolean {
  return useMediaQuery(`(min-width: ${TERMINAL_INLINE_MIN_WIDTH}px)`);
}
