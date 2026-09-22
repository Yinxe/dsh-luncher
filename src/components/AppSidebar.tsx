import { useCallback } from "react";
import {
  Bot, Copy, ExternalLink, FileCog, Home, KeyRound, Package, Puzzle, Rocket, ScrollText, X,
} from "lucide-react";

import {
  Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent, SidebarGroupLabel,
  SidebarHeader, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarRail,
  useSidebar,
} from "@/components/ui/sidebar";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { api } from "../api";
import type { EnvironmentInfo, Settings as SettingsT, View } from "../types";

/** 项目仓库：源码、问题反馈与更新日志都在这里（主栏底栏的「官方源 / 数据目录」并进侧栏后，
 *  侧栏底部顺带给出这个入口，省得用户去设置里翻仓库地址） */
const REPO_URL = "https://github.com/Yinxe/dsh-starter";
const CHANGELOG_URL = `${REPO_URL}/blob/main/CHANGELOG.md`;

interface Props {
  view: View;
  onNavigate: (v: View) => void;
  env: EnvironmentInfo;
  settings: SettingsT;
  /** 运行中的实例数（含独立进程 / 终端外部启动的） */
  runningInstanceCount: number;
  /** 可升级的版本数 */
  upgradableCount: number;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
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
 * 「标签 + 值」的信息行（侧栏底部卡片用）。
 *
 * 路径这类值是整行里最长的内容：左侧标签固定宽度让所有值左对齐成一列，
 * 值本身截断显示、完整内容进 tooltip，鼠标悬停时行尾浮出复制按钮 ——
 * 窄侧栏里「看得全」做不到，就把「拿得到」补上。
 */
function InfoRow({
  label, value, title, onCopy,
}: {
  label: string;
  value: string;
  title?: string;
  onCopy?: () => void;
}) {
  return (
    <div className="group/row flex items-center gap-1.5" title={title ?? value}>
      <span className="w-10 shrink-0 text-[10.5px] text-muted-foreground">{label}</span>
      <span className="min-w-0 flex-1 truncate font-mono text-[10.5px]">{value}</span>
      {onCopy && (
        <button
          type="button"
          title={`复制 ${title ?? value}`}
          aria-label={`复制${label}`}
          onClick={onCopy}
          className="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 transition-opacity hover:text-foreground focus-visible:opacity-100 group-hover/row:opacity-100"
        >
          <Copy className="size-3" />
        </button>
      )}
    </div>
  );
}

/**
 * 把绝对路径缩成 `~/…` 形式。
 *
 * home 从启动器数据目录反推（后端 `starter_home()` 固定是 `<home>/.dsh-starter`），
 * 不引额外的环境字段：宽度只有 200px 出头的侧栏里，`/home/yinxin/.dsh-starter/versions`
 * 这种整串路径会被截掉大半，`~/.dsh-starter/versions` 才看得清。
 * 反推失败（自定义 DSH_STARTER_HOME 之类）就原样显示，只靠截断 + tooltip 兜底。
 */
function tildePath(p: string, starterHome: string): string {
  const home = starterHome.match(/^(.*)[/\\]\.dsh-starter[/\\]?$/)?.[1];
  if (!home) return p;
  if (p === home) return "~";
  if (p.startsWith(`${home}/`) || p.startsWith(`${home}\\`)) return `~${p.slice(home.length)}`;
  return p;
}

/** 显示用的 registry：去掉 scheme —— 侧栏窄列里 `https://` 只是噪声（tooltip 与复制仍是完整地址） */
const shortRegistry = (url: string) => url.replace(/^https?:\/\//i, "").replace(/\/+$/, "");

/** GitHub 品牌标（lucide 已移除品牌图标；这里只画一个 logo 字形，不是自建 UI 组件） */
function GitHubMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden className={className}>
      <path d="M12 .3a12 12 0 0 0-3.8 23.4c.6.1.8-.3.8-.6v-2c-3.3.7-4-1.6-4-1.6-.6-1.4-1.4-1.8-1.4-1.8-1-.7.1-.7.1-.7 1.2.1 1.8 1.2 1.8 1.2 1 1.8 2.8 1.3 3.5 1 .1-.8.4-1.3.7-1.6-2.7-.3-5.5-1.3-5.5-5.9 0-1.3.5-2.4 1.2-3.2-.1-.3-.5-1.5.1-3.2 0 0 1-.3 3.3 1.2a11.5 11.5 0 0 1 6 0c2.3-1.5 3.3-1.2 3.3-1.2.6 1.7.2 2.9.1 3.2.8.8 1.2 1.9 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6A12 12 0 0 0 12 .3" />
    </svg>
  );
}

/**
 * 应用侧栏：shadcn Sidebar（collapsible=icon）。
 * 宽窗口常驻展开、窄窗口自动收成图标栏、超窄窗口退化为 Sheet 抽屉——
 * 三档切换由 hooks/use-layout.ts 控制 open，这里只负责内容编排。
 */
export default function AppSidebar({
  view, onNavigate, env, settings, runningInstanceCount, upgradableCount, onToast,
}: Props) {
  const { isMobile, setOpenMobile } = useSidebar();

  const openUrl = useCallback(
    (url: string, what: string) => {
      api.openUrl(url).catch((e) => onToast("err", `打开${what}失败：${e}`));
    },
    [onToast],
  );

  const copyPath = useCallback(
    async (text: string, label: string) => {
      try {
        await navigator.clipboard.writeText(text);
        onToast("ok", `${label}已复制：${text}`);
      } catch {
        // 剪贴板不可用时给出仍能拿到完整路径的下一步，而不是只说「复制失败」
        onToast("err", `复制失败（剪贴板不可用）：到「配置文件」或设置里可以看到完整路径`);
      }
    },
    [onToast],
  );

  const navItems: Array<[View, string, typeof Package, number | null, "warning" | "success" | null]> = [
    ["quick", "首页", Home, null, null],
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
            <div className="truncate text-[13px] font-bold tracking-tight">DSH Starter v{env.appVersion}</div>
            <div className="truncate text-[10px] text-muted-foreground">
              @deepseek-ai/dsh · {settings.activeVersion ? `v${settings.activeVersion}` : "未安装"}
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

      <SidebarFooter className="gap-2 border-t border-sidebar-border p-2">
        {/* 展开态：环境卡片 + 项目仓库卡片。
            这里以前只是三行「灯 + 文本 + 两行裸路径」，而主栏另有一条底栏显示
            「官方源 / 数据目录 / 托盘提示」—— 同一批数据放在两处、两处都不全。
            现在主栏底栏撤掉，全部收进这两张卡片：状态一行一灯、路径一列对齐、
            值可截断但能悬停看全文 / 一键复制，仓库入口固定在卡片底部。 */}
        <div className="space-y-2 group-data-[collapsible=icon]:hidden">
          <Card size="sm" className="gap-0 border-sidebar-border bg-sidebar-accent/40 py-2 shadow-none ring-sidebar-border">
            <div className="eyebrow px-2.5">运行状态</div>
            <div className="mt-1.5 space-y-0.5 px-2.5 text-[11.5px]">
              <StatusDot
                tone={env.node ? "ok" : "off"}
                label={env.node ? `Node v${env.node}` : "Node 未装"}
                title={env.nodePath ?? undefined}
              />
              <StatusDot
                tone={settings.activeVersion ? "ok" : "warn"}
                label={settings.activeVersion || "版本未选"}
                title={settings.activeVersion ? "当前 dsh 版本" : "还没选版本：到「版本与安装」安装并选用一个"}
              />
              <StatusDot
                tone={runningInstanceCount > 0 ? "ok" : "off"}
                label={`${runningInstanceCount} 个实例运行中`}
              />
            </div>

            <Separator className="my-2 bg-sidebar-border" />

            <div className="eyebrow px-2.5">目录与源</div>
            <div className="mt-1.5 space-y-0.5 px-2.5">
              <InfoRow
                label="npm 源"
                value={shortRegistry(env.registry)}
                title={env.registry}
                onCopy={() => copyPath(env.registry, "registry 地址")}
              />
              <InfoRow
                label="启动器"
                value={tildePath(env.dshHome, env.dshHome)}
                title={env.dshHome}
                onCopy={() => copyPath(env.dshHome, "启动器数据目录")}
              />
              <InfoRow
                label="版本"
                value={tildePath(env.versionsDir, env.dshHome)}
                title={env.versionsDir}
                onCopy={() => copyPath(env.versionsDir, "版本目录")}
              />
              <InfoRow
                label="dsh"
                value={tildePath(env.dshNativeHome, env.dshHome)}
                title={env.dshNativeHome}
                onCopy={() => copyPath(env.dshNativeHome, "dsh 数据目录")}
              />
            </div>

            <p className="mt-2 px-2.5 text-[10px] leading-relaxed text-muted-foreground">
              退出启动器会结束所有内嵌 dsh 进程；关闭窗口最小化到托盘
            </p>
          </Card>

          <Card size="sm" className="gap-0 border-sidebar-border bg-sidebar-accent/40 py-2 shadow-none ring-sidebar-border">
            <div className="flex items-center gap-2 px-2.5">
              <GitHubMark className="size-3.5 shrink-0 text-muted-foreground" />
              <div className="min-w-0 flex-1">
                <div className="truncate font-mono text-[11px] font-semibold">Yinxe/dsh-starter</div>
                <div className="truncate text-[10px] text-muted-foreground">源码 · 问题反馈 · 更新日志</div>
              </div>
            </div>
            <div className="mt-2 grid grid-cols-2 gap-1.5 px-2.5">
              <Button
                size="sm"
                variant="outline"
                className="h-6 gap-1 px-1.5 text-[10.5px]"
                title={REPO_URL}
                onClick={() => openUrl(REPO_URL, "项目仓库")}
              >
                <ExternalLink className="size-3" /> 打开仓库
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="h-6 gap-1 px-1.5 text-[10.5px]"
                title={CHANGELOG_URL}
                onClick={() => openUrl(CHANGELOG_URL, "更新日志")}
              >
                <ScrollText className="size-3" /> 更新日志
              </Button>
            </div>
          </Card>
        </div>

        {/* 收起态：三颗状态灯 + 仓库入口（tooltip 兜底） */}
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
          <Button
            variant="ghost"
            size="icon-sm"
            className="mt-0.5"
            title={`项目仓库：${REPO_URL}`}
            onClick={() => openUrl(REPO_URL, "项目仓库")}
          >
            <GitHubMark className="size-3.5" />
          </Button>
        </div>
      </SidebarFooter>

      {/* 边缘拖拽热区：点一下也能折叠/展开（抽屉态不需要，且会探出视口 8px） */}
      {!isMobile && <SidebarRail />}
    </Sidebar>
  );
}
