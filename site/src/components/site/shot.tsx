import { BoxesIcon, PanelTopIcon, RocketIcon } from "lucide-react";

import { Reveal } from "@/components/site/reveal";
import { SectionHeading } from "@/components/site/section-heading";
import preview from "../../../../images/preview.png";

/**
 * 界面截图。
 *
 * 图片直接引用仓库根目录的 images/preview.png（README 用的是同一张），
 * 不复制副本 —— 截图更新一次，两处都跟着变。
 */
export function Shot() {
  return (
    <section className="mx-auto max-w-[1180px] px-5 py-20 sm:px-8 sm:py-24">
      <Reveal>
        <SectionHeading
          index="05"
          kicker="界面"
          title="没有仪表盘，只有你下一步要点的地方"
          lead="三个主页面：版本与安装、Profile 实例、插件管理。侧栏底部常驻环境状态与数据目录，日志面板贴在最下方，出问题不用另开终端。"
        />
      </Reveal>

      <Reveal delay={80} className="mt-10">
        <figure className="panel overflow-hidden">
          <div className="flex items-center gap-3 border-b border-hairline px-4 py-2.5">
            <span className="flex gap-1.5" aria-hidden="true">
              <span className="size-2.5 rounded-full bg-muted-foreground/25" />
              <span className="size-2.5 rounded-full bg-muted-foreground/25" />
              <span className="size-2.5 rounded-full bg-muted-foreground/25" />
            </span>
            <span className="num text-[11.5px] text-muted-foreground">DSH Launcher — 版本与安装</span>
            <span className="eyebrow ml-auto hidden sm:inline">深色主题</span>
          </div>
          <img
            src={preview}
            alt="DSH Launcher 的「版本与安装」界面：左侧侧栏列出环境状态与数据目录，主区是版本列表与安装按钮，底部是日志面板。"
            width={1383}
            height={846}
            loading="lazy"
            decoding="async"
            className="block w-full"
          />
        </figure>
      </Reveal>

      <Reveal delay={120} className="mt-4 grid gap-4 sm:grid-cols-3">
        {[
          {
            icon: RocketIcon,
            title: "版本与安装",
            body: "官方版本列表、一键安装/卸载/切换，安装过程的 fetch/resolve/extract 实时打印。",
          },
          {
            icon: PanelTopIcon,
            title: "Profile 实例",
            body: "选 profile 启动，实例日志、运行时长、web 地址与「打开日志」都在一张卡里。",
          },
          {
            icon: BoxesIcon,
            title: "插件管理",
            body: "npm 包 / 链接直装 / clone + link 三种装法，装后校验，内置终端逐行实时输出。",
          },
        ].map((item) => (
          <div key={item.title} className="panel p-5">
            <item.icon className="size-4 text-primary" />
            <h3 className="mt-3 text-[14px] font-medium">{item.title}</h3>
            <p className="mt-1.5 text-[12.5px] text-muted-foreground">{item.body}</p>
          </div>
        ))}
      </Reveal>
    </section>
  );
}
