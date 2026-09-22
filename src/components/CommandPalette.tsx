import { Fragment, useMemo } from "react";
import type { LucideIcon } from "lucide-react";
import { Command, CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator, CommandShortcut } from "@/components/ui/command";

/** 命令面板里的一条命令：label 是展示与搜索的主文案，keywords 补充额外搜索词 */
export interface PaletteCommand {
  id: string;
  label: string;
  group: string;
  icon: LucideIcon;
  /** 右侧灰色提示（如「⌘K」「从侧栏进入」），不是快捷键绑定 */
  hint?: string;
  keywords?: string;
  disabled?: boolean;
  run: () => void;
}

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  commands: PaletteCommand[];
}

/** 底部说明行里的按键样式（与侧栏/顶栏的 kbd 观感一致） */
function Key({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex h-4 min-w-4 items-center justify-center rounded border border-border bg-muted px-1 font-mono text-[10px] text-muted-foreground">
      {children}
    </kbd>
  );
}

/**
 * 全局命令面板（⌘K / Ctrl+K）：汇总页面跳转与高频操作。
 * 命令列表由 App.tsx 组装传入，这里只负责搜索、分组展示与执行。
 */
export function CommandPalette({ open, onOpenChange, commands }: Props) {
  // 按 group 聚合成有序分组（保持命令首次出现的组顺序）
  const groups = useMemo(() => {
    const map = new Map<string, PaletteCommand[]>();
    for (const c of commands) {
      const list = map.get(c.group);
      if (list) list.push(c);
      else map.set(c.group, [c]);
    }
    return [...map.entries()];
  }, [commands]);

  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title="命令面板"
      description="搜索功能，或输入命令"
      className="top-1/4 sm:max-w-[560px]"
    >
      {/* 本项目的 CommandDialog 不含 Command 包装层（见 ui/command.tsx），
          cmdk 的 store 由 Command 创建，缺它会直接抛 undefined.subscribe */}
      <Command className="rounded-lg bg-transparent">
        <CommandInput placeholder="搜索功能，或输入命令…" />
        <CommandList className="max-h-[45vh]">
          <CommandEmpty>无匹配结果</CommandEmpty>
          {groups.map(([group, items], gi) => (
            <Fragment key={group}>
              {gi > 0 && <CommandSeparator />}
              <CommandGroup heading={group}>
                {items.map((c) => {
                  const Icon = c.icon;
                  return (
                    <CommandItem
                      key={c.id}
                      value={`${group} ${c.label} ${c.keywords ?? ""}`}
                      disabled={c.disabled}
                      onSelect={() => {
                        c.run();
                        onOpenChange(false);
                      }}
                    >
                      <Icon />
                      <span>{c.label}</span>
                      {c.hint && <CommandShortcut>{c.hint}</CommandShortcut>}
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            </Fragment>
          ))}
        </CommandList>
      </Command>
      <div className="flex items-center gap-4 border-t border-border px-3 py-1.5 text-[11px] text-muted-foreground">
        <span className="flex items-center gap-1"><Key>↑</Key><Key>↓</Key> 选择</span>
        <span className="flex items-center gap-1"><Key>Enter</Key> 执行</span>
        <span className="flex items-center gap-1"><Key>Esc</Key> 关闭</span>
      </div>
    </CommandDialog>
  );
}
