import type { ReactNode } from "react";
import {
  ActivityIcon,
  BookOpenIcon,
  BoxesIcon,
  BugIcon,
  FolderTreeIcon,
  GaugeIcon,
  LayoutGridIcon,
  PackageCheckIcon,
  PanelTopIcon,
  RefreshCwIcon,
  RocketIcon,
  Settings2Icon,
  TerminalIcon,
  WifiIcon,
  type LucideIcon,
} from "lucide-react";
import { cn } from "cn";

import { Button } from "@/components/ui/button";
import { GithubMark } from "@/components/site/icons";
import { Reveal } from "@/components/site/reveal";
import { SectionHeading } from "@/components/site/section-heading";
import { CHANGELOG_URL, REPO_URL } from "@/lib/release";

interface Cell {
  index: string;
  title: string;
  body: ReactNode;
  icon: LucideIcon;
  span?: 1 | 2;
}

/*
 * 功能清单。
 *
 * 这里只写「用户能看到的行为」和它的边界（例如插件管理一律走官方 dsh plugin、
 * 加速前缀永不写进 origin），不写实现栈。宽格留给需要解释的机制。
 */
const CELLS: Cell[] = [
  {
    index: "01",
    icon: LayoutGridIcon,
    title: "官方版本列表",
    body: <>直接拉 npm registry 上 @deepseek-ai/dsh 的全部版本，带 latest / next / alpha 渠道、发布日期与体积，可切镜像源；已装版本同时从启动器目录、npm 全局与 PATH 三个来源识别。</>,
  },
  {
    index: "02",
    icon: TerminalIcon,
    title: "内嵌实例 + 实时日志",
    body: <>「启动」把 dsh 作为启动器的子进程跑，日志逐行进底部面板（多实例 tab、运行时长、一键停止）；退出启动器，子进程随之被回收。</>,
  },
  {
    index: "03",
    icon: PanelTopIcon,
    title: "每 profile 一个实例",
    body: <>内嵌启动与系统终端启动受同一个约束：同一个 profile 不会同时跑出两份实例去抢端口。</>,
  },
  {
    index: "04",
    icon: PackageCheckIcon,
    span: 2,
    title: "插件管理，全部走官方 dsh plugin",
    body: (
      <>
        安装、卸载、升级都回到官方命令（<code className="num text-[12px]">dsh plugin add / remove</code>），
        启动器从不改写 profile 的{" "}
        <code className="num text-[12px]">package.json</code> 或{" "}
        <code className="num text-[12px]">dsh.profile.bundles</code>。
        装完立刻核对新增包：缺清单、没有可加载入口、loader entry id 冲突都会当场卸掉并说明原因；
        卸载前先查你自己的 <code className="num text-[12px]">cordis.patch.yml</code> 是否还引用它。
        内置终端逐行实时输出，可一键取消（连派生的孙进程一起收）。
      </>
    ),
  },
  {
    index: "05",
    icon: Settings2Icon,
    title: "模型与凭据",
    body: <>结构化编辑 providers 与默认模型（Provider → 模型 → 思考等级三级联动），保存只重写这两节，文件其余内容与注释逐字节保留，写前自动备份。</>,
  },
  {
    index: "06",
    icon: RocketIcon,
    title: "首次使用引导",
    body: <>全新机器上 dsh 数据目录还不存在时，profile 列表必然是空的。引导卡带 Node / dsh / npm 就绪状态，一键完成初始化。</>,
  },
  {
    index: "07",
    icon: BoxesIcon,
    title: "内置 Node 运行时",
    body: <>机器上没有 Node 也能用：一键下载 Node LTS 到启动器自己的数据目录，免 root、走国内镜像，不污染系统。</>,
  },
  {
    index: "08",
    icon: WifiIcon,
    title: "GitHub 加速",
    body: <>内置常用前缀并首次使用自动测速（下载与 git 分开测）。加速只注入进程内的 git 配置，永不写进仓库 origin —— 代理失效只退回直连，不会把克隆卡死。</>,
  },
  {
    index: "09",
    icon: ActivityIcon,
    title: "托盘与单实例",
    body: <>常驻托盘，关闭默认最小化；启动器自身只允许跑一个，重复双击会把已有窗口唤回前台，而不是开出第二个托盘图标。</>,
  },
  {
    index: "10",
    icon: FolderTreeIcon,
    span: 2,
    title: "分类日志与诊断包",
    body: (
      <>
        日志按 9 个子系统分文件落在{" "}
        <code className="num text-[12px]">~/.dsh-launcher/logs/</code>
        （实例、安装、插件、网络、UI、崩溃…），单文件超 1MB 自动滚动。
        报 bug 时用「生成诊断包」：环境摘要 + 设置（<strong className="text-foreground">凭据已脱敏</strong>）+ 日志尾部合并成一个文件，发一个文件就够。
      </>
    ),
  },
  {
    index: "11",
    icon: RefreshCwIcon,
    span: 2,
    title: "应用内自更新",
    body: <>启动时读取远端清单，发现新版本弹横幅（版本号 + 本版更新说明）→ 你点「下载并安装」才下载、验签、安装并重启。AppImage 原位替换，deb/rpm 弹授权窗口，Windows 交给安装器。</>,
  },
];

export function Features() {
  return (
    <section className="mx-auto max-w-[1180px] px-5 py-20 sm:px-8 sm:py-24">
      <Reveal>
        <SectionHeading
          index="04"
          kicker="功能"
          id="features"
          title="它到底替你管了什么"
          lead="一句话：把 dsh 从「一个 npm 包」变成「一个装好就能用的程序」——版本、实例、插件、配置、日志、更新，都不用你自己拼。"
        />
      </Reveal>

      <Reveal delay={80} className="mt-10">
        <div className="panel overflow-hidden">
          <div className="grid gap-px bg-hairline sm:grid-cols-2 lg:grid-cols-3">
            {CELLS.map((cell) => (
              <article
                key={cell.index}
                className={cn(
                  "group/cell relative flex flex-col gap-2.5 bg-card p-5 transition-colors hover:bg-muted/25 sm:p-6",
                  cell.span === 2 && "sm:col-span-2",
                )}
              >
                <div className="flex items-center justify-between gap-3">
                  <span className="eyebrow text-primary">{cell.index}</span>
                  <cell.icon className="size-4 text-muted-foreground/60 transition-colors group-hover/cell:text-muted-foreground" />
                </div>
                <h3 className="text-[15px] font-medium">{cell.title}</h3>
                <p className="max-w-[62ch] text-[13px] leading-[1.75] text-muted-foreground">{cell.body}</p>
              </article>
            ))}

            {/* 收尾格：把「然后呢」直接给出来，别让人回滚到页脚找链接 */}
            <article className="flex flex-col justify-between gap-4 bg-card p-5 sm:p-6">
              <div>
                <div className="flex items-center justify-between gap-3">
                  <span className="eyebrow text-primary">12</span>
                  <BookOpenIcon className="size-4 text-muted-foreground/60" />
                </div>
                <h3 className="mt-2.5 text-[15px] font-medium">源码、更新日志与反馈</h3>
                <p className="mt-2 text-[13px] leading-[1.75] text-muted-foreground">
                  每个版本的用户可见变化都写在 CHANGELOG 里，Release 正文与客户端更新弹窗都由它派生。
                </p>
              </div>
              <div className="flex flex-wrap gap-2">
                <Button asChild variant="outline" size="sm">
                  <a href={REPO_URL} target="_blank" rel="noreferrer noopener">
                    <GithubMark className="size-3.5" />
                    源码
                  </a>
                </Button>
                <Button asChild variant="ghost" size="sm" className="text-muted-foreground">
                  <a href={CHANGELOG_URL} target="_blank" rel="noreferrer noopener">
                    <BugIcon className="size-3.5" />
                    更新日志
                  </a>
                </Button>
              </div>
            </article>
          </div>
        </div>
      </Reveal>

      <Reveal delay={120} className="mt-4 flex flex-wrap items-center gap-x-6 gap-y-2 text-[12.5px] text-muted-foreground">
        <span className="flex items-center gap-2">
          <GaugeIcon className="size-3.5" />
          技术栈：Tauri 2（Rust）+ React 18 + TypeScript
        </span>
        <span>安装体积 5～86 MB（按平台）</span>
        <span>Windows · macOS · Linux 同一套界面</span>
      </Reveal>
    </section>
  );
}
