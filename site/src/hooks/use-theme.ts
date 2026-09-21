import { useCallback, useEffect, useState } from "react";

/**
 * 亮暗主题。
 *
 * **默认亮色**（用户没选过就是亮色，不跟随系统）：这一页是下载入口，第一眼要的是
 * 干净易读的纸底，而不是跟着访客的系统设置变。手动切过就记住选择，之后一直用它。
 *
 * 首帧由 index.html 里的内联脚本定好（避免「先亮后暗」的闪光），这里只负责读回状态与切换。
 */
const STORAGE_KEY = "dsh-site:theme";
export type ThemeName = "light" | "dark";

function apply(theme: ThemeName) {
  const root = document.documentElement;
  root.classList.toggle("dark", theme === "dark");
  root.style.colorScheme = theme;
}

export function useTheme() {
  const [theme, setTheme] = useState<ThemeName>(() =>
    typeof document === "undefined" ? "light" : document.documentElement.classList.contains("dark") ? "dark" : "light",
  );

  useEffect(() => {
    apply(theme);
  }, [theme]);

  const toggle = useCallback(() => {
    setTheme((prev) => {
      const next: ThemeName = prev === "dark" ? "light" : "dark";
      try {
        window.localStorage.setItem(STORAGE_KEY, next);
      } catch {
        /* 存不下也让它切这一次 */
      }
      return next;
    });
  }, []);

  return { theme, toggle };
}
