import * as React from "react"

/** 抽屉（窄窗口）断点：与 ui/sidebar.tsx 的 CSS 断点（md = 768px）保持一致。
 *  抽成常量是为了让布局档位 hook（hooks/use-layout.ts）与 Sidebar 走同一套判定，避免两处漂移。 */
export const MOBILE_BREAKPOINT = 768

export function useIsMobile() {
  const [isMobile, setIsMobile] = React.useState<boolean | undefined>(undefined)

  React.useEffect(() => {
    const mql = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`)
    const onChange = () => {
      setIsMobile(window.innerWidth < MOBILE_BREAKPOINT)
    }
    mql.addEventListener("change", onChange)
    setIsMobile(window.innerWidth < MOBILE_BREAKPOINT)
    return () => mql.removeEventListener("change", onChange)
  }, [])

  return !!isMobile
}
