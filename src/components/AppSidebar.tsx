import { useCallback } from "react";
import {
  BarChart3, Command, Copy, Download, ExternalLink, Gauge, Home, KeyRound, MessageSquareText, Package, RefreshCw, Rocket, ScrollText, Terminal, X,
} from "lucide-react";

import {
  Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent, SidebarGroupLabel,
  SidebarHeader, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarRail,
  useSidebar,
} from "@/components/ui/sidebar";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Popover, PopoverContent, PopoverTrigger,
} from "@/components/ui/popover";
import { Separator } from "@/components/ui/separator";
import { api } from "../api";
import type { EnvironmentInfo, Settings as SettingsT, View } from "../types";

/** 项目仓库：源码、问题反馈与更新日志都在这里（主栏底栏的「官方源 / 数据目录」并进侧栏后，
 *  侧栏底部顺带给出这个入口，省得用户去设置里翻仓库地址） */
const REPO_URL = "https://github.com/Yinxe/dsh-starter";
const CHANGELOG_URL = `${REPO_URL}/blob/main/CHANGELOG.md`;
/** 下载页（GitHub Pages）：各平台安装包直链的产品页 */
const SITE_URL = "https://yinxe.github.io/dsh-starter/";
/** 用户 QQ 群：反馈问题与交流（侧栏仓库卡里一键复制群号） */
const QQ_GROUP = "1067778060";

interface Props {
  view: View;
  onNavigate: (v: View) => void;
  env: EnvironmentInfo;
  settings: SettingsT;
  /** 运行中的实例数（含独立进程 / 终端外部启动的） */
  runningInstanceCount: number;
  /** 可升级的版本数 */
  upgradableCount: number;
  /** 通用终端面板是否打开 */
  terminalOpen: boolean;
  /** 打开/收起通用终端面板 */
  onToggleTerminal: () => void;
  /** 检查启动器新版本（原顶栏按钮，收进仓库卡） */
  onCheckUpdate: () => void;
  /** 打开全局命令面板（面板本体挂在 App.tsx，⌘K / Ctrl+K 同效） */
  onOpenPalette: () => void;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

/** 命令面板快捷键的展示文案（仅显示用，监听在 App.tsx 同时接受 ⌘K 与 Ctrl+K） */
const PALETTE_KEY =
  typeof navigator !== "undefined" && /mac/i.test(navigator.platform || navigator.userAgent) ? "⌘K" : "Ctrl+K";

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
  view, onNavigate, env, settings, runningInstanceCount, upgradableCount,
  terminalOpen, onToggleTerminal, onCheckUpdate, onOpenPalette, onToast,
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
        onToast("err", `复制失败（剪贴板不可用）：完整路径可在「Profiles」工作台的「配置文件」Tab 与设置里看到`);
      }
    },
    [onToast],
  );

  const navItems: Array<[View, string, typeof Package, number | null, "warning" | "success" | null]> = [
    ["quick", "首页", Home, null, null],
    ["versions", "版本与安装", Package, upgradableCount > 0 ? upgradableCount : null, "warning"],
    ["profiles", "Profiles（实例与配置）", Rocket, runningInstanceCount > 0 ? runningInstanceCount : null, "success"],
    ["credentials", "凭据管理", KeyRound, null, null],
    ["stats", "统计", BarChart3, runningInstanceCount > 0 ? runningInstanceCount : null, "success"],
    ["logs", "系统日志", ScrollText, null, null],
  ];

  const navigate = (key: View) => {
    onNavigate(key);
    // 抽屉态：选中即收起，避免遮住刚打开的内容
    if (isMobile) setOpenMobile(false);
  };

  /**
   * 「运行状态」详情：环境状态灯 + 实例终端入口 + 目录与源路径。
   * 展开态与收起态共用同一份内容，点按钮后在侧栏旁边以 Popover 展示，
   * 不再常驻一张大卡片占掉侧栏底部空间。
   */
  const envDetails = (
    <>
      <div className="eyebrow">运行状态</div>
      <div className="space-y-0.5 text-[11.5px]">
        <StatusDot
          tone={env.node ? "ok" : "off"}
          label={env.node ? `Node v${env.node}` : "Node 未装"}
          title={env.nodePath ?? undefined}
        />
        <StatusDot
          tone={env.npm ? "ok" : env.npmPath ? "warn" : "off"}
          label={env.npm ? `npm ${env.npm}` : env.npmPath ? "npm 不可用" : "npm 未装"}
          title={
            env.npm
              ? (env.npmPath ?? undefined)
              : env.npmPath
                ? `已解析到 ${env.npmPath}，但执行失败（不是可用的 npm？）。详见「系统日志」页的 app 分类`
                : "PATH 中没有找到 npm；可在设置里指定 Node 路径，或安装内置 Node 运行时"
          }
        />
        <StatusDot
          tone={settings.activeVersion ? "ok" : "warn"}
          label={settings.activeVersion || "版本未选"}
          title={settings.activeVersion ? "当前 dsh 版本" : "还没选版本：到「版本与安装」安装并选用一个"}
        />
        <StatusDot
          tone={runningInstanceCount > 0 ? "ok" : "off"}
          label={`${runningInstanceCount} 个实例运行中 · ${env.os}/${env.arch}`}
        />
        <Button
          size="sm"
          variant={terminalOpen ? "secondary" : "outline"}
          className="mt-1.5 h-6 w-full gap-1 text-[10.5px]"
          title="打开/收起终端面板：实例日志与安装任务"
          onClick={onToggleTerminal}
        >
          <Terminal className="size-3" /> 终端
          {runningInstanceCount > 0 && (
            <span className="ml-0.5 rounded-full bg-emerald-500/15 px-1 font-mono text-[10px] text-emerald-600 dark:text-emerald-400">
              {runningInstanceCount}
            </span>
          )}
        </Button>
      </div>

      <Separator className="my-0.5 bg-sidebar-border" />

      <div className="eyebrow">目录与源</div>
      <div className="space-y-0.5">
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

      <p className="text-[10px] leading-relaxed text-muted-foreground">
        退出启动器只结束子进程实例，独立进程实例继续运行；关闭窗口最小化到托盘
      </p>
    </>
  );

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

        {/* 命令面板入口：汇总全部页面跳转与高频操作，键盘 ⌘K / Ctrl+K 同效 */}
        <SidebarGroup className="pt-0">
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  tooltip="命令面板"
                  className="h-8 gap-2 rounded-lg border border-sidebar-border bg-sidebar-accent/40 text-muted-foreground hover:text-foreground"
                  onClick={onOpenPalette}
                >
                  <Command />
                  <span>命令面板</span>
                  <kbd className="ml-auto rounded border border-border bg-muted px-1 font-mono text-[10px] leading-4 text-muted-foreground group-data-[collapsible=icon]:hidden">
                    {PALETTE_KEY}
                  </kbd>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter className="gap-2 border-t border-sidebar-border p-2">
        {/* 展开态：运行状态收成一个按钮，点击后在侧栏旁弹出详情 Popover；
            项目仓库卡片保持常驻。 */}
        <div className="space-y-2 group-data-[collapsible=icon]:hidden">
          <Popover>
            <PopoverTrigger asChild>
              <Button
                size="sm"
                variant="outline"
                className="h-7 w-full gap-1.5 border-sidebar-border bg-sidebar-accent/40 text-[10.5px] font-normal shadow-none"
                title="查看运行状态与环境详情"
              >
                <Gauge className="size-3" /> 运行状态
                {runningInstanceCount > 0 && (
                  <span className="ml-auto rounded-full bg-emerald-500/15 px-1 font-mono text-[10px] text-emerald-600 dark:text-emerald-400">
                    {runningInstanceCount}
                  </span>
                )}
              </Button>
            </PopoverTrigger>
            <PopoverContent side="right" align="start" className="w-72">
              {envDetails}
            </PopoverContent>
          </Popover>

          <Card size="sm" className="gap-0 border-sidebar-border bg-sidebar-accent/40 py-2 shadow-none ring-sidebar-border">
            <div className="flex items-center gap-2 px-2.5">
              <GitHubMark className="size-3.5 shrink-0 text-muted-foreground" />
              <div className="min-w-0 flex-1">
                <div className="truncate font-mono text-[11px] font-semibold">Yinxe/dsh-starter</div>
                <div className="truncate text-[10px] text-muted-foreground">源码 · 问题反馈 · 更新日志 · 下载页</div>
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
              <Button
                size="sm"
                variant="outline"
                className="h-6 gap-1 px-1.5 text-[10.5px]"
                title="检查 DSH Starter 新版本"
                onClick={onCheckUpdate}
              >
                <RefreshCw className="size-3" /> 检查更新
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="h-6 gap-1 px-1.5 text-[10.5px]"
                title={SITE_URL}
                onClick={() => openUrl(SITE_URL, "下载页")}
              >
                <Download className="size-3" /> 打开下载页
              </Button>
            </div>
            {/* QQ 群没有可跳转的加群链接（客户端内搜索/扫码为准），点击复制群号最实用 */}
            <div className="mt-1.5 px-2.5">
              <Button
                size="sm"
                variant="outline"
                className="h-6 w-full gap-1 px-1.5 text-[10.5px]"
                title={`复制 QQ 群号，在 QQ 里搜群号 ${QQ_GROUP} 加群`}
                onClick={() => copyPath(QQ_GROUP, "QQ 群号")}
              >
                <MessageSquareText className="size-3" /> QQ 群 {QQ_GROUP}
              </Button>
            </div>
          </Card>
        </div>

        {/* 收起态：状态灯收成一个按钮（保留灯做概览），点击同样弹出详情 Popover；
            实例终端 / 仓库入口保持独立按钮（tooltip 兜底） */}
        <div className="hidden flex-col items-center gap-2 py-1 group-data-[collapsible=icon]:flex">
          <Popover>
            <PopoverTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                className="h-auto flex-col gap-1.5 py-1.5"
                title="查看运行状态与环境详情"
              >
                <span
                  className={`led ${env.node ? "bg-emerald-500" : "bg-red-500"}`}
                  title={env.node ? `Node v${env.node}` : "Node 未装"}
                />
                <span
                  className={`led ${env.npm ? "bg-emerald-500" : env.npmPath ? "bg-amber-500" : "bg-red-500"}`}
                  title={env.npm ? `npm ${env.npm}` : env.npmPath ? "npm 不可用" : "npm 未装"}
                />
                <span
                  className={`led ${settings.activeVersion ? "bg-emerald-500" : "bg-amber-500"}`}
                  title={settings.activeVersion || "版本未选"}
                />
                <span
                  className={`led ${runningInstanceCount > 0 ? "bg-emerald-500" : "bg-muted-foreground/40"}`}
                  title={`${runningInstanceCount} 个实例运行中`}
                />
              </Button>
            </PopoverTrigger>
            <PopoverContent side="right" align="start" className="w-72">
              {envDetails}
            </PopoverContent>
          </Popover>
          <Button
            variant={terminalOpen ? "secondary" : "ghost"}
            size="icon-sm"
            title="打开/收起终端面板：实例日志与安装任务"
            onClick={onToggleTerminal}
          >
            <Terminal className="size-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            className="mt-0.5"
            title={`项目仓库：${REPO_URL}`}
            onClick={() => openUrl(REPO_URL, "项目仓库")}
          >
            <GitHubMark className="size-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            className="mt-0.5"
            title={`QQ 群 ${QQ_GROUP}（点击复制群号）`}
            onClick={() => copyPath(QQ_GROUP, "QQ 群号")}
          >
            <MessageSquareText className="size-3.5" />
          </Button>
        </div>
      </SidebarFooter>

      {/* 边缘拖拽热区：点一下也能折叠/展开（抽屉态不需要，且会探出视口 8px） */}
      {!isMobile && <SidebarRail />}
    </Sidebar>
  );
}
