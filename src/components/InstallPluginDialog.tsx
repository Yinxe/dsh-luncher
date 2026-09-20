import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Cable, ExternalLink, FolderGit2, GitBranch, Loader2, Plus, Search, TriangleAlert,
} from "lucide-react";
import {
  Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Field, FieldDescription, FieldLabel, FieldSeparator,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import PluginCandidateList from "@/components/PluginCandidateList";
import { api } from "../api";
import type { GitHubRepoInfo, PackageSearchItem, PluginCandidate } from "../types";

interface Props {
  profile: string;
  open: boolean;
  onClose: () => void;
  /** 批量安装（一个终端任务里按顺序执行多条 dsh plugin add） */
  onInstallMany: (specs: string[], mode: "install" | "upgrade") => void;
  /** clone 仓库 + 本地 link 安装 */
  onCloneInstall: (input: {
    url: string;
    gitRef: string | null;
    subPath: string | null;
    build: boolean;
  }) => void;
}

/** 表单里的小号字段标题（统一的次级说明层级） */
const labelCls = "text-[11.5px] font-normal text-muted-foreground";
/** 字段说明文字 */
const descCls = "text-[10.5px] leading-relaxed";

const isTarballUrl = (s: string) =>
  /^https?:\/\//i.test(s.trim()) &&
  (/\.(tgz|tar\.gz|tar)$/i.test(s.trim()) || s.includes("/releases/download/"));

/** owner/repo → https://github.com/owner/repo（clone 用） */
function normalizeGitUrl(input: string): string {
  const s = input.trim();
  if (!s) return s;
  if (/^(https?:\/\/|git@|ssh:\/\/|git:\/\/)/.test(s)) return s;
  if (s.startsWith("github:")) return `https://github.com/${s.slice("github:".length)}`;
  if (/^[\w.-]+\/[\w.-]+$/.test(s)) return `https://github.com/${s}`;
  return s;
}

/** 输入里是否带 ref（#ref / #ref&path:…） */
function refOf(input: string): string | null {
  const frag = input.split("#")[1];
  if (!frag) return null;
  for (const part of frag.split("&")) {
    const p = part.trim();
    if (p && !p.startsWith("path:")) return p;
  }
  return null;
}

/**
 * 插件的四种安装方式（全部由后端走官方 `dsh plugin` 命令，输出进内置终端）：
 * 1. NPM 包 —— registry 搜索 / 精确规格；版本号可检测更新
 * 2. GitHub 仓库 —— 探测插件包（monorepo 子包一并列出）后安装；提交可检测更新
 * 3. 链接安装 —— 本地 link 路径或 .tgz 直链；仅手动更新
 * 4. Clone 仓库 + 本地 link —— 克隆到启动器目录，可 git pull 更新
 */
export default function InstallPluginDialog({
  profile, open, onClose, onInstallMany, onCloneInstall,
}: Props) {
  const [tab, setTab] = useState("npm");

  // ── npm ──
  const [query, setQuery] = useState("");
  const [searching, setSearching] = useState(false);
  const [searchErr, setSearchErr] = useState<string | null>(null);
  const [results, setResults] = useState<PackageSearchItem[]>([]);
  const [manualSpec, setManualSpec] = useState("");

  // ── github ──
  const [ghInput, setGhInput] = useState("");
  const [ghPath, setGhPath] = useState("");
  const [ghLoading, setGhLoading] = useState(false);
  const [ghErr, setGhErr] = useState<string | null>(null);
  const [ghInfo, setGhInfo] = useState<GitHubRepoInfo | null>(null);
  const [ghSel, setGhSel] = useState<string[]>([]);

  // ── 链接 ──
  const [linkInput, setLinkInput] = useState("");
  const [linkProbing, setLinkProbing] = useState(false);
  const [linkErr, setLinkErr] = useState<string | null>(null);
  const [linkCands, setLinkCands] = useState<PluginCandidate[] | null>(null);
  const [linkSel, setLinkSel] = useState<string[]>([]);

  // ── clone ──
  const [cloneInput, setCloneInput] = useState("");
  const [cloneRef, setCloneRef] = useState("");
  const [clonePath, setClonePath] = useState("");
  const [cloneBuild, setCloneBuild] = useState(true);
  const [cloneProbing, setCloneProbing] = useState(false);
  const [cloneErr, setCloneErr] = useState<string | null>(null);
  const [cloneCands, setCloneCands] = useState<PluginCandidate[] | null>(null);

  useEffect(() => {
    if (!open) return;
    setTab("npm");
    setQuery(""); setSearchErr(null); setResults([]); setManualSpec("");
    setGhInput(""); setGhPath(""); setGhErr(null); setGhInfo(null); setGhSel([]);
    setLinkInput(""); setLinkErr(null); setLinkCands(null); setLinkSel([]);
    setCloneInput(""); setCloneRef(""); setClonePath(""); setCloneBuild(true);
    setCloneErr(null); setCloneCands(null);
  }, [open]);

  // ── npm 搜索 ──
  const search = useCallback(async () => {
    const q = query.trim();
    if (!q) return;
    setSearching(true);
    setSearchErr(null);
    try {
      setResults(await api.searchPackages(q));
    } catch (e) {
      setResults([]);
      setSearchErr(String(e));
    } finally {
      setSearching(false);
    }
  }, [query]);

  // ── GitHub 探测 ──
  const previewGithub = useCallback(async () => {
    let s = ghInput.trim();
    if (!s) return;
    const p = ghPath.trim();
    if (p && !isTarballUrl(s)) {
      s = s.includes("#") ? `${s}&path:${p}` : `${s}#path:${p}`;
    }
    setGhLoading(true);
    setGhErr(null);
    setGhInfo(null);
    setGhSel([]);
    try {
      const info = await api.fetchGithubRepo(s);
      setGhInfo(info);
      // 默认选中第一个「已就绪」的候选，用户可再调整
      const first = info.candidates.find((c) => c.ready) ?? info.candidates[0];
      if (first) setGhSel([first.installSpec]);
    } catch (e) {
      setGhErr(String(e));
    } finally {
      setGhLoading(false);
    }
  }, [ghInput, ghPath]);

  // ── 本地路径探测 ──
  const probeLocal = useCallback(async () => {
    const p = linkInput.trim();
    if (!p) return;
    setLinkProbing(true);
    setLinkErr(null);
    setLinkCands(null);
    setLinkSel([]);
    try {
      const list = await api.probeLocalPlugins(p);
      setLinkCands(list);
      const first = list.find((c) => c.ready) ?? list[0];
      if (first) setLinkSel([first.installSpec]);
    } catch (e) {
      setLinkErr(String(e));
    } finally {
      setLinkProbing(false);
    }
  }, [linkInput]);

  // ── clone 前先探测仓库（可选，用于选子包） ──
  const probeForClone = useCallback(async () => {
    const s = cloneInput.trim();
    if (!s) return;
    setCloneProbing(true);
    setCloneErr(null);
    setCloneCands(null);
    try {
      const info = await api.fetchGithubRepo(s);
      setCloneCands(info.candidates);
      if (!cloneRef && info.gitRef) setCloneRef(info.gitRef);
    } catch (e) {
      setCloneErr(String(e));
    } finally {
      setCloneProbing(false);
    }
  }, [cloneInput, cloneRef]);

  const ghTarball = ghInfo?.probe === "tarball";
  const selectedGh = useMemo(
    () => (ghInfo?.candidates ?? []).filter((c) => ghSel.includes(c.installSpec)),
    [ghInfo, ghSel],
  );
  const needsBuildHint = selectedGh.some((c) => !c.libOk);

  const installGh = () => {
    if (ghSel.length === 0) return;
    onInstallMany(ghSel, "install");
    onClose();
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[88vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl">
        <DialogHeader className="shrink-0 gap-1.5 border-b border-border px-5 py-3.5 pr-12">
          <DialogTitle>安装插件</DialogTitle>
          <DialogDescription className="text-[11.5px] leading-relaxed">
            安装到 profile「{profile}」——全部通过官方 <span className="font-mono">dsh plugin add</span> 执行，
            输出实时显示在插件页的内置终端里。
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          <Tabs value={tab} onValueChange={setTab} className="gap-4">
            <TabsList className="w-full">
              <TabsTrigger value="npm" className="text-xs">NPM 包</TabsTrigger>
              <TabsTrigger value="github" className="text-xs">GitHub 仓库</TabsTrigger>
              <TabsTrigger value="link" className="text-xs">链接 / 本地</TabsTrigger>
              <TabsTrigger value="clone" className="text-xs">Clone 仓库</TabsTrigger>
            </TabsList>

            {/* ── 1. NPM 包 ─────────────────────────── */}
            <TabsContent value="npm" className="space-y-4">
              <Field>
                <FieldLabel htmlFor="plugin-search" className={labelCls}>搜索 registry</FieldLabel>
                <div className="flex items-center gap-2">
                  <Input
                    id="plugin-search"
                    autoFocus
                    className="h-8 min-w-0 flex-1 font-mono text-xs"
                    placeholder="关键词，如 mcwiki、search、tts…"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && search()}
                  />
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-8 shrink-0"
                    disabled={searching || !query.trim()}
                    onClick={search}
                  >
                    {searching ? <Loader2 className="animate-spin" /> : <Search />} 搜索
                  </Button>
                </div>
                <FieldDescription className={descCls}>
                  装之前先看描述；带版本号的包可用下方「精确规格」安装。npm 包按版本号检测更新。
                </FieldDescription>
              </Field>

              {searchErr && (
                <Alert variant="destructive" className="py-1.5">
                  <TriangleAlert />
                  <AlertDescription className="text-[11.5px] leading-relaxed break-words">
                    {searchErr}
                  </AlertDescription>
                </Alert>
              )}

              {results.length > 0 && (
                <div className="max-h-64 space-y-1.5 overflow-y-auto pr-1">
                  {results.map((r) => (
                    <div
                      key={`${r.name}@${r.version}`}
                      className="rounded-lg border border-border bg-background/50 p-2.5"
                    >
                      <div className="flex items-center gap-2">
                        <span
                          className="min-w-0 flex-1 truncate font-mono text-[12.5px] font-semibold"
                          title={r.name}
                        >
                          {r.name}
                        </span>
                        <Badge variant="outline" className="shrink-0 font-mono text-[10px]">{r.version}</Badge>
                        {r.publishedAt && (
                          <span className="hidden shrink-0 text-[10px] text-muted-foreground sm:inline">
                            {r.publishedAt.slice(0, 10)}
                          </span>
                        )}
                        <Button
                          size="sm"
                          variant="outline"
                          className="h-6 shrink-0 px-2 text-[11px]"
                          title={`安装 ${r.name}@${r.version}`}
                          onClick={() => {
                            onInstallMany([`${r.name}@${r.version}`], "install");
                            onClose();
                          }}
                        >
                          <Plus /> 安装
                        </Button>
                      </div>
                      <p className="mt-1 line-clamp-2 text-[11px] leading-relaxed text-muted-foreground">
                        {r.description || "（无描述）"}
                      </p>
                      {r.link && (
                        <button
                          className="mt-0.5 inline-flex items-center gap-1 text-[10.5px] text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                          onClick={() => api.openUrl(r.link!).catch(() => undefined)}
                        >
                          <ExternalLink className="h-3 w-3" /> 包页面
                        </button>
                      )}
                    </div>
                  ))}
                </div>
              )}
              {results.length === 0 && !searching && !searchErr && (
                <p className="py-2 text-center text-[11.5px] text-muted-foreground">
                  输入关键词搜索 registry
                </p>
              )}

              <FieldSeparator>
                <span className="text-[10.5px]">或直接安装已知规格</span>
              </FieldSeparator>
              <Field>
                <div className="flex items-center gap-2">
                  <Input
                    id="plugin-spec"
                    className="h-8 min-w-0 flex-1 font-mono text-xs"
                    placeholder="如 @dshp/mcwiki-search@1.2.3"
                    value={manualSpec}
                    onChange={(e) => setManualSpec(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && manualSpec.trim()) {
                        onInstallMany([manualSpec.trim()], "install");
                        onClose();
                      }
                    }}
                  />
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-8 shrink-0"
                    disabled={!manualSpec.trim()}
                    onClick={() => {
                      onInstallMany([manualSpec.trim()], "install");
                      onClose();
                    }}
                  >
                    <Plus /> 安装
                  </Button>
                </div>
                <FieldDescription className={descCls}>
                  支持 npm 包名、版本区间与 dist-tag。
                </FieldDescription>
              </Field>
            </TabsContent>

            {/* ── 2. GitHub 仓库 ────────────────────── */}
            <TabsContent value="github" className="space-y-4">
              <Field>
                <FieldLabel htmlFor="gh-repo" className={labelCls}>仓库地址</FieldLabel>
                <div className="flex items-center gap-2">
                  <Input
                    id="gh-repo"
                    autoFocus
                    className="h-8 min-w-0 flex-1 font-mono text-xs"
                    placeholder="owner/repo、仓库 URL 或 .tgz 直链"
                    value={ghInput}
                    onChange={(e) => setGhInput(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && previewGithub()}
                  />
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-8 shrink-0"
                    disabled={ghLoading || !ghInput.trim()}
                    onClick={previewGithub}
                  >
                    {ghLoading ? <Loader2 className="animate-spin" /> : <GitBranch />} 探测
                  </Button>
                </div>
                <FieldDescription className={descCls}>
                  探测会扫描整棵文件树：仓库根的插件包 + monorepo（pnpm-workspace / workspaces）
                  的子包插件都会列出来，可一次勾选多个安装。走 jsDelivr CDN + git，不消耗
                  GitHub API 额度（60 次/小时 的匿名限制不会因此被磨光）。
                </FieldDescription>
              </Field>

              <Field>
                <FieldLabel htmlFor="gh-path" className={labelCls}>
                  插件在仓库中的路径{isTarballUrl(ghInput) ? "" : "（可选，填了就只看这一个）"}
                </FieldLabel>
                <Input
                  id="gh-path"
                  className="h-8 font-mono text-xs"
                  placeholder="如 plugins/mcwiki-search"
                  value={ghPath}
                  disabled={isTarballUrl(ghInput)}
                  onChange={(e) => setGhPath(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && previewGithub()}
                />
              </Field>

              {ghErr && (
                <Alert variant="destructive" className="py-1.5">
                  <TriangleAlert />
                  <AlertDescription className="text-[11.5px] leading-relaxed break-words">
                    {ghErr}
                  </AlertDescription>
                </Alert>
              )}

              {ghInfo && (
                <div className="space-y-3 rounded-lg border border-border bg-muted/30 p-3">
                  <div className="flex items-start gap-2">
                    <GitBranch className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
                    <div className="min-w-0 flex-1 space-y-1.5">
                      <div className="truncate font-mono text-[12.5px] font-semibold" title={ghInfo.fullName}>
                        {ghInfo.fullName}
                      </div>
                      <div className="flex flex-wrap items-center gap-1.5">
                        <Badge variant="outline" className="font-mono text-[10px]">★ {ghInfo.stars}</Badge>
                        {ghInfo.license && <Badge variant="secondary" className="text-[10px]">{ghInfo.license}</Badge>}
                        {ghInfo.gitRef && <Badge variant="info" className="font-mono text-[10px]">#{ghInfo.gitRef}</Badge>}
                        {ghInfo.isMonorepo && (
                          <Badge variant="success" className="text-[10px]">
                            monorepo · {ghInfo.workspaceGlobs.join(" ")}
                          </Badge>
                        )}
                        {ghInfo.metaDegraded && (
                          <Badge
                            variant="outline"
                            className="text-[10px]"
                            title="探测与更新走 jsDelivr / git 免额度通道，不受影响；stars/license 需要 GitHub API，额度用尽时暂缺。可在设置里填 GitHub Token 提高额度。"
                          >
                            元数据受限
                          </Badge>
                        )}
                        {ghInfo.probe === "contents" && (
                          <Badge variant="warning" className="text-[10px]">已降级为单目录探测</Badge>
                        )}
                      </div>
                    </div>
                  </div>

                  <p className="line-clamp-3 text-[11px] leading-relaxed text-muted-foreground">
                    {ghInfo.description || "（无描述）"}
                  </p>

                  {ghTarball ? (
                    <div className="space-y-2 border-t border-border pt-3">
                      <div className="flex items-start gap-2 text-[10.5px]">
                        <span className="shrink-0 pt-0.5 text-muted-foreground">直装链接</span>
                        <span className="min-w-0 flex-1 break-all rounded-md bg-background/80 px-1.5 py-0.5 font-mono">
                          {ghInfo.installSpec}
                        </span>
                      </div>
                      <Button
                        size="sm"
                        className="w-full"
                        onClick={() => {
                          onInstallMany([ghInfo.installSpec], "install");
                          onClose();
                        }}
                      >
                        <Plus /> 安装该打包产物
                      </Button>
                      <p className={descCls}>
                        打包产物直链没有版本渠道，之后只能手动重新安装同一链接来更新。
                      </p>
                    </div>
                  ) : (
                    <>
                      <div className="flex items-center gap-2 border-t border-border pt-3">
                        <span className="text-[11.5px] font-semibold">
                          探测到 {ghInfo.candidates.length} 个候选包
                        </span>
                        <span className="flex-1" />
                        {ghInfo.factsPending && (
                          <Badge variant="warning" className="text-[10px]">部分元数据读取超时</Badge>
                        )}
                      </div>
                      <PluginCandidateList
                        candidates={ghInfo.candidates}
                        selected={ghSel}
                        onChange={setGhSel}
                        emptyText="这个仓库里没有找到含 package.json 的插件包"
                      />
                      {needsBuildHint && (
                        <p className="text-[10.5px] leading-relaxed text-amber-600 dark:text-amber-400">
                          选中的包里有的没提交 lib/：直接安装会缺构建产物，建议改用「Clone 仓库」方式安装（会装依赖并构建）。
                        </p>
                      )}
                    </>
                  )}
                </div>
              )}

              {!ghInfo && !ghErr && (
                <p className="py-2 text-center text-[11.5px] leading-relaxed text-muted-foreground">
                  输入仓库后先探测：GitHub 仓库安装按提交哈希检测更新
                </p>
              )}
            </TabsContent>

            {/* ── 3. 链接 / 本地 ────────────────────── */}
            <TabsContent value="link" className="space-y-4">
              <Field>
                <FieldLabel htmlFor="link-target" className={labelCls}>本地路径或打包产物链接</FieldLabel>
                <div className="flex items-center gap-2">
                  <Input
                    id="link-target"
                    autoFocus
                    className="h-8 min-w-0 flex-1 font-mono text-xs"
                    placeholder="/path/to/plugin 或 https://…/pkg.tgz"
                    value={linkInput}
                    onChange={(e) => setLinkInput(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && !isTarballUrl(linkInput) && probeLocal()}
                  />
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-8 shrink-0"
                    disabled={linkProbing || !linkInput.trim() || isTarballUrl(linkInput)}
                    onClick={probeLocal}
                    title="本地目录：探测目录与 monorepo 子包"
                  >
                    {linkProbing ? <Loader2 className="animate-spin" /> : <Cable />} 探测
                  </Button>
                </div>
                <FieldDescription className={descCls}>
                  本地目录会用 <span className="font-mono">link:</span> 软链安装（改源码即时生效，无版本更新渠道）；
                  也可以直接粘贴 <span className="font-mono">.tgz</span> / releases 资产链接安装。
                </FieldDescription>
              </Field>

              {isTarballUrl(linkInput) && (
                <div className="space-y-2 rounded-lg border border-border bg-muted/30 p-3">
                  <div className="flex items-start gap-2 text-[10.5px]">
                    <span className="shrink-0 pt-0.5 text-muted-foreground">安装规格</span>
                    <span className="min-w-0 flex-1 break-all rounded-md bg-background/80 px-1.5 py-0.5 font-mono">
                      {linkInput.trim()}
                    </span>
                  </div>
                  <Button
                    size="sm"
                    className="w-full"
                    onClick={() => {
                      onInstallMany([linkInput.trim()], "install");
                      onClose();
                    }}
                  >
                    <Plus /> 安装该打包产物
                  </Button>
                </div>
              )}

              {linkErr && (
                <Alert variant="destructive" className="py-1.5">
                  <TriangleAlert />
                  <AlertDescription className="text-[11.5px] leading-relaxed break-words">
                    {linkErr}
                  </AlertDescription>
                </Alert>
              )}

              {linkCands && (
                <div className="space-y-2 rounded-lg border border-border bg-muted/30 p-3">
                  <div className="text-[11.5px] font-semibold">
                    探测到 {linkCands.length} 个候选包
                  </div>
                  <PluginCandidateList
                    candidates={linkCands}
                    selected={linkSel}
                    onChange={setLinkSel}
                    emptyText="该目录下没有找到插件包（缺 package.json / lib）"
                  />
                </div>
              )}

              {!linkCands && !linkErr && !isTarballUrl(linkInput) && (
                <p className="py-2 text-center text-[11.5px] leading-relaxed text-muted-foreground">
                  本地 link 与打包直链都属于「仅手动更新」：链接安装无法自动检测新版本
                </p>
              )}
            </TabsContent>

            {/* ── 4. Clone 仓库 ─────────────────────── */}
            <TabsContent value="clone" className="space-y-4">
              <Field>
                <FieldLabel htmlFor="clone-url" className={labelCls}>git 远端地址</FieldLabel>
                <div className="flex items-center gap-2">
                  <Input
                    id="clone-url"
                    autoFocus
                    className="h-8 min-w-0 flex-1 font-mono text-xs"
                    placeholder="owner/repo 或 https://github.com/owner/repo.git"
                    value={cloneInput}
                    onChange={(e) => setCloneInput(e.target.value)}
                  />
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-8 shrink-0"
                    disabled={cloneProbing || !cloneInput.trim()}
                    onClick={probeForClone}
                    title="探测仓库内的插件子包（可选）"
                  >
                    {cloneProbing ? <Loader2 className="animate-spin" /> : <GitBranch />} 探测
                  </Button>
                </div>
                <FieldDescription className={descCls}>
                  克隆到 <span className="font-mono">~/.dsh-launcher/git-plugins/&lt;owner&gt;-&lt;repo&gt;</span>，
                  再以 <span className="font-mono">link:</span> 安装。更新方式 = 在该目录 <span className="font-mono">git pull</span>
                  （插件页「本地克隆仓库」里一键执行）。
                </FieldDescription>
              </Field>

              <div className="grid gap-3 sm:grid-cols-2">
                <Field>
                  <FieldLabel htmlFor="clone-ref" className={labelCls}>分支 / 标签 / 提交（可选）</FieldLabel>
                  <Input
                    id="clone-ref"
                    className="h-8 font-mono text-xs"
                    placeholder="main、v1.2.3 或 sha"
                    value={cloneRef}
                    onChange={(e) => setCloneRef(e.target.value)}
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="clone-path" className={labelCls}>插件子路径（monorepo 必填）</FieldLabel>
                  <Input
                    id="clone-path"
                    className="h-8 font-mono text-xs"
                    placeholder="如 plugins/mcwiki-search"
                    value={clonePath}
                    onChange={(e) => setClonePath(e.target.value)}
                  />
                </Field>
              </div>

              <div className="flex items-start gap-2.5 rounded-lg border border-border bg-background/50 px-3 py-2.5">
                <Switch
                  id="clone-build"
                  checked={cloneBuild}
                  onCheckedChange={setCloneBuild}
                  className="mt-0.5"
                />
                <div className="min-w-0 flex-1">
                  <label htmlFor="clone-build" className="text-[11.5px] font-medium">
                    克隆后安装依赖并构建（pnpm install + pnpm run build）
                  </label>
                  <p className={descCls}>
                    仓库没有提交 <span className="font-mono">lib/</span> 时需要；两步失败都不会中断最后的 link 安装，
                    结果以终端输出为准。
                  </p>
                </div>
              </div>

              {cloneErr && (
                <Alert variant="destructive" className="py-1.5">
                  <TriangleAlert />
                  <AlertDescription className="text-[11.5px] leading-relaxed break-words">
                    {cloneErr}
                  </AlertDescription>
                </Alert>
              )}

              {cloneCands && (
                <div className="space-y-2 rounded-lg border border-border bg-muted/30 p-3">
                  <div className="text-[11.5px] font-semibold">
                    仓库内 {cloneCands.length} 个候选包（点「填入」设定子路径）
                  </div>
                  <PluginCandidateList
                    candidates={cloneCands}
                    selected={clonePath ? [cloneCands.find((c) => c.path === clonePath)?.installSpec ?? ""].filter(Boolean) : []}
                    onChange={(specs) => {
                      const hit = cloneCands.find((c) => c.installSpec === specs[specs.length - 1]);
                      setClonePath(hit?.path ?? "");
                    }}
                    multiple={false}
                  />
                </div>
              )}
            </TabsContent>
          </Tabs>
        </div>

        {/* 固定脚注：主操作按当前标签页变化 */}
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-t border-border bg-muted/30 px-5 py-2.5">
          {tab === "github" && ghInfo && !ghTarball && (
            <Button size="sm" disabled={ghSel.length === 0} onClick={installGh}>
              <Plus /> 安装选中的 {ghSel.length} 个插件
            </Button>
          )}
          {tab === "link" && linkCands && (
            <Button
              size="sm"
              disabled={linkSel.length === 0}
              onClick={() => {
                onInstallMany(linkSel, "install");
                onClose();
              }}
            >
              <Plus /> link 安装选中的 {linkSel.length} 个
            </Button>
          )}
          {tab === "clone" && (
            <Button
              size="sm"
              disabled={!cloneInput.trim()}
              onClick={() => {
                onCloneInstall({
                  url: normalizeGitUrl(cloneInput),
                  gitRef: cloneRef.trim() || refOf(cloneInput),
                  subPath: clonePath.trim() || null,
                  build: cloneBuild,
                });
                onClose();
              }}
            >
              <FolderGit2 /> Clone 并 link 安装
            </Button>
          )}
          <span className="flex-1" />
          <p className="text-[10.5px] leading-relaxed text-muted-foreground">
            安装/卸载/升级全部经由官方 <span className="font-mono">dsh plugin</span> 命令执行
          </p>
        </div>
      </DialogContent>
    </Dialog>
  );
}
