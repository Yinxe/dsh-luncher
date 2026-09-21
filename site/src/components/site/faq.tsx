import { ExternalLinkIcon } from "lucide-react";

import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from "@/components/ui/accordion";
import { Button } from "@/components/ui/button";
import { Reveal } from "@/components/site/reveal";
import { SectionEyebrow } from "@/components/site/section-heading";
import { REPO_URL, type ReleaseState } from "@/lib/release";

/**
 * 常见问题：左侧悬挂标题 + 右侧问题列表（两栏）。
 * 不用卡片包一层 —— 手风琴自带分隔线与展开状态，再套一层框只是多一层噪音。
 */
export function Faq({ state }: { state: ReleaseState }) {
  const v = state.version ? `v${state.version}` : "当前版本";

  const items: { q: string; a: React.ReactNode }[] = [
    {
      q: "要先装 Node.js 吗？",
      a: (
        <>
          不必须。启动器自带探测：机器上已有 Node / npm 会直接用；一个都没有时，设置里可以一键把 Node LTS
          下载到自己的数据目录（免 root、走国内镜像），后续 dsh 版本与插件都跑在这份运行时上。
        </>
      ),
    },
    {
      q: "支持哪些系统？",
      a: (
        <>
          Windows 10 / 11（x64）、macOS 10.15+（通用二进制，Apple Silicon 与 Intel 同一个包）、
          主流的 glibc 发行版 Linux（x86_64）。Linux 托盘需要{" "}
          <code className="num text-[12px]">libayatana-appindicator3</code>（deb / rpm 会声明这个依赖）；
          极简发行版跑 AppImage 需要自备 FUSE。
        </>
      ),
    },
    {
      q: "会污染我的全局环境吗？",
      a: (
        <>
          不会。每个 dsh 版本装在{" "}
          <code className="num text-[12px]">~/.dsh-launcher/versions/&lt;版本&gt;</code> 的独立目录里，
          互不干扰，也不动全局 npm 前缀；卸载就是删目录。插件装在 profile 自己的{" "}
          <code className="num text-[12px]">node_modules</code> 里。
        </>
      ),
    },
    {
      q: "DeepSeek Harness 是什么？和这个启动器什么关系？",
      a: (
        <>
          dsh 是{" "}
          <a
            href="https://www.npmjs.com/package/@deepseek-ai/dsh"
            target="_blank"
            rel="noreferrer noopener"
            className="text-foreground underline decoration-border decoration-dotted underline-offset-4"
          >
            @deepseek-ai/dsh
          </a>
          （DeepSeek Harness CLI）。本启动器是社区做的第三方图形启动器：负责把它装好、拉起来，并管住 profile、
          插件与模型配置。所有插件与 profile 操作都调用 dsh 官方命令，不私改它的配置文件结构。
        </>
      ),
    },
    {
      q: "更新走哪条通道？会不会被投毒？",
      a: (
        <>
          默认自建源（Cloudflare R2），GitHub Release 作为兜底，设置里可二选一。安装包用内置公钥验签，
          签名不匹配直接拒绝安装 —— 即使地址被代理或镜像改写，也无法投毒。当前版本 {v}，
          本页顶部的读数条能看到两条源此刻各自是否可达。
        </>
      ),
    },
    {
      q: "macOS 说「无法验证开发者」怎么办？",
      a: (
        <>
          应用没有做 Apple 公证。首次打开时在「系统设置 → 隐私与安全性」点「仍要打开」，或执行{" "}
          <code className="num text-[12px]">
            xattr -dr com.apple.quarantine "/Applications/DSH Launcher.app"
          </code>
          。上面的安装说明里也有这条。
        </>
      ),
    },
    {
      q: "数据都存在哪？怎么彻底删掉？",
      a: (
        <>
          启动器数据在 <code className="num text-[12px]">~/.dsh-launcher/</code>
          （设置、内置运行时、git 插件克隆、分类日志、各版本目录）；dsh 自己的数据仍在{" "}
          <code className="num text-[12px]">$DSH_HOME</code>（默认{" "}
          <code className="num text-[12px]">~/.dsh</code>），profile 与凭据都在那儿。
          删掉这两个目录就等于完全卸载（系统级安装包记得再用包管理器卸一次）。
        </>
      ),
    },
    {
      q: "怎么反馈问题？",
      a: (
        <>
          设置 →「生成诊断包」：它会合并环境摘要、设置（凭据已脱敏）与全部日志，发这一个文件到{" "}
          <a
            href={`${REPO_URL}/issues`}
            target="_blank"
            rel="noreferrer noopener"
            className="text-foreground underline decoration-border decoration-dotted underline-offset-4"
          >
            Issues
          </a>{" "}
          即可，不用逐个回答「node 装哪了」「npm 是哪个」。
        </>
      ),
    },
  ];

  return (
    <section className="mx-auto max-w-[1180px] px-5 py-20 sm:px-8 sm:py-24">
      <div className="grid gap-x-12 gap-y-8 lg:grid-cols-[300px_minmax(0,1fr)]">
        <Reveal className="lg:sticky lg:top-20 lg:self-start">
          <SectionEyebrow index="07" kicker="常见问题" />
          <h2 className="section-title mt-5">装之前想知道的那几件事</h2>
          <p className="mt-3 max-w-[32ch] text-[13.5px] text-muted-foreground">
            没覆盖到的疑问欢迎直接提 issue —— 带上「生成诊断包」的文件，能省掉一轮来回。
          </p>
          <Button asChild variant="outline" size="sm" className="mt-5">
            <a href={`${REPO_URL}/issues`} target="_blank" rel="noreferrer noopener">
              去提 issue
              <ExternalLinkIcon className="size-3.5" />
            </a>
          </Button>
        </Reveal>

        <Reveal delay={80}>
          <Accordion type="single" collapsible className="divide-y divide-hairline border-t border-hairline">
            {items.map((item, i) => (
              <AccordionItem key={item.q} value={`item-${i}`} className="border-hairline">
                <AccordionTrigger className="group/faq py-4 text-[14.5px] hover:no-underline">
                  <span className="flex items-baseline gap-3 pr-4">
                    <span className="num text-[11.5px] text-muted-foreground">
                      {String(i + 1).padStart(2, "0")}
                    </span>
                    <span className="group-hover/faq:text-primary">{item.q}</span>
                  </span>
                </AccordionTrigger>
                <AccordionContent className="pb-5">
                  <div className="max-w-[72ch] pl-8 text-[13.5px] leading-[1.85] break-words text-muted-foreground">
                    {item.a}
                  </div>
                </AccordionContent>
              </AccordionItem>
            ))}
          </Accordion>
        </Reveal>
      </div>
    </section>
  );
}
