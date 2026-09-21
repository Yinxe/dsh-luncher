import { useMemo } from "react";
import { ArrowRightIcon, RefreshCwIcon, TriangleAlertIcon } from "lucide-react";
import { cn } from "cn";

import { Button } from "@/components/ui/button";
import { CopyButton } from "@/components/site/copy-button";
import { Led } from "@/components/site/led";
import type { ReleaseController } from "@/hooks/use-release";
import { formatBytes, formatDate, relativeTime, shortenUrl } from "@/lib/format";
import {
  GH_RELEASES_URL,
  PLATFORM_LABEL,
  R2_MANIFEST_URL,
  detectPlatform,
  pickPrimary,
  statusLabel,
  statusTone,
  type SourceStatus,
} from "@/lib/release";

/**
 * 首屏。
 *
 * 左边是「这是什么、点哪下」，右边不是装饰图，而是**这个页面自己取到的实时清单**：
 * 哪条源通了、版本号是多少、直链长什么样。下载页最容易失信的地方是「版本号是硬编码的」，
 * 这里直接把证据摆出来。
 */
export function Hero({ controller }: { controller: ReleaseController }) {
  const { state, source, setSource, reload, rows, loading } = controller;
  const platform = useMemo(detectPlatform, []);
  const primary = useMemo(() => pickPrimary(rows, platform), [rows, platform]);

  const primaryUrl = primary?.url ?? null;
  const assetCount = rows.filter((r) => !r.updaterOnly).length;

  return (
    <section id="top" className="relative">
      <div className="mx-auto max-w-[1180px] px-5 pt-14 pb-4 sm:px-8 sm:pt-20">
        <div className="grid gap-12 lg:grid-cols-12 lg:gap-10">
          {/* ── 左：主张与主按钮 ───────────────────── */}
          <div className="min-w-0 lg:col-span-7">
            <div className="rise flex flex-wrap items-center gap-x-3 gap-y-1" style={{ animationDelay: "40ms" }}>
              <span className="eyebrow text-primary">Tauri 2</span>
              <span className="text-muted-foreground/40">/</span>
              <span className="eyebrow">Windows · macOS · Linux</span>
            </div>

            <h1
              className="text-display-cjk rise mt-6 text-[2.5rem] sm:text-[3.4rem] lg:text-[3.9rem]"
              style={{ animationDelay: "120ms" }}
            >
              装好、跑起来、管得住。
            </h1>

            <p
              className="rise mt-6 max-w-[46ch] text-[15.5px] text-muted-foreground"
              style={{ animationDelay: "200ms" }}
            >
              DSH Launcher 是{" "}
              <a
                href="https://www.npmjs.com/package/@deepseek-ai/dsh"
                target="_blank"
                rel="noreferrer noopener"
                className="font-mono text-[13.5px] text-foreground underline decoration-border decoration-dotted underline-offset-4 hover:decoration-primary"
              >
                @deepseek-ai/dsh
              </a>{" "}
              的图形启动器：版本、profile、实例、插件与模型配置都在一个界面里，
              不用再记命令行，也不必先把 Node 环境折腾明白。
            </p>

            <div className="rise mt-9 flex flex-wrap items-center gap-3" style={{ animationDelay: "280ms" }}>
              <Button asChild size="lg" className="h-11 gap-2 px-5 text-[15px] shadow-[0_0_0_1px_rgba(77,107,254,.5),0_12px_40px_-12px_rgba(77,107,254,.75)]">
                <a href="#download">
                  下载 {PLATFORM_LABEL[platform]} 版
                  {primary?.bytes ? (
                    <span className="num ml-1 text-[12px] opacity-70">{formatBytes(primary.bytes)}</span>
                  ) : null}
                  <ArrowRightIcon className="size-4" />
                </a>
              </Button>
              <Button asChild variant="outline" size="lg" className="h-11 px-5 text-[15px]">
                <a href="#download">全部平台与直链</a>
              </Button>
              {primaryUrl ? (
                <CopyButton
                  text={primaryUrl}
                  label="复制直链"
                  variant="ghost"
                  className="h-11 px-3 text-muted-foreground"
                />
              ) : null}
            </div>

            <div
              className="rise mt-6 flex flex-wrap items-center gap-x-5 gap-y-2 text-[12.5px] text-muted-foreground"
              style={{ animationDelay: "340ms" }}
            >
              <span className="flex items-center gap-2">
                <Led tone="signal" />
                国内直连走自建源，GitHub 兜底
              </span>
              <span className="flex items-center gap-2">
                <Led tone="signal" />
                更新包签名校验，验签不过不安装
              </span>
              <span className="flex items-center gap-2">
                <Led tone="signal" />
                用户级安装，不需要 root
              </span>
            </div>

            {/* 关键数字：全部来自刚刚那份清单，不是写死的文案 */}
            <dl
              className="rise mt-10 grid grid-cols-2 gap-x-6 gap-y-5 border-t border-hairline pt-6 sm:grid-cols-4"
              style={{ animationDelay: "400ms" }}
            >
              <Metric label="当前版本" value={state.version ? `v${state.version}` : "—"} />
              <Metric
                label="发布"
                value={formatDate(state.publishedAt)}
                hint={relativeTime(state.publishedAt)}
              />
              <Metric label="安装包" value={loading ? "…" : `${assetCount} 个`} hint="含 3 个平台" />
              <Metric label="更新源" value="2 条" hint="自建源 + GitHub" />
            </dl>
          </div>

          {/* ── 右：实时清单面板 ───────────────────── */}
          <div className="min-w-0 lg:col-span-5">
            <div className="rise" style={{ animationDelay: "260ms" }}>
              <div className="panel corners p-5 sm:p-6">
                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-2">
                    <span className="eyebrow text-foreground/70">[ 实时清单 ]</span>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-xs"
                    onClick={reload}
                    aria-label="重新拉取清单"
                    title="重新拉取清单"
                    className="text-muted-foreground"
                  >
                    <RefreshCwIcon className={cn(loading && "animate-spin")} />
                  </Button>
                </div>

                <p className="mt-3 text-[13px] text-muted-foreground">
                  这个页面每次打开都会重新拉一次发布清单（
                  <span className="font-mono text-[12px]">cache: no-store</span>），
                  所以上面显示的版本号就是此刻的真实最新版。
                </p>

                <div className="mt-5 space-y-2.5">
                  <SourceRow
                    name="自建源 · Cloudflare R2"
                    status={state.settled ? state.r2 : "probing"}
                    detail={statusLabel(state.r2, state.r2Version ?? state.version)}
                    active={source === "r2"}
                    onClick={() => setSource("r2")}
                  />
                  <SourceRow
                    name="GitHub Release"
                    status={state.settled ? state.github : "probing"}
                    detail={statusLabel(state.github, state.version)}
                    active={source === "github"}
                    onClick={() => setSource("github")}
                  />
                </div>

                {state.r2 === "reachable" ? (
                  <p className="mt-3 text-[11.5px] leading-relaxed text-muted-foreground">
                    自建源桶没有配 CORS，浏览器读不到清单内容（只探得到「可达」）。这不影响下载，
                    也不影响启动器 —— 它走系统 HTTP 客户端。版本号与更新说明因此取自 GitHub Release。
                  </p>
                ) : null}

                <div className="mt-5 space-y-3 border-t border-hairline pt-5">
                  <UrlRow label="清单地址" url={R2_MANIFEST_URL} />
                  <UrlRow
                    label="当前平台直链"
                    url={primaryUrl}
                    fallback={GH_RELEASES_URL}
                    loading={loading}
                  />
                </div>

                {/* 这里不再放第二个切换器：上面两行本身就是可点的源选择，
                    真正的大切换器在「下载」区块的标题栏上（那里切换的影响最直观） */}
                <div className="mt-5 flex flex-wrap items-center gap-3">
                  {state.versionMismatch ? (
                    <span className="flex items-center gap-1.5 text-[12px] text-warn">
                      <TriangleAlertIcon className="size-3.5" />
                      两条源版本不一致，自建源可能还没同步
                    </span>
                  ) : (
                    <span className="text-[11.5px] text-muted-foreground">
                      {!state.settled
                        ? "正在读取清单"
                        : state.version
                          ? state.notesFrom === "r2"
                            ? "版本与说明取自建源清单"
                            : "版本取自 GitHub Release"
                          : "两条源都不可达"}
                    </span>
                  )}
                </div>
              </div>

              <p className="mt-3 px-1 text-[11.5px] text-muted-foreground">
                直链由清单里的版本号现算，页面不会把版本号写死在 HTML 里。
              </p>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}

function Metric({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div>
      <dt className="eyebrow">{label}</dt>
      <dd className="num mt-1.5 text-[19px] text-foreground">
        {value}
        {hint ? <span className="ml-2 text-[11.5px] text-muted-foreground">{hint}</span> : null}
      </dd>
    </div>
  );
}

function SourceRow({
  name,
  status,
  detail,
  active,
  onClick,
}: {
  name: string;
  status: SourceStatus;
  detail: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-active={active}
      className={cn(
        "row-rail flex w-full items-center gap-3 rounded-lg border border-transparent bg-muted/40 px-3 py-2.5 text-left transition-colors",
        "hover:border-border hover:bg-muted/70",
        active && "border-border bg-muted/70",
      )}
    >
      <Led tone={statusTone(status)} pulse={status === "probing"} />
      <span className="text-[13px]">{name}</span>
      <span
        className={cn("num ml-auto text-[11.5px]", status === "down" ? "text-warn" : "text-muted-foreground")}
      >
        {detail}
      </span>
      {active ? <span className="eyebrow text-primary">当前</span> : null}
    </button>
  );
}

function UrlRow({
  label,
  url,
  fallback,
  loading = false,
}: {
  label: string;
  url: string | null;
  fallback?: string;
  loading?: boolean;
}) {
  const value = url ?? fallback ?? null;
  return (
    <div className="min-w-0">
      <div className="flex items-center justify-between gap-2">
        <span className="eyebrow">{label}</span>
        {value ? (
          <CopyButton text={value} label="复制" variant="ghost" size="xs" className="text-muted-foreground" />
        ) : null}
      </div>
      <div className="url-line mt-1 truncate" title={value ?? undefined}>
        {value ? shortenUrl(value, 64) : loading ? "正在生成…" : "暂不可用"}
      </div>
    </div>
  );
}
