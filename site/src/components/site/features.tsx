import type { ReactNode } from "react";
import {
  ActivityIcon,
  BoxesIcon,
  FolderTreeIcon,
  GaugeIcon,
  PackageCheckIcon,
  RefreshCwIcon,
  Settings2Icon,
  TerminalIcon,
  WifiIcon,
  type LucideIcon,
} from "lucide-react";
import { cn } from "cn";

import { Button } from "@/components/ui/button";
import { GithubMark as GithubLogo } from "@/components/site/icons";
import { Reveal } from "@/components/site/reveal";
import { SectionHeading } from "@/components/site/section-heading";
import { CAPABILITY_GROUPS } from "@/lib/capabilities";
import { CHANGELOG_URL, REPO_URL } from "@/lib/release";

interface Cell {
  index: string;
  title: string;
  body: ReactNode;
  icon: LucideIcon;
  /** 宽格留给需要解释机制的条目，形成宽窄混排（每行合计 3 列） */
  span?: 2;
}

/*
 * 核心机制：只写「为什么这么做」和边界（例如插件管理一律走官方 dsh plugin、
 * 加速前缀永不写进 origin）—— 这些边界才是值不值得装的判断依据。
 * 「是什么」的完整清单在下面的「全部能力」里，两处不要写成同一句话。
 */
const CELLS: Cell[] = [
  {
    index: "01",
    icon: PackageCheckIcon,
    span: 2,
    title: "插件管理，全部走官方 dsh plugin",
    body: (
      <>
        安装、卸载、升级都回到官方命令（<code className="num text-[12px]">dsh plugin add / remove</code>），
        启动器从不改写 profile 的 <code className="num text-[12px]">package.json</code> 或{" "}
        <code className="num text-[12px]">dsh.profile.bundles</code>。装完立刻核对新增包：缺清单、
        没有可加载入口、loader entry id 冲突都会当场卸掉并说明原因；卸载前还会查你自己的{" "}
        <code className="num text-[12px]">cordis.patch.yml</code> 有没有引用它。
      </>
    ),
  },
  {
    index: "02",
    icon: ActivityIcon,
    title: "每 profile 一个实例",
    body: <>内嵌启动与系统终端启动受同一约束：同一个 profile 不会跑出两份实例去抢端口。</>,
  },
  {
    index: "03",
    icon: TerminalIcon,
    title: "内嵌实例 + 实时日志",
    body: <>「启动」把 dsh 作为子进程跑，日志逐行进底部面板（多实例 tab、运行时长、一键停止）；关掉启动器，子进程随之回收。</>,
  },
  {
    index: "04",
    icon: Settings2Icon,
    span: 2,
    title: "模型与凭据，只改该改的",
    body: (
      <>
        结构化编辑 providers 与默认模型（Provider → 模型 → 思考等级三级联动）；保存只重写这两节，
        <code className="num text-[12px]">settings.yaml</code> 里其余内容与节外注释逐字节保留，写前自动备份。
        凭据值只在详情弹窗里可见，文件权限自动收紧到仅本用户可读写。
      </>
    ),
  },
  {
    index: "05",
    icon: WifiIcon,
    span: 2,
    title: "GitHub 加速，永不写进 origin",
    body: <>内置常用前缀 + 首次使用自动测速（下载与 git 分开测，因为代理放不放行 git 会随时段变化）。加速只注入进程内的 git 配置 —— 代理失效只会退回直连，不会像手改 remote 那样把克隆永久卡死。</>,
  },
  {
    index: "06",
    icon: BoxesIcon,
    title: "内置 Node + 首次引导",
    body: <>机器上没有 Node 也能用：一键把 Node LTS 装进启动器自己的目录（免 root、走国内镜像），再一键完成 dsh 初始化。</>,
  },
  {
    index: "07",
    icon: RefreshCwIcon,
    span: 2,
    title: "应用内自更新，签名说了算",
    body: <>启动时读远端清单，有新版本弹横幅（版本号 + 本版说明）→ 你点「下载并安装」才下载、验签、安装并重启。改过一个字节的包装不上；AppImage 原位替换，deb / rpm 弹授权窗口，Windows 交给安装器。</>,
  },
  {
    index: "08",
    icon: FolderTreeIcon,
    title: "分类日志 + 诊断包",
    body: <>9 个子系统分文件、单文件超 1MB 自动滚动；「生成诊断包」把环境摘要、设置（凭据已脱敏）与日志合成一个文件。</>,
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
          title="它管 dsh 的周边，不动 dsh 本身"
          lead="启动器不重写 dsh 的任何能力：装哪个版本、用哪个 profile、装哪些插件、连哪个模型、日志去哪儿看，这些周边事由它办完；dsh 仍然是那个命令行工具。每一条都写了它的边界。"
        />
      </Reveal>

      {/* ── 核心机制：8 个亮点，宽窄混排 ───────────── */}
      <Reveal delay={80} className="mt-10">
        <h3 className="eyebrow text-foreground/70">核心机制</h3>
        <div className="panel mt-4 overflow-hidden">
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
                <h4 className="text-[15px] font-medium">{cell.title}</h4>
                <p className="max-w-[64ch] text-[13px] leading-[1.75] text-muted-foreground">{cell.body}</p>
              </article>
            ))}

            {/* 收尾：占满一行，把「然后呢」直接给出来 */}
            <article className="flex flex-col justify-between gap-5 bg-muted/30 p-5 sm:col-span-2 sm:flex-row sm:items-center sm:p-6 lg:col-span-3">
              <div>
                <span className="eyebrow text-primary">→</span>
                <h4 className="mt-2.5 text-[15px] font-medium">源码、更新日志与反馈</h4>
                <p className="mt-2 max-w-[70ch] text-[13px] leading-[1.75] text-muted-foreground">
                  每个版本的用户可见变化都写在 CHANGELOG 里 —— GitHub Release 正文与客户端「发现新版本」弹窗都由它派生。
                </p>
              </div>
              <div className="flex shrink-0 flex-wrap gap-2">
                <Button asChild variant="outline" size="sm">
                  <a href={REPO_URL} target="_blank" rel="noreferrer noopener">
                    <GithubLogo className="size-3.5" />
                    源码
                  </a>
                </Button>
                <Button asChild variant="ghost" size="sm" className="text-muted-foreground">
                  <a href={CHANGELOG_URL} target="_blank" rel="noreferrer noopener">
                    更新日志
                  </a>
                </Button>
              </div>
            </article>
          </div>
        </div>
      </Reveal>

      {/* ── 全部能力：按领域铺开的规格表 ───────────── */}
      <Reveal delay={120} className="mt-12">
        <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
          <h3 className="eyebrow text-foreground/70">全部能力</h3>
          <span className="text-[11.5px] text-muted-foreground">
            按领域列全，逐条对应 README 的功能说明
          </span>
        </div>
        <div className="mt-4 grid gap-x-12 gap-y-9 border-t border-hairline pt-8 sm:grid-cols-2 lg:grid-cols-3">
          {CAPABILITY_GROUPS.map((group) => (
            <div key={group.id}>
              <h4 className="eyebrow text-primary">{group.label}</h4>
              <ul className="mt-3.5 space-y-2.5">
                {group.items.map((item) => (
                  <li key={item.title} className="text-[12.5px] leading-[1.75] text-muted-foreground">
                    <span className="text-foreground">{item.title}</span>
                    <span className="mx-1.5 text-muted-foreground/45">·</span>
                    {item.detail}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      </Reveal>

      <Reveal delay={160} className="mt-8 flex flex-wrap items-center gap-x-6 gap-y-2 text-[12.5px] text-muted-foreground">
        <span className="flex items-center gap-2">
          <GaugeIcon className="size-3.5" />
          Tauri 2（Rust）+ React 18 + TypeScript
        </span>
        <span>安装包 5 – 86 MB（按平台）</span>
        <span>三个平台同一套界面</span>
      </Reveal>
    </section>
  );
}
