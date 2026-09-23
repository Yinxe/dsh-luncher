import * as React from "react"

import { MOBILE_BREAKPOINT } from "./use-mobile"

/**
 * 侧栏档位（按窗口宽度分档，只在跨档时重渲染）：
 *
 * - `expanded` 宽窗口：侧栏常驻展开（16rem），导航文字 + 角标齐全；
 * - `rail`     窄窗口：侧栏自动收成 3rem 图标栏，把横向空间让给内容区，
 *              用户仍可手动展开（此时以浮层方式并排，不遮挡内容）；
 * - `drawer`   超窄窗口：侧栏退化为抽屉（shadcn Sidebar 内置的 Sheet），
 *              顶栏的折叠按钮变成「打开菜单」，选中导航项后自动收抽屉。
 */
export type NavTier = "expanded" | "rail" | "drawer"

/** ≥ 该宽度默认展开侧栏；窄于它自动收成图标栏（与 Tailwind lg 断点对齐，便于和内容区栅格协同） */
export const NAV_EXPAND_MIN_WIDTH = 1024

/** 订阅一条 media query，只在跨越断点时触发一次重渲染（比监听 resize 省得多） */
export function useMediaQuery(query: string): boolean {
  const subscribe = React.useCallback(
    (onChange: () => void) => {
      const mql = window.matchMedia(query)
      mql.addEventListener("change", onChange)
      return () => mql.removeEventListener("change", onChange)
    },
    [query]
  )
  const getSnapshot = React.useCallback(() => window.matchMedia(query).matches, [query])
  return React.useSyncExternalStore(subscribe, getSnapshot, () => false)
}

/** 当前窗口宽度落在哪个侧栏档位 */
export function useNavTier(): NavTier {
  const isDrawer = useMediaQuery(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`)
  const isRail = useMediaQuery(`(max-width: ${NAV_EXPAND_MIN_WIDTH - 1}px)`)
  if (isDrawer) return "drawer"
  return isRail ? "rail" : "expanded"
}

/**
 * 侧栏开合状态：默认跟随档位（宽窗口展开 / 窄窗口收成图标栏），
 * 用户手动点过折叠按钮后以手动值为准；一旦跨越断点换档则丢弃手动值，
 * 重新回到该档的默认形态——这样拖窗口永远不会把侧栏卡在错误形态上。
 */
export function useSidebarOpen(): [boolean, (open: boolean) => void] {
  const tier = useNavTier()
  const [override, setOverride] = React.useState<boolean | null>(null)

  React.useEffect(() => {
    setOverride(null)
  }, [tier])

  return [override ?? tier === "expanded", setOverride]
}
