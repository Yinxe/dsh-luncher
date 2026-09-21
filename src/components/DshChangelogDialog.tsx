import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ExternalLink, FileText, Loader2, RefreshCw, ScrollText, TriangleAlert,
} from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import ReleaseNotes, { parseReleaseBody } from "@/components/ReleaseNotes";
import { cmpVer } from "@/lib/version";
import { api } from "../api";
import type { DshRelease } from "../types";

interface Props {
  open: boolean;
  onClose: () => void;
  /** 版本列表（来自版本表，新→旧）；没有 Release 的版本也会列出来并标注 */
  versions: string[];
  /** 打开时定位到哪个版本 */
  initialVersion: string | null;
  onOpenUrl: (url: string) => void;
}

const RELEASES_PAGE = "https://github.com/deepseek-ai/deepseek-harness/releases";
const fmtDate = (iso: string | null) =>
  iso && !isNaN(new Date(iso).getTime()) ? new Date(iso).toISOString().slice(0, 10) : "";

/**
 * dsh 更新日志：左栏逐个版本、右栏是该版本的 Release 正文。
 *
 * 数据来自官方 monorepo 的 GitHub Release（tag `dsh-v*`），一次拉全量并缓存
 * （Rust 侧 10 分钟 + ETag），所以前后翻版本是零等待的 —— 官方发的是中英双段，
 * 正文里用 `<h3 id="cn-…">` 之类的锚点分段，这里按语言切成两个标签页。
 * npm 的 packument 里没有 changelog，这也正是「更新日志只能来自 Release」的原因。
 */
export default function DshChangelogDialog({
  open, onClose, versions, initialVersion, onOpenUrl,
}: Props) {
  const [releases, setReleases] = useState<DshRelease[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(initialVersion);
  const [lang, setLang] = useState<"zh" | "en">("zh");
  const listRef = useRef<HTMLDivElement | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setReleases(await api.dshReleaseNotes());
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  // 每次打开：定位到点进来的那个版本；首次打开才拉数据（Rust 侧也有缓存）
  useEffect(() => {
    if (!open) return;
    setSelected(initialVersion ?? versions[0] ?? null);
    if (releases === null && !loading) load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, initialVersion]);

  const byVersion = useMemo(() => {
    const map = new Map<string, DshRelease>();
    for (const r of releases ?? []) map.set(r.version, r);
    return map;
  }, [releases]);

  /** 版本表里的版本 + Release 里独有的版本（官方有时只发 Release 不发 npm） */
  const allVersions = useMemo(() => {
    const seen = new Set<string>();
    const out: string[] = [];
    for (const v of versions) if (v && !seen.has(v)) { seen.add(v); out.push(v); }
    for (const r of releases ?? []) if (!seen.has(r.version)) { seen.add(r.version); out.push(r.version); }
    return out.sort((a, b) => cmpVer(b, a));
  }, [versions, releases]);

  useEffect(() => {
    if (!open || !selected) return;
    listRef.current
      ?.querySelector<HTMLElement>(`[data-version="${CSS.escape(selected)}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [open, selected, allVersions]);

  // 兜底：Release 拉回来后若定位的版本不在列表里（或还没定位），落到最新的一个
  useEffect(() => {
    if (!open || allVersions.length === 0) return;
    if (selected && allVersions.includes(selected)) return;
    setSelected(allVersions[0]);
  }, [open, allVersions, selected]);

  const release = selected ? byVersion.get(selected) : undefined;
  const parsed = useMemo(() => (release ? parseReleaseBody(release.body) : null), [release]);
  const hasZh = !!parsed?.zh;
  const hasEn = !!parsed?.en;

  // 切版本/切语言：有中文就默认中文，只有英文的时候落到英文
  useEffect(() => {
    setLang(hasZh ? "zh" : "en");
  }, [hasZh, hasEn, selected]);

  const body = parsed ? (lang === "zh" ? parsed.zh : parsed.en) ?? parsed.plain : "";
  const withNotes = allVersions.filter((v) => byVersion.has(v)).length;

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-0 overflow-hidden p-0 sm:max-w-4xl">
        <DialogHeader className="border-b border-border px-4 py-3">
          <DialogTitle className="flex flex-wrap items-center gap-2 text-[15px]">
            <ScrollText className="size-4 text-primary" /> dsh 更新日志
            <Badge variant="outline" className="text-[10px]">GitHub Releases</Badge>
            {releases && (
              <span className="text-[10.5px] font-normal text-muted-foreground">
                {withNotes} 个版本有发布说明
              </span>
            )}
          </DialogTitle>
          <DialogDescription>
            官方仓库 deepseek-ai/deepseek-harness 里各版本的 Release 正文。
            早期版本没建 Release，左栏会标成「无发布说明」。
          </DialogDescription>
        </DialogHeader>

        <div className="grid min-h-0 md:grid-cols-[190px_1fr]">
          {/* 左栏：版本列表（有发布说明的带色点，一眼看出哪些能看） */}
          <div
            ref={listRef}
            className="max-h-[38vh] overflow-y-auto border-b border-border p-1.5 md:max-h-[62vh] md:border-b-0 md:border-r"
          >
            {allVersions.length === 0 && (
              <div className="p-2 text-[11.5px] text-muted-foreground">版本列表为空</div>
            )}
            {allVersions.map((v) => {
              const has = byVersion.has(v);
              const on = v === selected;
              return (
                <button
                  key={v}
                  data-version={v}
                  type="button"
                  onClick={() => setSelected(v)}
                  title={has ? undefined : "官方没有为这个版本建 Release，没有发布说明可看"}
                  className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors ${
                    on ? "bg-primary/10 ring-1 ring-primary/30" : "hover:bg-muted/60"
                  }`}
                >
                  <span
                    className={`size-1.5 shrink-0 rounded-full ${has ? "bg-emerald-500" : "bg-muted-foreground/30"}`}
                  />
                  <span className={`min-w-0 flex-1 truncate font-mono text-[11.5px] ${on ? "font-semibold text-foreground" : ""}`}>
                    {v}
                  </span>
                  <span className="shrink-0 font-mono text-[10px] text-muted-foreground">
                    {fmtDate(byVersion.get(v)?.publishedAt ?? null).slice(5) || "—"}
                  </span>
                </button>
              );
            })}
          </div>

          {/* 右栏：选中版本的正文 */}
          <div className="max-h-[52vh] overflow-y-auto px-4 py-3 md:max-h-[62vh]">
            {loading && (
              <div className="flex h-40 items-center justify-center gap-2 text-[12.5px] text-muted-foreground">
                <Loader2 className="size-4 animate-spin" /> 正在读取官方 Release…
              </div>
            )}

            {!loading && error && (
              <Alert variant="destructive">
                <TriangleAlert />
                <AlertDescription className="space-y-2 text-[11.5px] leading-relaxed">
                  <div className="break-words">{error}</div>
                  <div className="flex flex-wrap gap-1.5">
                    <Button size="sm" variant="outline" className="h-6 px-2 text-[10.5px]" onClick={load}>
                      <RefreshCw className="size-3" /> 重试
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-6 px-2 text-[10.5px]"
                      onClick={() => onOpenUrl(RELEASES_PAGE)}
                    >
                      <ExternalLink className="size-3" /> 到 GitHub 上看
                    </Button>
                  </div>
                  <p className="text-[10.5px] opacity-80">
                    匿名访问 api.github.com 只有 60 次/小时，被限流时可以在设置里填一个 GitHub Token（5000/小时）。
                  </p>
                </AlertDescription>
              </Alert>
            )}

            {!loading && !error && !release && (
              <div className="space-y-2 py-6 text-center">
                <FileText className="mx-auto size-5 text-muted-foreground" />
                <div className="text-[12.5px] font-medium">{selected ?? "该版本"} 没有发布说明</div>
                <p className="mx-auto max-w-md text-[11px] leading-relaxed text-muted-foreground">
                  官方只为部分版本建了 GitHub Release。想看这一段里发生了什么，可以到
                  仓库的 Releases 页按 tag 翻，或对比两个 tag 之间的提交。
                </p>
                <Button size="sm" variant="outline" onClick={() => onOpenUrl(RELEASES_PAGE)}>
                  <ExternalLink /> 打开 Releases 页
                </Button>
              </div>
            )}

            {!loading && !error && release && (
              <div className="space-y-2.5">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-mono text-[14px] font-bold">{release.version}</span>
                  {release.prerelease ? (
                    <Badge variant="warning" className="text-[10px]">预发布</Badge>
                  ) : (
                    <Badge variant="success" className="text-[10px]">正式版</Badge>
                  )}
                  {release.publishedAt && (
                    <span className="font-mono text-[10.5px] text-muted-foreground">
                      {fmtDate(release.publishedAt)}
                    </span>
                  )}
                  <span className="flex-1" />
                  {hasZh && hasEn && (
                    <Tabs value={lang} onValueChange={(v) => setLang(v as "zh" | "en")}>
                      <TabsList>
                        <TabsTrigger value="zh" className="px-2.5 text-[11px]">中文</TabsTrigger>
                        <TabsTrigger value="en" className="px-2.5 text-[11px]">English</TabsTrigger>
                      </TabsList>
                    </Tabs>
                  )}
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-6 px-2 text-[10.5px]"
                    title={release.htmlUrl}
                    onClick={() => release.htmlUrl && onOpenUrl(release.htmlUrl)}
                  >
                    <ExternalLink className="size-3" /> 在 GitHub 查看
                  </Button>
                </div>

                {parsed?.compareUrl && (
                  <button
                    type="button"
                    className="text-[10.5px] text-primary underline-offset-2 hover:underline"
                    title={parsed.compareUrl}
                    onClick={() => onOpenUrl(parsed.compareUrl!)}
                  >
                    <ExternalLink className="mr-1 inline size-3" />
                    查看这一版的完整提交对比
                  </button>
                )}

                <ReleaseNotes notes={body} onOpenUrl={onOpenUrl} />
              </div>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
