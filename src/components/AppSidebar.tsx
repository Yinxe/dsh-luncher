import { Bot, FileCog, KeyRound, Package, Puzzle, Rocket, X } from "lucide-react";

import {
  Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent, SidebarGroupLabel,
  SidebarHeader, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarRail,
  useSidebar,
} from "@/components/ui/sidebar";
import { Button } from "@/components/ui/button";
import type { EnvironmentInfo, Settings as SettingsT, View } from "../types";

interface Props {
  view: View;
  onNavigate: (v: View) => void;
  env: EnvironmentInfo;
  settings: SettingsT;
  /** 运行中的实例数（含独立进程 / 终端外部启动的） */
  runningInstanceCount: number;
  /** 可升级的版本数 */
  upgradableCount: number;
}

/** 侧栏底部的一行状态：展开态用「灯 + 文字」，收起态只剩灯（tooltip 兜底） */
function StatusDot({ tone, label, title }: { tone: "ok" | "warn" | "off"; label: string; title?: string }) {
  const color =
    tone === "ok" ? "bg-emerald-500" : tone === "warn" ? "bg-amber-500" : "bg-muted-foreground/40";
  return (
    <div className="flex items-center gap-2" title={title ?? label}>
      <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${color}`} />
      <span className="truncate font-mono">{label}</span>
    </div>
  );
}

/**
 * 应用侧栏：shadcn Sidebar（collapsible=icon）。
 * 宽窗口常驻展开、窄窗口自动收成图标栏、超窄窗口退化为 Sheet 抽屉——
 * 三档切换由 hooks/use-layout.ts 控制 open，这里只负责内容编排。
 */
export default function AppSidebar({
  view, onNavigate, env, settings, runningInstanceCount, upgradableCount,
}: Props) {
  const { isMobile, setOpenMobile } = useSidebar();

  const navItems: Array<[View, string, typeof Package, number | null, "warning" | "success" | null]> = [
    ["versions", "版本与安装", Package, upgradableCount > 0 ? upgradableCount : null, "warning"],
    ["profiles", "Profile 实例", Rocket, runningInstanceCount > 0 ? runningInstanceCount : null, "success"],
    ["plugins", "插件管理", Puzzle, null, null],
    ["models", "模型配置", Bot, null, null],
    ["config", "配置文件", FileCog, null, null],
    ["credentials", "凭据管理", KeyRound, null, null],
  ];

  const navigate = (key: View) => {
    onNavigate(key);
    // 抽屉态：选中即收起，避免遮住刚打开的内容
    if (isMobile) setOpenMobile(false);
  };

  return (
    <Sidebar collapsible="icon" className="border-sidebar-border">
      <SidebarHeader className="border-b border-sidebar-border">
        <div className="flex items-center gap-2.5 px-1 py-1 group-data-[collapsible=icon]:hidden">
          {/* 走 public/ 静态资源，避免把 SVG 当 JS 模块加载（见 index.html 的 favicon） */}
          <img src="/dsh-logo.svg" alt="DSH" className="h-8 w-8 shrink-0" draggable={false} />
          <div className="min-w-0 leading-tight">
            <div className="truncate text-[13px] font-bold tracking-tight">DSH Launcher</div>
            <div className="truncate text-[10px] text-muted-foreground">
              @deepseek-ai/dsh · v{env.appVersion}
            </div>
          </div>
          {/* 抽屉态没有 Sheet 自带的关闭按钮（Sidebar 隐藏了它），这里补一个 */}
          <Button
            variant="ghost"
            size="icon-sm"
            className="ml-auto md:hidden"
            title="收起菜单"
            onClick={() => setOpenMobile(false)}
          >
            <X />
          </Button>
        </div>
        {/* 收起态：只留居中的 logo */}
        <div className="hidden justify-center py-0.5 group-data-[collapsible=icon]:flex">
          <img src="/dsh-logo.svg" alt="DSH" className="h-7 w-7" draggable={false} />
        </div>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel className="eyebrow">导航</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {navItems.map(([key, label, Icon, badge, badgeTone]) => (
                <SidebarMenuItem key={key}>
                  <SidebarMenuButton
                    isActive={view === key}
                    tooltip={label}
                    className={`relative gap-2.5 ${view === key ? "text-foreground" : "text-muted-foreground hover:text-foreground"}`}
                    onClick={() => navigate(key)}
                  >
                    {view === key && (
                      <span className="absolute left-0 top-1/2 h-4 w-[2.5px] -translate-y-1/2 animate-in fade-in slide-in-from-left-1 rounded-full bg-primary duration-200 group-data-[collapsible=icon]:hidden" />
                    )}
                    <Icon className="opacity-80" />
                    <span>{label}</span>
                  </SidebarMenuButton>
                  {badge != null && (
                    <>
                      <SidebarMenuBadge
                        className={badgeTone === "success" ? "text-emerald-600 dark:text-emerald-400" : "text-amber-600 dark:text-amber-400"}
                      >
                        {badge}
                      </SidebarMenuBadge>
                      {/* 收起态角标被隐藏，用一个点标记「有动静」 */}
                      <span
                        title={`${label}：${badge}`}
                        className={`absolute right-0.5 top-0.5 hidden size-2 rounded-full ring-2 ring-sidebar group-data-[collapsible=icon]:block ${
                          badgeTone === "success" ? "bg-emerald-500" : "bg-amber-500"
                        }`}
                      />
                    </>
                  )}
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter className="border-t border-sidebar-border">
        {/* 展开态：完整环境状态 */}
        <div className="space-y-2 px-1 py-0.5 text-[11.5px] group-data-[collapsible=icon]:hidden">
          <StatusDot
            tone={env.node ? "ok" : "off"}
            label={env.node ? `Node v${env.node}` : "Node 未装"}
            title={env.nodePath ?? undefined}
          />
          <StatusDot
            tone={settings.activeVersion ? "ok" : "warn"}
            label={settings.activeVersion || "版本未选"}
          />
          <StatusDot
            tone={runningInstanceCount > 0 ? "ok" : "off"}
            label={`${runningInstanceCount} 个实例运行中`}
          />
          <div className="space-y-0.5 pt-1 text-[10.5px] leading-relaxed text-muted-foreground">
            <div className="truncate" title={env.registry}>registry：{env.registry}</div>
            <div className="break-all" title={env.versionsDir}>数据目录：{env.versionsDir}</div>
          </div>
        </div>
        {/* 收起态：三颗状态灯 */}
        <div className="hidden flex-col items-center gap-2 py-1 group-data-[collapsible=icon]:flex">
          <span
            className={`led ${env.node ? "bg-emerald-500" : "bg-red-500"}`}
            title={env.node ? `Node v${env.node}` : "Node 未装"}
          />
          <span
            className={`led ${settings.activeVersion ? "bg-emerald-500" : "bg-amber-500"}`}
            title={settings.activeVersion || "版本未选"}
          />
          <span
            className={`led ${runningInstanceCount > 0 ? "bg-emerald-500" : "bg-muted-foreground/40"}`}
            title={`${runningInstanceCount} 个实例运行中`}
          />
        </div>
      </SidebarFooter>

      {/* 边缘拖拽热区：点一下也能折叠/展开（抽屉态不需要，且会探出视口 8px） */}
      {!isMobile && <SidebarRail />}
    </Sidebar>
  );
}
