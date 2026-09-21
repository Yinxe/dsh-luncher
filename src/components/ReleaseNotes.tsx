import type { ReactNode } from "react";

/**
 * 发布说明渲染：把 CHANGELOG.md 的版本段落（Markdown 子集）渲染成统一的排版。
 *
 * 只支持实际会用到的语法：`###` 小标题、`- ` 列表、`---` 分隔线、`**加粗**`、
 * `` `代码` ``、`[文本](链接)`，其余按普通段落处理 —— 发布说明是给用户读的，
 * 不需要引入完整 Markdown 渲染器（那会多出几十 KB 依赖）。
 */

/** 从完整发布说明里抽一句纯文本摘要，给横幅用（不显示 Markdown 记号） */
export function notesTeaser(notes: string | null | undefined, maxItems = 2): string {
  if (!notes) return "";
  const items = notes
    .split(/\r?\n/)
    .filter((l) => /^\s*[-*]\s+\S/.test(l))
    .map((l) => stripInline(l.replace(/^\s*[-*]\s+/, "").trim()));
  const picked = items.slice(0, maxItems);
  if (!picked.length) return stripInline(notes.split(/\r?\n/).find((l) => l.trim()) ?? "");
  return picked.join("；") + (items.length > picked.length ? " …" : "");
}

/** 去掉行内 Markdown 记号，只留文字 */
function stripInline(text: string): string {
  return text
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/^\s*#+\s*/, "")
    .trim();
}

/** 语言锚点标记：`<!--@id:cn-0.1.6-alpha.2-->`，解析时用得到、渲染时不留痕 */
const ID_MARK = /^<!--@id:(.*?)-->$/;

export interface ParsedReleaseBody {
  /** 中文段（Markdown 子集） */
  zh: string | null;
  /** 英文段 */
  en: string | null;
  /** 正文末尾的 Full Changelog 对比链接 */
  compareUrl: string | null;
  /** 没有语言分段（或只有一段）时的正文 */
  plain: string;
}

/**
 * 解析 dsh 官方 Release 正文。
 *
 * 官方正文的形态不统一，实测有两种：`<h2 id="chinese">…</h2>` 分中英两段，
 * 或 `<h3 id="cn-0.1.6-alpha.2">新增功能</h3>` 只给第一段带锚点、后续小节是
 * 普通 Markdown 标题；两种都靠 `id` 里的 cn/zh/chinese、en/english 判语言。
 * 这里把 HTML 标题统一转成 Markdown 标题（本文件的渲染器只认 Markdown 子集），
 * 再按语言切段，顺带摘出「语言导航行」与「Full Changelog」链接 —— 两样都不是
 * 给读者看的内容，前者是 GitHub 页内跳转、后者要单独做成按钮。
 */
export function parseReleaseBody(body: string): ParsedReleaseBody {
  let text = (body ?? "").replace(/\r\n?/g, "\n");

  // 1. HTML 标题 → 段落标记 + Markdown 标题
  text = text.replace(/<h([1-6])\b([^>]*)>([\s\S]*?)<\/h\1>/gi, (_m, _lvl, attrs: string, inner: string) => {
    const id = /\bid\s*=\s*"([^"]*)"/i.exec(attrs)?.[1]
      ?? /\bid\s*=\s*'([^']*)'/i.exec(attrs)?.[1]
      ?? "";
    const title = inner.replace(/<[^>]+>/g, "").trim();
    return `\n<!--@id:${id}-->\n### ${title}\n`;
  });

  // 2. 语言导航行（`[中文](#cn-…) | [English](#en-…)`）与 Full Changelog 都摘掉
  const compareUrl =
    /^\s*(?:Full Changelog|完整变更(?:日志)?|完整更新日志)\s*[:：]\s*(\S+)\s*$/im.exec(text)?.[1] ?? null;
  text = text.replace(/^\s*(?:Full Changelog|完整变更(?:日志)?|完整更新日志)\s*[:：].*$/gim, "");
  text = text.replace(/^\s*(?:\[[^\]]+\]\(#[^)]*\)\s*\|?\s*)+$/gm, "");

  // 3. 按语言锚点切段：没有语言 id 的标题（小节标题、或自带 id 的小节）留在当前段里
  const buckets: { lang: "zh" | "en" | null; lines: string[] }[] = [{ lang: null, lines: [] }];
  for (const line of text.split("\n")) {
    const mark = ID_MARK.exec(line.trim());
    if (mark) {
      const id = mark[1].toLowerCase();
      const lang = /^(cn|zh|chinese)/.test(id) ? "zh" : /^(en|english)/.test(id) ? "en" : null;
      const cur = buckets[buckets.length - 1];
      const curHasText = cur.lines.some((l) => l.trim());
      if (lang && lang !== cur.lang && curHasText) {
        buckets.push({ lang, lines: [] });
      } else if (lang && !curHasText) {
        cur.lang = lang;
      }
      continue;
    }
    buckets[buckets.length - 1].lines.push(line);
  }

  const clean = (lines: string[]) =>
    lines
      .join("\n")
      .replace(/^\s*[-*_]{3,}\s*$/gm, "")  // 中英之间的分隔线不留在段尾
      .replace(/[ \t]+$/gm, "")
      .replace(/\n{3,}/g, "\n\n")
      .trim();

  // 有些版本的段落标题就是「0.1.3-alpha.2 · 中文」——版本号与语言在对话框里已经写了，
  // 留在正文里只是重复
  const stripLangTitle = (text: string) =>
    text.replace(/^#{1,6}\s*[^\n]*·\s*(?:中文|English)\s*\n+/, "");

  const zh = buckets.find((b) => b.lang === "zh");
  const en = buckets.find((b) => b.lang === "en");
  const zhText = zh ? stripLangTitle(clean(zh.lines)) : "";
  const enText = en ? stripLangTitle(clean(en.lines)) : "";
  const plain = stripLangTitle(clean(buckets.flatMap((b) => b.lines)));

  return {
    zh: zhText || null,
    en: enText || null,
    compareUrl,
    plain,
  };
}

/** 行内解析：**粗体** / `代码` / [链接](url) */
function renderInline(text: string, onOpenUrl?: (url: string) => void): ReactNode[] {
  const re = /(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\([^)]+\))/g;
  return text.split(re).filter(Boolean).map((part, i) => {
    if (/^\*\*[^*]+\*\*$/.test(part)) {
      return <b key={i} className="font-semibold text-foreground">{part.slice(2, -2)}</b>;
    }
    if (/^`[^`]+`$/.test(part)) {
      return (
        <code key={i} className="rounded bg-muted px-1 py-0.5 font-mono text-[12px]">
          {part.slice(1, -1)}
        </code>
      );
    }
    const link = part.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
    if (link) {
      const [, label, url] = link;
      return onOpenUrl ? (
        <button
          key={i}
          type="button"
          className="cursor-pointer text-primary underline underline-offset-2"
          onClick={() => onOpenUrl(url)}
        >
          {label}
        </button>
      ) : (
        <span key={i} className="text-primary underline underline-offset-2">{label}</span>
      );
    }
    return <span key={i}>{part}</span>;
  });
}

export default function ReleaseNotes({
  notes,
  onOpenUrl,
}: {
  notes: string;
  onOpenUrl?: (url: string) => void;
}) {
  const blocks: ReactNode[] = [];
  let bullets: string[] = [];

  const flushBullets = () => {
    if (!bullets.length) return;
    blocks.push(
      <ul key={`ul-${blocks.length}`} className="ml-4 list-disc space-y-1">
        {bullets.map((b, i) => (
          <li key={i} className="pl-0.5">{renderInline(b, onOpenUrl)}</li>
        ))}
      </ul>
    );
    bullets = [];
  };

  for (const raw of notes.split(/\r?\n/)) {
    const line = raw.trimEnd();
    if (!line.trim()) { flushBullets(); continue; }
    if (/^[-*]{3,}$/.test(line.trim())) { flushBullets(); blocks.push(<hr key={`hr-${blocks.length}`} className="border-border/60" />); continue; }
    const heading = line.match(/^#{1,6}\s+(.*)$/);
    if (heading) {
      flushBullets();
      blocks.push(
        <div key={`h-${blocks.length}`} className="text-[13px] font-semibold text-foreground">
          {renderInline(heading[1].trim(), onOpenUrl)}
        </div>
      );
      continue;
    }
    const bullet = line.match(/^\s*[-*]\s+(.*)$/);
    if (bullet) { bullets.push(bullet[1].trim()); continue; }
    flushBullets();
    blocks.push(
      <p key={`p-${blocks.length}`} className="text-muted-foreground">
        {renderInline(line.trim(), onOpenUrl)}
      </p>
    );
  }
  flushBullets();

  return <div className="space-y-2 text-[13px] leading-relaxed">{blocks}</div>;
}
