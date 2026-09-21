import { ExternalLinkIcon } from "lucide-react";

import { GithubMark } from "@/components/site/icons";
import { WhaleMark } from "@/components/site/whale";
import { formatDate } from "@/lib/format";
import {
  CHANGELOG_URL,
  R2_MANIFEST_URL,
  REPO_URL,
  type ReleaseState,
  type SourceStatus,
} from "@/lib/release";

/** 页脚只放最短的状态词，细节在首屏那张「实时清单」卡里 */
function shortStatus(status: SourceStatus): string {
  switch (status) {
    case "probing":
      return "探测中";
    case "readable":
    case "reachable":
      return "在线";
    case "down":
      return "不可达";
  }
}

export function Footer({ state }: { state: ReleaseState }) {
  const year = new Date().getFullYear();

  const columns: { title: string; links: { label: string; href: string; external?: boolean }[] }[] = [
    {
      title: "项目",
      links: [
        { label: "源码仓库", href: REPO_URL, external: true },
        { label: "问题反馈", href: `${REPO_URL}/issues`, external: true },
        { label: "更新日志", href: CHANGELOG_URL, external: true },
        { label: "全部发布", href: `${REPO_URL}/releases`, external: true },
      ],
    },
    {
      title: "资源",
      links: [
        { label: "自建源清单 latest.json", href: R2_MANIFEST_URL, external: true },
        { label: "发布流程文档", href: `${REPO_URL}/blob/main/docs/RELEASING.md`, external: true },
        {
          label: "dsh（npm）",
          href: "https://www.npmjs.com/package/@deepseek-ai/dsh",
          external: true,
        },
      ],
    },
  ];

  return (
    <footer className="mt-8 border-t border-hairline">
      <div className="mx-auto max-w-[1180px] px-5 py-12 sm:px-8">
        <div className="grid gap-10 lg:grid-cols-12">
          <div className="lg:col-span-5">
            <div className="flex items-center gap-2.5">
              <WhaleMark className="size-[22px] text-primary" />
              <span className="text-display text-[12.5px] tracking-[0.22em] uppercase">DSH Starter</span>
            </div>
            <p className="mt-4 max-w-[40ch] text-[13px] text-muted-foreground">
              DeepSeek Harness（@deepseek-ai/dsh）的跨平台图形启动器。
              这一页只是它的介绍与下载入口 —— 上面的版本号、体积与直链都是打开时现拉的。
            </p>
            <div className="mt-5 flex items-center gap-2">
              <a
                href={REPO_URL}
                target="_blank"
                rel="noreferrer noopener"
                className="flex items-center gap-2 rounded-md border border-border px-2.5 py-1.5 text-[12.5px] text-muted-foreground transition-colors hover:border-primary/40 hover:text-foreground"
              >
                <GithubMark className="size-3.5" />
                Yinxe/dsh-starter
              </a>
            </div>
          </div>

          {columns.map((col) => (
            <nav key={col.title} className="lg:col-span-3">
              <h3 className="eyebrow">{col.title}</h3>
              <ul className="mt-4 space-y-2.5">
                {col.links.map((link) => (
                  <li key={link.label}>
                    <a
                      href={link.href}
                      target={link.external ? "_blank" : undefined}
                      rel={link.external ? "noreferrer noopener" : undefined}
                      className="group/lnk flex items-center gap-1.5 text-[13px] text-muted-foreground transition-colors hover:text-foreground"
                    >
                      {link.label}
                      {link.external ? (
                        <ExternalLinkIcon className="size-3 opacity-0 transition-opacity group-hover/lnk:opacity-60" />
                      ) : null}
                    </a>
                  </li>
                ))}
              </ul>
            </nav>
          ))}
        </div>

        <div className="mt-12 flex flex-wrap items-center gap-x-5 gap-y-2 border-t border-hairline pt-6 text-[11.5px] text-muted-foreground">
          <span>© {year} DSH Starter</span>
          <span className="num">
            {state.version ? `v${state.version}` : "版本未知"}
            {state.publishedAt ? ` · ${formatDate(state.publishedAt)}` : ""}
          </span>
          <span className="hidden sm:inline">第三方启动器，dsh 本体由 @deepseek-ai/dsh 提供</span>
          <span className="num ml-auto">
            {`自建源${shortStatus(state.r2)}`}
            {" · "}
            {`GitHub ${shortStatus(state.github)}`}
          </span>
        </div>
      </div>
    </footer>
  );
}
