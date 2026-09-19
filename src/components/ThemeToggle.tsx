import { Monitor, Moon, Sun } from "lucide-react";
import { useTheme, type Theme } from "@/lib/theme";
import { Button } from "@/components/ui/button";

const ORDER: Theme[] = ["system", "light", "dark"];

export function ThemeToggle() {
  const { theme, resolved, setTheme } = useTheme();
  const next = ORDER[(ORDER.indexOf(theme) + 1) % ORDER.length];
  const label =
    theme === "system" ? `跟随系统（当前${resolved === "dark" ? "深色" : "浅色"}）` : theme === "dark" ? "深色" : "浅色";
  return (
    <Button
      variant="ghost"
      size="icon"
      onClick={() => setTheme(next)}
      title={`主题：${label}（点击切换）`}
    >
      {theme === "system" ? (
        <Monitor className="h-4 w-4" />
      ) : resolved === "dark" ? (
        <Moon className="h-4 w-4" />
      ) : (
        <Sun className="h-4 w-4" />
      )}
    </Button>
  );
}
