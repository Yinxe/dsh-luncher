import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

export type Theme = "light" | "dark" | "system";
const STORAGE_KEY = "dsh-starter-theme";
/** 0.2.0 改名前的键：读取时兜底认它一次，别让老用户升级后主题被重置 */
const LEGACY_STORAGE_KEY = "dsh-launcher-theme";

/** 已存的主题：新键优先，没有就回落到旧键（旧值一旦被写过就以新键为准） */
function readStoredTheme(): Theme {
  const saved = localStorage.getItem(STORAGE_KEY) ?? localStorage.getItem(LEGACY_STORAGE_KEY);
  return saved === "light" || saved === "dark" || saved === "system" ? saved : "system";
}

interface ThemeCtx {
  theme: Theme;
  resolved: "light" | "dark";
  setTheme: (t: Theme) => void;
}

const Ctx = createContext<ThemeCtx>({
  theme: "system",
  resolved: "dark",
  setTheme: () => undefined,
});

function systemPrefersDark(): boolean {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;
}

function apply(theme: Theme): "light" | "dark" {
  const resolved = theme === "system" ? (systemPrefersDark() ? "dark" : "light") : theme;
  document.documentElement.classList.toggle("dark", resolved === "dark");
  return resolved;
}

/** 短暂挂上 .theme-transitioning，让颜色属性获得 250ms 过渡（全平台统一的主题切换效果） */
function withColorTransition(flush: () => void): void {
  document.documentElement.classList.add("theme-transitioning");
  flush();
  window.setTimeout(
    () => document.documentElement.classList.remove("theme-transitioning"),
    300
  );
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(readStoredTheme);
  const [resolved, setResolved] = useState<"light" | "dark">(() => apply(theme));

  // 跟随系统实时切换
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      if (readStoredTheme() === "system") {
        withColorTransition(() => setResolved(apply("system")));
      }
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);

  const setTheme = useCallback((t: Theme) => {
    localStorage.setItem(STORAGE_KEY, t);
    setThemeState(t);
    withColorTransition(() => setResolved(apply(t)));
  }, []);

  return <Ctx.Provider value={{ theme, resolved, setTheme }}>{children}</Ctx.Provider>;
}

export function useTheme() {
  return useContext(Ctx);
}
