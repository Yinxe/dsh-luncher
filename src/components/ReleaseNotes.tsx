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
