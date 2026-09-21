import {
  ChevronDownIcon,
  DownloadIcon,
  ExternalLinkIcon,
  TriangleAlertIcon,
} from "lucide-react";
import { toast } from "sonner";
import { cn } from "cn";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Skeleton } from "@/components/ui/skeleton";
import { CopyButton } from "@/components/site/copy-button";
import { PlatformMark } from "@/components/site/icons";
import { Reveal } from "@/components/site/reveal";
import { SectionHeading } from "@/components/site/section-heading";
import { SourceSwitch } from "@/components/site/source-switch";
import type { DownloadRow, ReleaseController } from "@/hooks/use-release";
import { copyText } from "@/lib/copy";
import { formatBytes, shortenUrl } from "@/lib/format";
import { GH_RELEASES_URL, detectPlatform, type PlatformId } from "@/lib/release";

const ORDER: { id: PlatformId; label: string; hint: string }[] = [
  { id: "windows", label: "Windows", hint: "Windows 10 / 11 · x64" },
  { id: "macos", label: "macOS", hint: "macOS 10.15+ · 通用二进制" },
  { id: "linux", label: "Linux", hint: "glibc 发行版 · x86_64" },
];

/**
 * 下载矩阵 —— 这个页面的主菜。
 *
 * 不做成三张并列的「卡片」：直链是长字符串，横向铺开才好比对，
 * 所以按平台分行、每个包一行，行内左侧是身份、右侧是动作，地址单独一行常显。
 * 源切换会同时改变所有行 —— 包括「这个源到底有没有托管这个包」。
 */
export function DownloadMatrix({ controller }: { controller: ReleaseController }) {
  const { rows, state, source, setSource, loading } = controller;
  const platform = detectPlatform();

  const groups = ORDER.map((g) => ({
    ...g,
    items: rows.filter((r) => r.platform === g.id && !r.updaterOnly),
  })).filter((g) => g.items.length > 0);

  const updaterRows = rows.filter((r) => r.updaterOnly && r.url);
  const installable = rows.filter((r) => !r.updaterOnly && r.url);

  async function copyAll() {
    const header =
      source === "r2"
        ? `# DSH Launcher ${state.version ? `v${state.version}` : ""} 直链（自建源 · Cloudflare R2，地址永久不变）`
        : `# DSH Launcher ${state.version ? `v${state.version}` : ""} 直链（GitHub Release，与版本绑定）`;
    const body = installable
      .map((r) => `- ${r.label}（${r.arch}）：${r.url}`)
      .join("\n");
    const ok = await copyText(`${header}\n${body}\n`);
    toast[ok ? "success" : "error"](ok ? `已复制 ${installable.length} 条直链` : "复制失败，请手动选中");
  }

  return (
    <section className="mx-auto max-w-[1180px] px-5 py-20 sm:px-8 sm:py-24">
      <Reveal>
        <SectionHeading
          index="01"
          kicker="下载"
          id="download"
          title="按平台取包，直链就在行里"
          lead={
            source === "r2" ? (
              <>
                当前是<strong className="text-foreground">自建源（Cloudflare R2）</strong>：
                地址与版本无关（<span className="font-mono text-[13px]">…/latest/windows-x64-setup.exe</span>），
                发布时同名覆盖，所以一条链接可以长期用；末尾的{" "}
                <span className="font-mono text-[13px]">?v=</span> 只是给 CDN 的缓存指纹，去掉也照样能下。
              </>
            ) : (
              <>
                当前是<strong className="text-foreground">GitHub Release</strong>：
                地址与版本绑定（<span className="font-mono text-[13px]">…/releases/download/v{state.version ?? "x.y.z"}/…</span>），
                永久有效，适合固定版本归档；国内直连可能连不上，慢的话切回自建源。
              </>
            )
          }
          action={<SourceSwitch value={source} onChange={setSource} state={state} />}
        />
      </Reveal>

      <Reveal delay={80} className="mt-10">
        <div className="panel overflow-hidden">
          {loading && groups.length === 0 ? (
            <MatrixSkeleton />
          ) : (
            groups.map((group) => (
              <div key={group.id} className="not-last:border-b not-last:border-hairline">
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1 bg-muted/30 px-4 py-3 sm:px-5">
                  <PlatformMark platform={group.id} className="size-4 text-muted-foreground" />
                  <span className="text-[13.5px] font-medium">{group.label}</span>
                  <span className="eyebrow">{group.hint}</span>
                  {platform === group.id ? (
                    <Badge variant="secondary" className="ml-1 h-5">
                      你的系统
                    </Badge>
                  ) : null}
                  <span className="num ml-auto text-[11.5px] text-muted-foreground">
                    {group.items.length} 个包
                  </span>
                </div>

                <ul>
                  {group.items.map((row) => (
                    <AssetRow key={row.id} row={row} detected={platform === row.platform} />
                  ))}
                </ul>
              </div>
            ))
          )}
        </div>
      </Reveal>

      {updaterRows.length > 0 ? (
        <Reveal delay={120} className="mt-4">
          <Collapsible>
            <div className="panel overflow-hidden">
              <CollapsibleTrigger className="group/upd flex w-full items-center gap-3 px-4 py-3 text-left transition-colors hover:bg-muted/40 sm:px-5">
                <ChevronDownIcon className="size-4 text-muted-foreground transition-transform group-data-[state=open]/upd:rotate-180" />
                <span className="text-[13.5px]">自动更新产物（由客户端使用，不用手装）</span>
                <span className="num ml-auto text-[11.5px] text-muted-foreground">
                  {updaterRows.length} 个
                </span>
              </CollapsibleTrigger>
              <CollapsibleContent>
                <ul className="border-t border-hairline">
                  {updaterRows.map((row) => (
                    <li key={row.id} className="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3 sm:px-5">
                      <span className="num rounded border border-border px-1.5 text-[11px] text-muted-foreground">
                        {row.ext}
                      </span>
                      <span className="text-[13px] text-muted-foreground">{row.label}</span>
                      {row.url ? <CopyButton text={row.url} label="复制" variant="ghost" size="xs" /> : null}
                    </li>
                  ))}
                </ul>
              </CollapsibleContent>
            </div>
          </Collapsible>
        </Reveal>
      ) : null}

      <Reveal delay={140} className="mt-5 flex flex-wrap items-center gap-3">
        <Button type="button" variant="outline" size="sm" onClick={copyAll} disabled={installable.length === 0}>
          复制全部直链（{installable.length}）
        </Button>
        <Button asChild variant="ghost" size="sm" className="text-muted-foreground">
          <a href={GH_RELEASES_URL} target="_blank" rel="noreferrer noopener">
            打开 GitHub Release 页
            <ExternalLinkIcon className="size-3.5" />
          </a>
        </Button>
        {state.settled && !state.version ? (
          <span className="flex items-center gap-1.5 text-[12.5px] text-warn">
            <TriangleAlertIcon className="size-3.5" />
            没读到任何版本号：自建源的固定键地址仍然可用（路径与版本无关），但 GitHub 直链需要版本号
          </span>
        ) : null}
      </Reveal>
    </section>
  );
}

function AssetRow({ row, detected }: { row: DownloadRow; detected: boolean }) {
  const blocked = !row.url;

  return (
    <li
      className="row-rail not-last:border-t not-last:border-hairline"
      data-active={row.recommended ? "true" : "false"}
    >
      <div className="flex flex-col gap-3 py-4 pl-5 pr-4 sm:flex-row sm:items-center sm:gap-6 sm:pl-6 sm:pr-5">
        <div className="flex min-w-0 flex-1 items-start gap-3">
          <span className="num mt-[3px] flex h-5 shrink-0 items-center rounded border border-border px-1.5 text-[10.5px] uppercase text-muted-foreground">
            {row.ext}
          </span>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-[14.5px] font-medium">{row.label}</span>
              {row.recommended ? (
                <Badge className="h-5 bg-primary/15 text-[11px] text-primary" variant="secondary">
                  推荐
                </Badge>
              ) : null}
              {detected ? (
                <Badge variant="outline" className="h-5 text-[11px]">
                  你的系统
                </Badge>
              ) : null}
              {blocked ? (
                <Badge className="h-5 gap-1 bg-warn/15 text-[11px] text-warn" variant="secondary">
                  <TriangleAlertIcon className="size-3" />
                  自建源未托管
                </Badge>
              ) : null}
            </div>
            <p className="mt-1.5 max-w-[60ch] text-[12.5px] text-muted-foreground">{row.blurb}</p>
            <p className="num mt-1.5 text-[11.5px] text-muted-foreground/80">{row.arch}</p>
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-2 sm:gap-3">
          <span className="num w-[68px] text-right text-[12.5px] text-muted-foreground">
            {formatBytes(row.bytes)}
          </span>
          {row.url ? <CopyButton text={row.url} iconOnly label="复制直链" /> : null}
          {row.url ? (
            <Button asChild size="sm" variant={row.recommended ? "default" : "outline"} className="h-8">
              <a
                href={row.url}
                target="_blank"
                rel="noreferrer noopener"
                title={`下载 ${row.label}`}
                aria-label={`下载 ${row.label}`}
              >
                <DownloadIcon />
                下载
              </a>
            </Button>
          ) : (
            <Button asChild size="sm" variant="outline" className="h-8 text-warn">
              <a href={GH_RELEASES_URL} target="_blank" rel="noreferrer noopener">
                去 GitHub
                <ExternalLinkIcon className="size-3.5" />
              </a>
            </Button>
          )}
        </div>
      </div>

      {row.url ? (
        <div className="flex items-center gap-3 pb-4 pl-5 pr-4 sm:pl-6 sm:pr-5">
          {/* min-w-0 + flex-1：直链是 nowrap，不这样写它会把整行撑出视口 */}
          <span className="url-line min-w-0 flex-1 truncate" title={row.url}>
            {shortenUrl(row.url, 96)}
          </span>
        </div>
      ) : (
        <p className={cn("pb-4 pl-5 pr-4 text-[12px] text-muted-foreground sm:pl-6 sm:pr-5")}>
          {row.unavailable} ——{" "}
          <a
            href={GH_RELEASES_URL}
            target="_blank"
            rel="noreferrer noopener"
            className="text-foreground underline decoration-border decoration-dotted underline-offset-4"
          >
            到 GitHub Release 下载
          </a>
          ，或切到 GitHub 源。
        </p>
      )}
    </li>
  );
}

function MatrixSkeleton() {
  // 用 shadcn 的 Skeleton 打底，只叠加一条扫描动画（比转圈更贴这套控制台语言）
  return (
    <div className="divide-y divide-hairline">
      {["Windows", "macOS", "Linux"].map((name) => (
        <div key={name} className="px-4 py-4 sm:px-5">
          <div className="flex items-center gap-3">
            <Skeleton className="sweep h-4 w-20" />
            <Skeleton className="sweep h-4 w-40" />
          </div>
          <div className="mt-4 space-y-3">
            <Skeleton className="sweep h-3 w-full max-w-[420px]" />
            <Skeleton className="sweep h-3 w-full max-w-[300px]" />
          </div>
        </div>
      ))}
    </div>
  );
}
