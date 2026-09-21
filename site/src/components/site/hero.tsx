import { useMemo } from "react";
import { ArrowRightIcon, DownloadIcon, ExternalLinkIcon, RefreshCwIcon } from "lucide-react";
import { cn } from "cn";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CopyButton } from "@/components/site/copy-button";
import { PlatformMark } from "@/components/site/icons";
import { Led } from "@/components/site/led";
import type { DownloadRow, ReleaseController } from "@/hooks/use-release";
import { formatBytes, formatDate, relativeTime } from "@/lib/format";
import {
  GH_RELEASES_URL,
  PLATFORM_LABEL,
  R2_MANIFEST_URL,
  detectPlatform,
  pickPrimary,
  statusLabel,
  statusTone,
  type PlatformId,
} from "@/lib/release";

/**
 * 首屏。
 *
 * 结构上做两件事，都是为了「先能下载，再谈别的」：
 * 1. 左边讲这是什么，右边直接给**你当前系统**的安装包（点一下就能下）—— 不是一张状态卡；
 * 2. 版本 / 发布 / 两条源的可达性收成一条通栏读数条（仪表盘的读数区）：
 *    既完成「版本号不是写死的」这件事的举证，又不占首屏的视觉重量。
 */
export function Hero({ controller }: { controller: ReleaseController }) {
  const { state, reload, rows, loading } = controller;
  const platform = useMemo(detectPlatform, []);
  const primary = useMemo(() => pickPrimary(rows, platform), [rows, platform]);
  const mine = useMemo(
    () => rows.filter((r) => !r.updaterOnly && r.platform === platform),
    [rows, platform],
  );

  return (
    <section id="top" className="relative">
      <div className="mx-auto max-w-[1180px] px-5 pt-14 pb-14 sm:px-8 sm:pt-20">
        <div className="grid gap-12 lg:grid-cols-12 lg:gap-12">
          {/* ── 左：这是什么 ───────────────────── */}
          <div className="min-w-0 lg:col-span-7">
            <div className="rise flex flex-wrap items-center gap-x-3 gap-y-1" style={{ animationDelay: "40ms" }}>
              <span className="eyebrow text-primary">Tauri 2</span>
              <span className="text-muted-foreground/40">/</span>
              <span className="eyebrow">Windows · macOS · Linux</span>
            </div>

            <h1
              className="text-display-cjk rise mt-6 text-[2.4rem] sm:text-[3rem] lg:text-[3.4rem]"
              style={{ animationDelay: "120ms" }}
            >
              DeepSeek Harness 启动器
            </h1>

            <p
              className="text-display-cjk rise mt-5 max-w-[30ch] text-[1.25rem] text-foreground sm:text-[1.4rem]"
              style={{ animationDelay: "180ms" }}
            >
              装 dsh、起实例、管插件、配模型 —— 都在一个界面里。
            </p>

            <p
              className="rise mt-5 max-w-[46ch] text-[15px] text-muted-foreground"
              style={{ animationDelay: "220ms" }}
            >
              <a
                href="https://www.npmjs.com/package/@deepseek-ai/dsh"
                target="_blank"
                rel="noreferrer noopener"
                className="font-mono text-[14px] text-foreground underline decoration-border decoration-dotted underline-offset-4 hover:decoration-primary"
              >
                @deepseek-ai/dsh
              </a>{" "}
              （DeepSeek Harness CLI）本身是命令行工具。这个启动器负责把它装好、拉起来、
              管住周边：版本、profile、插件、模型配置与日志；dsh 该在终端里跑时，仍然在终端里跑
              —— 也可以在启动器内嵌跑，日志实时看。
            </p>

            <div className="rise mt-9 flex flex-wrap items-center gap-3" style={{ animationDelay: "280ms" }}>
              <Button
                asChild
                size="lg"
                className="h-11 gap-2 px-5 text-[15px] shadow-[0_0_0_1px_color-mix(in_oklab,var(--primary)_50%,transparent),0_12px_40px_-14px_color-mix(in_oklab,var(--primary)_85%,transparent)]"
              >
                <a href="#download">
                  <DownloadIcon className="size-4" />
                  下载 {PLATFORM_LABEL[platform]} 版
                  {primary?.bytes ? (
                    <span className="num ml-1 text-[12px] opacity-75">{formatBytes(primary.bytes)}</span>
                  ) : null}
                </a>
              </Button>
              <Button asChild variant="outline" size="lg" className="h-11 px-5 text-[15px]">
                <a href="#download">
                  全部安装包
                  <ArrowRightIcon className="size-4" />
                </a>
              </Button>
            </div>

            <ul
              className="rise mt-8 flex flex-wrap items-center gap-x-6 gap-y-2.5 text-[12.5px] text-muted-foreground"
              style={{ animationDelay: "340ms" }}
            >
              {["国内直连优先走自建源", "安装包签名校验，验签不过不装", "用户级安装，不需要 root"].map((text) => (
                <li key={text} className="flex items-center gap-2">
                  <Led tone="signal" />
                  {text}
                </li>
              ))}
            </ul>
          </div>

          {/* ── 右：你的系统，直接下 ───────────────── */}
          <div className="min-w-0 lg:col-span-5">
            <div className="panel corners rise p-5 sm:p-6" style={{ animationDelay: "260ms" }}>
              <div className="flex flex-wrap items-center gap-x-2.5 gap-y-1">
                <PlatformMark platform={platform} className="size-4 text-primary" />
                <span className="eyebrow text-foreground/70">[ 你的系统 ]</span>
                <span className="text-[13.5px] font-medium">{PLATFORM_LABEL[platform]}</span>
                <span className="num ml-auto text-[11.5px] text-muted-foreground">
                  {state.version ? `v${state.version}` : loading ? "读取中" : "—"}
                </span>
              </div>

              <p className="mt-2.5 text-[12.5px] text-muted-foreground">{PLATFORM_HINT[platform]}</p>

              <ul className="mt-5 space-y-2">
                {loading && mine.length === 0 ? (
                  <li className="num flex h-14 items-center justify-center rounded-lg border border-hairline text-[12px] text-muted-foreground">
                    正在读取清单…
                  </li>
                ) : (
                  mine.map((row) => <QuickRow key={row.id} row={row} />)
                )}
              </ul>

              <div className="mt-5 flex items-center justify-between gap-3 border-t border-hairline pt-4">
                <a
                  href="#download"
                  className="text-[12.5px] text-muted-foreground transition-colors hover:text-foreground"
                >
                  看全部 3 个平台共 6 个包 ↓
                </a>
                {primary?.url ? (
                  <CopyButton text={primary.url} label="复制直链" variant="ghost" size="xs" />
                ) : null}
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* ── 通栏读数条：版本 / 发布 / 两条源 / 清单 ───────── */}
      <div className="border-y border-hairline bg-muted/25">
        <div className="mx-auto max-w-[1180px] px-5 sm:px-8">
          <dl className="grid grid-cols-2 gap-x-6 gap-y-5 py-6 sm:grid-cols-4 lg:grid-cols-[repeat(4,minmax(0,1fr))_auto] lg:gap-x-10">
            <Readout
              label="当前版本"
              value={state.version ? `v${state.version}` : loading ? "…" : "未读到"}
              mono
            />
            <Readout label="发布" value={formatDate(state.publishedAt)} hint={relativeTime(state.publishedAt)} mono />
            <Readout
              label="自建源 · R2"
              value={statusLabel(state.r2, state.r2Version ?? state.version)}
              tone={statusTone(state.r2)}
              pulse={!state.settled}
            />
            <Readout
              label="GitHub Release"
              value={statusLabel(state.github, state.version)}
              tone={statusTone(state.github)}
              pulse={!state.settled}
            />
            <div className="col-span-2 flex items-center gap-2 sm:col-span-4 lg:col-span-1 lg:justify-end">
              <Button asChild variant="outline" size="sm" className="text-[12.5px]">
                <a href={R2_MANIFEST_URL} target="_blank" rel="noreferrer noopener">
                  更新清单 latest.json
                  <ExternalLinkIcon className="size-3.5" />
                </a>
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                onClick={reload}
                className="text-muted-foreground"
                aria-label="重新读取清单"
                title="重新读取清单"
              >
                <RefreshCwIcon className={cn(loading && "animate-spin")} />
              </Button>
            </div>
          </dl>
        </div>
      </div>

      {state.r2 === "reachable" ? (
        <div className="mx-auto max-w-[1180px] px-5 pt-4 sm:px-8">
          <p className="text-[11.5px] text-muted-foreground">
            自建源桶没有配 CORS，浏览器读不到清单内容（只探得到「可达」）。这不影响下载，也不影响启动器
            —— 它走系统 HTTP 客户端。版本号与更新说明因此取自 GitHub Release。
          </p>
        </div>
      ) : null}
    </section>
  );
}

function QuickRow({ row }: { row: DownloadRow }) {
  const blocked = !row.url;
  return (
    <li
      className="row-rail flex items-center gap-3 rounded-lg border border-hairline px-3 py-2.5"
      data-active={blocked ? "false" : "true"}
    >
      <span className="num flex h-5 shrink-0 items-center rounded border border-border px-1.5 text-[10.5px] uppercase text-muted-foreground">
        {row.ext}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-[13.5px]">{row.label.replace(/^(Windows|Linux|macOS) /, "")}</span>
          {row.recommended && !blocked ? (
            <Badge className="h-5 shrink-0 bg-primary/12 text-[10.5px] text-primary" variant="secondary">
              推荐
            </Badge>
          ) : null}
        </div>
        <span className="num text-[11px] text-muted-foreground">{formatBytes(row.bytes)}</span>
      </div>
      {row.url ? (
        <Button asChild size="sm" variant={row.recommended ? "default" : "outline"} className="h-7 shrink-0">
          <a href={row.url} target="_blank" rel="noreferrer noopener" aria-label={`下载 ${row.label}`}>
            下载
          </a>
        </Button>
      ) : (
        <Button asChild size="sm" variant="outline" className="h-7 shrink-0 text-warn">
          <a href={GH_RELEASES_URL} target="_blank" rel="noreferrer noopener">
            去 GitHub
          </a>
        </Button>
      )}
    </li>
  );
}

function Readout({
  label,
  value,
  hint,
  tone,
  pulse,
  mono,
}: {
  label: string;
  value: string;
  hint?: string;
  tone?: "signal" | "warn" | "muted";
  pulse?: boolean;
  mono?: boolean;
}) {
  return (
    <div className="min-w-0">
      <dt className="eyebrow flex items-center gap-2">
        {tone ? <Led tone={tone} pulse={pulse} /> : null}
        {label}
      </dt>
      <dd className={cn("mt-1.5 text-[14px]", mono && "num", tone === "warn" && "text-warn")}>
        {value}
        {hint ? <span className="ml-2 text-[11.5px] text-muted-foreground">{hint}</span> : null}
      </dd>
    </div>
  );
}

const PLATFORM_HINT: Record<PlatformId, string> = {
  windows: "x64 安装向导，双击即可；企业批量部署可选下方的 MSI。",
  macos: "通用二进制，Apple Silicon 与 Intel 用同一个包。",
  linux: "AppImage 免安装；deb / rpm 交给系统包管理器。",
  unknown: "没能识别你的系统，请从下方列表里选。",
};
