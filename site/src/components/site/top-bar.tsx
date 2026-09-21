import { MoonIcon, SunIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { GithubMark } from "@/components/site/icons";
import { Led } from "@/components/site/led";
import { SourceSwitch } from "@/components/site/source-switch";
import { WhaleMark } from "@/components/site/whale";
import type { ReleaseController } from "@/hooks/use-release";
import { useTheme } from "@/hooks/use-theme";
import { REPO_URL } from "@/lib/release";

const NAV = [
  { href: "#download", label: "下载" },
  { href: "#install", label: "安装" },
  { href: "#sources", label: "更新源" },
  { href: "#features", label: "功能" },
  { href: "#faq", label: "常见问题" },
];

export function TopBar({ controller }: { controller: ReleaseController }) {
  const { state, source, setSource } = controller;
  const { theme, toggle } = useTheme();
  const manifestOk = state.r2 !== "down" || state.github !== "down";

  return (
    <header className="sticky top-0 z-40 border-b border-hairline bg-background/85 backdrop-blur-md">
      <div className="mx-auto flex h-14 max-w-[1180px] items-center gap-3 px-5 sm:px-8">
        <a href="#top" className="flex items-center gap-2.5" aria-label="回到页面顶部">
          <WhaleMark className="size-[22px] text-primary" />
          <span className="text-display hidden text-[12.5px] tracking-[0.22em] uppercase sm:inline">
            DSH Starter
          </span>
        </a>

        {/* 版本徽标：值来自实时清单，所以它同时是「清单读到没」的指示灯 */}
        <span className="flex items-center gap-1.5 rounded-md border border-border bg-card/60 px-2 py-[3px]">
          <Led tone={!state.settled ? "muted" : manifestOk ? "signal" : "warn"} pulse={!state.settled} />
          <span className="num text-[11px] text-muted-foreground">
            {state.version ? `v${state.version}` : state.settled ? "清单不可达" : "读取中"}
          </span>
        </span>

        <nav className="ml-3 hidden items-center gap-5 md:flex">
          {NAV.map((item) => (
            <a
              key={item.href}
              href={item.href}
              className="text-[13px] text-muted-foreground transition-colors hover:text-foreground"
            >
              {item.label}
            </a>
          ))}
        </nav>

        <div className="ml-auto flex items-center gap-1.5 sm:gap-2">
          <SourceSwitch value={source} onChange={setSource} state={state} className="hidden sm:flex" />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            onClick={toggle}
            className="text-muted-foreground"
            aria-label={theme === "dark" ? "切换到亮色主题" : "切换到暗色主题"}
            title={theme === "dark" ? "切换到亮色主题" : "切换到暗色主题"}
          >
            {theme === "dark" ? <SunIcon /> : <MoonIcon />}
          </Button>
          <Button asChild variant="ghost" size="icon-sm" className="text-muted-foreground">
            <a href={REPO_URL} target="_blank" rel="noreferrer noopener" aria-label="GitHub 仓库">
              <GithubMark className="size-4" />
            </a>
          </Button>
        </div>
      </div>
    </header>
  );
}
