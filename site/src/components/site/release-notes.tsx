import { useState, type ReactNode } from "react";
import { ChevronDownIcon, ExternalLinkIcon } from "lucide-react";
import { cn } from "cn";

import { Button } from "@/components/ui/button";
import { Reveal } from "@/components/site/reveal";
import { SectionHeading } from "@/components/site/section-heading";
import { formatDate } from "@/lib/format";
import { CHANGELOG_URL, type ReleaseState } from "@/lib/release";

/**
 * 本版更新说明。
 *
 * 文字来自自建源清单 latest.json 的 `notes` —— 与客户端「发现新版本」弹窗、
 * GitHub Release 正文是同一份（都由 CHANGELOG.md 派生）。所以这块不是页面作者写的
 * 摘要，而是发布流程的原文；这里只做排版。
 */
export function ReleaseNotes({ state }: { state: ReleaseState }) {
  const notes = state.notes?.trim();
  const [open, setOpen] = useState(false);

  if (!notes) return null;

  const long = notes.length > 620;

  return (
    <section className="mx-auto max-w-[1180px] px-5 py-20 sm:px-8 sm:py-24">
      <Reveal>
        <SectionHeading
          index="06"
          kicker="更新日志"
          id="notes"
          title={state.version ? `v${state.version} 改了什么` : "本版改了什么"}
          lead={
            <>
              这段原文与客户端更新弹窗、GitHub Release 正文同源，都取自仓库的{" "}
              <a
                href={CHANGELOG_URL}
                target="_blank"
                rel="noreferrer noopener"
                className="text-foreground underline decoration-border decoration-dotted underline-offset-4"
              >
                CHANGELOG.md
              </a>
              ，写的是用户看得见的变化。
              {state.notesFrom === "github" ? "（本次取自 GitHub Release 正文）" : null}
            </>
          }
          action={
            state.publishedAt ? (
              <span className="num text-[11.5px] text-muted-foreground">
                发布于 {formatDate(state.publishedAt)}
              </span>
            ) : null
          }
        />
      </Reveal>

      <Reveal delay={80} className="mt-10">
        <div className="panel p-5 sm:p-7">
          <div className={cn("relative", long && !open && "max-h-[360px] overflow-hidden")}>
            <Notes md={notes} />
            {long && !open ? (
              <div className="pointer-events-none absolute inset-x-0 bottom-0 h-28 bg-gradient-to-t from-card to-transparent" />
            ) : null}
          </div>

          <div className="mt-6 flex flex-wrap items-center gap-3">
            {long ? (
              <Button type="button" variant="outline" size="sm" onClick={() => setOpen((v) => !v)}>
                <ChevronDownIcon className={cn("transition-transform", open && "rotate-180")} />
                {open ? "收起" : "展开全部"}
              </Button>
            ) : null}
            <Button asChild variant="ghost" size="sm" className="text-muted-foreground">
              <a href={CHANGELOG_URL} target="_blank" rel="noreferrer noopener">
                看完整更新历史
                <ExternalLinkIcon className="size-3.5" />
              </a>
            </Button>
          </div>
        </div>
      </Reveal>
    </section>
  );
}

/** 只认发布说明实际会用到的几种 Markdown：### 标题、- 列表、--- 分隔、行内粗体/链接/代码 */
function Notes({ md }: { md: string }) {
  const blocks: ReactNode[] = [];
  let list: string[] = [];
  let key = 0;

  const flush = () => {
    if (list.length === 0) return;
    const items = list;
    list = [];
    blocks.push(
      <ul key={`ul-${key++}`} className="mt-3 space-y-2.5">
        {items.map((item, i) => (
          <li key={i} className="relative pl-4 text-[13.5px] leading-[1.8] text-muted-foreground">
            <span aria-hidden="true" className="absolute left-0 top-[0.72em] size-1 rounded-full bg-primary/70" />
            <Inline text={item} />
          </li>
        ))}
      </ul>,
    );
  };

  for (const raw of md.split("\n")) {
    const line = raw.trim();
    if (!line) {
      flush();
      continue;
    }
    if (line.startsWith("- ")) {
      list.push(line.slice(2));
      continue;
    }
    flush();
    if (line.startsWith("#### ")) {
      blocks.push(
        <h4 key={`h4-${key++}`} className="mt-6 text-[13.5px] font-medium text-foreground">
          <Inline text={line.slice(5)} />
        </h4>,
      );
    } else if (line.startsWith("### ")) {
      blocks.push(
        <h3 key={`h3-${key++}`} className="eyebrow mt-7 block text-primary first:mt-0">
          {line.slice(4)}
        </h3>,
      );
    } else if (line.startsWith("## ")) {
      blocks.push(
        <h3 key={`h2-${key++}`} className="mt-7 text-[15px] font-medium first:mt-0">
          {line.slice(3)}
        </h3>,
      );
    } else if (/^-{3,}$/.test(line)) {
      blocks.push(<hr key={`hr-${key++}`} className="my-6 border-hairline" />);
    } else {
      blocks.push(
        <p key={`p-${key++}`} className="mt-3 text-[13.5px] leading-[1.8] text-muted-foreground first:mt-0">
          <Inline text={line} />
        </p>,
      );
    }
  }
  flush();

  return <div>{blocks}</div>;
}

/** 行内排版：**粗体**、[文字](链接)、`代码` —— 顺序敏感，一次扫描搞定 */
function Inline({ text }: { text: string }) {
  const out: ReactNode[] = [];
  const re = /(\*\*[^*]+\*\*)|(\[[^\]]+\]\([^)]+\))|(`[^`]+`)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let i = 0;

  while ((m = re.exec(text)) !== null) {
    if (m.index > last) out.push(text.slice(last, m.index));
    const token = m[0];
    if (token.startsWith("**")) {
      out.push(
        <strong key={i++} className="font-medium text-foreground">
          {token.slice(2, -2)}
        </strong>,
      );
    } else if (token.startsWith("[")) {
      const link = /\[([^\]]+)\]\(([^)]+)\)/.exec(token);
      if (link) {
        out.push(
          <a
            key={i++}
            href={link[2]}
            target="_blank"
            rel="noreferrer noopener"
            className="text-foreground underline decoration-border decoration-dotted underline-offset-4"
          >
            {link[1]}
          </a>,
        );
      } else {
        out.push(token);
      }
    } else {
      out.push(
        <code key={i++} className="num rounded bg-muted px-1.5 py-0.5 text-[12px] text-foreground">
          {token.slice(1, -1)}
        </code>,
      );
    }
    last = m.index + token.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return <>{out}</>;
}
