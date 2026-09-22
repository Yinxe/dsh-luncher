import { BarChart3Icon, PanelTopIcon, RocketIcon } from "lucide-react";

import { Reveal } from "@/components/site/reveal";
import { SectionHeading } from "@/components/site/section-heading";
import preview from "../../../../images/preview.png";
import preview2 from "../../../../images/preview2.png";
import preview3 from "../../../../images/preview3.png";

/**
 * 界面截图。
 *
 * 图片直接引用仓库根目录的 images/（README 用的是同一批），不复制副本 ——
 * 截图更新一次，两处都跟着变。
 *
 * 排版上让图**比正文容器更宽**（1180 → 1320）：整页都是同一个宽度的栏，
 * 这里破一次格，视线会自然停在这张图上。用「更宽的容器」而不是负 margin，
 * 窄屏才不会顶出视口。
 */
const SHOTS = [
  {
    src: preview,
    label: "DSH Starter — 版本与安装",
    alt: "DSH Starter 的「版本与安装」界面：左侧侧栏列出环境状态与数据目录，主区是版本列表与安装按钮，底部是日志面板。",
    width: 2556,
    height: 1380,
  },
  {
    src: preview2,
    label: "DSH Starter — Profile 实例 + Web 独立窗口",
    alt: "「Profile 实例」页旁并排开着两个应用内独立窗口，各自承载一个实例的 DSH Web 界面，没有浏览器地址栏。",
    width: 2543,
    height: 1378,
  },
  {
    src: preview3,
    label: "DSH Starter — 统计",
    alt: "「统计」页：Token 活动热力图按周铺色，下方是模型用量分布环形图与各模型的输入 / 输出 / 缓存明细。",
    width: 2556,
    height: 1380,
  },
];
export function Shot() {
  return (
    <section className="py-20 sm:py-24">
      <div className="mx-auto max-w-[1180px] px-5 sm:px-8">
        <Reveal>
          <SectionHeading
            index="05"
            kicker="界面"
            title="界面长这样"
            lead="三张深色主题截图：「版本与安装」主页面、Profile 实例旁用独立窗口直接开 Web 界面、以及统计页的 Token 热力图与模型分布 —— 右上角一键切亮色。"
          />
        </Reveal>
      </div>

      <div className="mx-auto mt-10 max-w-[1320px] space-y-8 px-5 sm:px-8">
        {SHOTS.map((shot, i) => (
          <Reveal key={shot.label} delay={80 + i * 40}>
            <figure className="panel overflow-hidden p-0">
              <div className="flex items-center gap-3 border-b border-hairline px-4 py-2.5">
                <span className="flex gap-1.5" aria-hidden="true">
                  <span className="size-2.5 rounded-full bg-muted-foreground/25" />
                  <span className="size-2.5 rounded-full bg-muted-foreground/25" />
                  <span className="size-2.5 rounded-full bg-muted-foreground/25" />
                </span>
                <span className="num text-[11.5px] text-muted-foreground">{shot.label}</span>
                <span className="eyebrow ml-auto hidden sm:inline">深色主题</span>
              </div>
              <img
                src={shot.src}
                alt={shot.alt}
                width={shot.width}
                height={shot.height}
                loading="lazy"
                decoding="async"
                className="block w-full"
              />
            </figure>
          </Reveal>
        ))}
      </div>

      <div className="mx-auto mt-8 max-w-[1180px] px-5 sm:px-8">
        <Reveal delay={120} className="grid gap-x-12 gap-y-6 border-t border-hairline pt-8 lg:grid-cols-3">
          {[
            {
              icon: RocketIcon,
              title: "版本与安装",
              body: "官方版本列表、一键安装 / 卸载 / 切换，安装过程的 fetch、resolve、extract 实时打印在卡里。",
            },
            {
              icon: PanelTopIcon,
              title: "Profile 实例",
              body: "选 profile 启动，实例日志、运行时长、web 地址与「打开日志」都在一张卡里；Web 界面直接在应用内独立窗口打开，多实例并排用。",
            },
            {
              icon: BarChart3Icon,
              title: "统计",
              body: "本机 dsh 的 Token 用量与在线时长：按日堆叠趋势、活动热力图、模型用量分布，一键导出分享报表 PNG。",
            },
          ].map((item) => (
            <div key={item.title}>
              <div className="flex items-center gap-2">
                <item.icon className="size-4 text-primary" />
                <h3 className="text-[14px] font-medium">{item.title}</h3>
              </div>
              <p className="mt-1.5 max-w-[46ch] text-[12.5px] text-muted-foreground">{item.body}</p>
            </div>
          ))}
        </Reveal>
      </div>
    </section>
  );
}
