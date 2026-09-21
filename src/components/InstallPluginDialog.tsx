import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Cable, ExternalLink, FolderGit2, GitBranch, Layers, Loader2, Plus, Search, TriangleAlert, Zap,
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
import type { CloneProbe, GhAccel, PackageSearchItem } from "../types";

interface Props {
  profile: string;
  open: boolean;
  onClose: () => void;
  /** 批量安装（一个终端任务里按顺序执行多条 dsh plugin add） */
  onInstallMany: (specs: string[], mode: "install" | "upgrade") => void;
  /** clone 仓库 + 本地 link 安装；accel=false 时本次不走 GitHub 加速 */
  onCloneInstall: (
    input: {
      url: string;
      gitRef: string | null;
      subPath: string | null;
      build: boolean;
    },
    accel: boolean,
  ) => void;
}

/** 表单里的小号字段标题（统一的次级说明层级） */
const labelCls = "text-[11.5px] font-normal text-muted-foreground";
/** 字段说明文字 */
const descCls = "text-[10.5px] leading-relaxed";

const isTarballUrl = (s: string) =>
  /^https?:\/\//i.test(s.trim()) &&
  (/\.(tgz|tar\.gz|tar)$/i.test(s.trim()) || s.includes("/releases/download/"));

const isLocalPath = (s: string) =>
  /^(~\/|\/|\.{1,2}\/|[A-Za-z]:[\\/]|\\\\)/.test(s.trim());

/** owner/repo → https://github.com/owner/repo（clone 用） */
function normalizeGitUrl(input: string): string {
  const s = input.trim();
  if (!s) return s;
  if (/^(https?:\/\/|git@|ssh:\/\/|git:\/\/)/.test(s)) return s;
  if (s.startsWith("github:")) return `https://github.com/${s.slice("github:".length)}`;
  if (/^[\w.-]+\/[\w.-]+/.test(s)) return `https://github.com/${s}`;
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

/** GitHub 输入（owner/repo、仓库 URL、tree 链接、fragment）→ 规范化后的安装规格 */
function githubSpec(raw: string): { spec: string; ref: string | null; path: string | null } | null {
  const s = raw.trim();
  if (!s || isTarballUrl(s)) return null;
  const [main, fragment] = s.split("#");
  let rest = main.trim().replace(/\/+$/, "");
  rest = rest.replace(/^github:/i, "");
  rest = rest.replace(/^git\+https:\/\/github\.com\//i, "");
  rest = rest.replace(/^https?:\/\/github\.com\//i, "");
  rest = rest.replace(/^git@github\.com:/i, "");
  const seg = rest.split("/").filter(Boolean);
  if (seg.length < 2) return null;
  const owner = seg[0];
  const repo = seg[1].replace(/\.git$/i, "");
  const valid = (x: string) => /^[\w.-]+$/.test(x);
  if (!valid(owner) || !valid(repo)) return null;
  let ref: string | null = null;
  let path: string | null = null;
  const tail = seg.slice(2);
  if (tail.length >= 2 && (tail[0] === "tree" || tail[0] === "blob")) {
    ref = tail[1];
    if (tail.length > 2) path = tail.slice(2).join("/");
  } else if (tail.length === 1) {
    ref = tail[0];
  }
  for (const part of (fragment ?? "").split("&")) {
    const p = part.trim();
    if (!p) continue;
    if (p.startsWith("path:")) path = p.slice("path:".length).trim() || path;
    else ref = p;
  }
  const spec = `github:${owner}/${repo}${ref ? `#${ref}` : ""}${
    path ? `${ref ? "&" : "#"}path:${path}` : ""
  }`;
  return { spec, ref, path };
}

type LinkKind = "link" | "github" | "tgz" | "raw";

/** 链接直装：本地 link / 仓库插件链接 / .tgz 直链 → dsh 安装规格 */
function toInstallSpec(raw: string): { spec: string; kind: LinkKind; note: string } {
  const s = raw.trim();
  if (!s) return { spec: "", kind: "raw", note: "" };
  if (s.startsWith("file://")) {
    const p = s.slice("file://".length);
    return { spec: `link:${p}`, kind: "link", note: "本地目录：软链安装，改源码即时生效" };
  }
  if (isLocalPath(s)) {
    return { spec: `link:${s}`, kind: "link", note: "本地目录：软链安装，改源码即时生效" };
  }
  if (/^https?:\/\//i.test(s) && !/github\.com\//i.test(s)) {
    return isTarballUrl(s)
      ? { spec: s, kind: "tgz", note: "打包产物直链：装一次，没有版本渠道" }
      : { spec: s, kind: "raw", note: "按原样交给 dsh plugin add" };
  }
  const gh = githubSpec(s);
  if (gh) {
    return {
      spec: gh.spec,
      kind: "github",
      note: gh.path
        ? `GitHub 仓库子包：${gh.path}（pnpm 会下整仓 tar.gz，慢就用「Clone 仓库」）`
        : "GitHub 仓库根包（未填子路径；monorepo 建议改用「Clone 仓库」安装）",
    };
  }
  if (isTarballUrl(s)) {
    return { spec: s, kind: "tgz", note: "打包产物直链：装一次，没有版本渠道" };
  }
  return { spec: s, kind: "raw", note: "按原样交给 dsh plugin add（npm 规格等）" };
}

/** 加速摘要：前缀 + 耗时（clone 关心 git 一列） */
function accelText(a: GhAccel | null): string {
  if (!a || a.nodes.length === 0) return "";
  const git = a.nodes.find((n) => n.gitMs != null);
  const dl = a.nodes[0];
  const parts = [`git → ${git ? `${git.prefix} ${git.gitMs}ms` : "无可用 git 代理"}`];
  if (dl) parts.push(`下载 → ${dl.prefix} ${dl.ms}ms`);
  return parts.join(" · ");
}

/**
 * 本次 clone 的**实际请求地址**。
 *
 * 为什么要在界面上算一遍：git 的改写（`url.<前缀>.insteadOf`）发生在 git 进程内部，
 * 命令行那行永远是原地址 —— 光看地址框和终端输出，没法判断加速到底有没有生效。
 * 规则必须与后端 `ghaccel::pick_prefix` 一致：指定的前缀能用就用它，否则退回第一个
 * 支持 git 的前缀；只改写 `https://github.com/`（SSH 形式不在匹配范围里）。
 */
type EffectiveGit = { kind: "accel"; url: string } | { kind: "direct"; why: string };

function effectiveGit(
  raw: string,
  on: boolean,
  accel: GhAccel | null,
  preferred: string,
): EffectiveGit | null {
  const url = normalizeGitUrl(raw);
  if (!url) return null;
  if (/^(git@|ssh:\/\/|git:\/\/)/.test(url)) {
    return { kind: "direct", why: "SSH 形式的地址不会被改写，想走加速请改用 https 地址" };
  }
  if (!/^https?:\/\/github\.com\//i.test(url)) {
    return { kind: "direct", why: "非 github 域名，加速不会介入" };
  }
  if (!on) return { kind: "direct", why: "本次任务关掉了加速" };
  if (!accel || accel.nodes.length === 0) {
    return { kind: "direct", why: "还没测速，点右侧「测速」后生效" };
  }
  const gitNodes = accel.nodes.filter((n) => n.gitMs != null);
  if (gitNodes.length === 0) {
    return { kind: "direct", why: "测速结果里没有支持 git 的前缀" };
  }
  const want = preferred.trim();
  // 设置里可能填的是不带尾斜杠的形式，后端 normalize_prefix 会补上，这里对齐
  const wantNorm = want && !want.endsWith("/") ? `${want}/` : want;
  const pick = gitNodes.find((n) => n.prefix === wantNorm) ?? gitNodes[0];
  const p = pick.prefix.endsWith("/") ? pick.prefix : `${pick.prefix}/`;
  return { kind: "accel", url: url.startsWith(p) ? url : `${p}${url}` };
}

/**
 * 插件的三种安装方式（全部由后端走官方 `dsh plugin` 命令，输出进内置终端）：
 * 1. NPM 包 —— registry 搜索 / 精确规格；版本号可检测更新
 * 2. 链接直装 —— 本地 link 路径、仓库插件链接或 .tgz 直链；无版本渠道，仅手动重装
 * 3. Clone 仓库 + 本地 link —— 克隆到启动器目录，可 git pull 更新
 *
 * 只有 Clone 保留「探测」：探测 = 先真克隆到本地再扫描工作树（不查 jsDelivr 索引、
 * 不查 GitHub 文件树），因此候选与 lib/ 判定与实际安装的那份目录完全一致。
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

  // ── 链接直装 ──
  const [linkInput, setLinkInput] = useState("");

  // ── clone ──
  const [cloneInput, setCloneInput] = useState("");
  const [cloneRef, setCloneRef] = useState("");
  const [clonePath, setClonePath] = useState("");
  const [cloneBuild, setCloneBuild] = useState(true);
  const [cloneProbing, setCloneProbing] = useState(false);
  const [cloneErr, setCloneErr] = useState<string | null>(null);
  const [cloneProbe, setCloneProbe] = useState<CloneProbe | null>(null);
  const [accelOn, setAccelOn] = useState(true);
  const [accelInfo, setAccelInfo] = useState<GhAccel | null>(null);
  const [accelLoading, setAccelLoading] = useState(false);
  const [accelErr, setAccelErr] = useState<string | null>(null);
  /** 设置里固定的前缀（留空 = 自动挑最快）。界面要按同一条规则算实际请求地址 */
  const [accelPreferred, setAccelPreferred] = useState("");

  useEffect(() => {
    if (!open) return;
    setTab("npm");
    setQuery(""); setSearchErr(null); setResults([]); setManualSpec("");
    setLinkInput("");
    setCloneInput(""); setCloneRef(""); setClonePath(""); setCloneBuild(true);
    setCloneErr(null); setCloneProbe(null);
    setAccelErr(null);
    // 加速开关的默认值跟着设置走（clone 页仍可临时改成本次不加速）
    api.getSettings()
      .then((s) => { setAccelOn(s.githubAccel); setAccelPreferred(s.githubProxy); })
      .catch(() => undefined);
    // 已缓存的测速结果先显示出来；没有缓存就等用户点「测速」或探测时自动跑
    api.getGithubAccel(false).then(setAccelInfo).catch(() => undefined);
  }, [open]);

  // ── npm 搜索 ──
  const searchSeq = useRef(0);
  const search = useCallback(async () => {
    const q = query.trim();
    if (!q) return;
    // 连点/连按回车会并发多次搜索，慢的那个响应会后到并覆盖结果，
    // 用递增序号只让「最后一次」写入，避免装到没搜过的包。
    const mySeq = ++searchSeq.current;
    setSearching(true);
    setSearchErr(null);
    try {
      const r = await api.searchPackages(q);
      if (searchSeq.current === mySeq) setResults(r);
    } catch (e) {
      if (searchSeq.current === mySeq) {
        setResults([]);
        setSearchErr(String(e));
      }
    } finally {
      if (searchSeq.current === mySeq) setSearching(false);
    }
  }, [query]);

  const link = useMemo(() => toInstallSpec(linkInput), [linkInput]);

  // ── GitHub 加速：手动测速 ──
  const measureAccel = useCallback(async () => {
    setAccelLoading(true);
    setAccelErr(null);
    try {
      setAccelInfo(await api.getGithubAccel(true));
    } catch (e) {
      setAccelErr(String(e));
    } finally {
      setAccelLoading(false);
    }
  }, []);

  // ── Clone 探测：先克隆（或同步）到 git-plugins，再本地扫描 ──
  const probeForClone = useCallback(async () => {
    const s = cloneInput.trim();
    if (!s) return;
    setCloneProbing(true);
    setCloneErr(null);
    setCloneProbe(null);
    try {
      const probe = await api.probeCloneRepo(normalizeGitUrl(s), cloneRef.trim() || refOf(s), accelOn);
      setCloneProbe(probe);
      if (probe.accel) {
        // 探测时若刚测过速，顺手把结果拿到界面上（走缓存，不会二次联网）
        api.getGithubAccel(false).then(setAccelInfo).catch(() => undefined);
      }
      if (!clonePath.trim()) {
        const ready = probe.candidates.filter((c) => c.ready);
        const pick = ready.length === 1 ? ready[0] : probe.candidates.length === 1 ? probe.candidates[0] : null;
        if (pick) setClonePath(pick.path);
      }
    } catch (e) {
      setCloneErr(String(e));
    } finally {
      setCloneProbing(false);
    }
  }, [cloneInput, cloneRef, accelOn, clonePath]);

  const accelSummary = useMemo(() => accelText(accelInfo), [accelInfo]);
  // 本次 clone 真正会请求的地址（直连时给出原因），见 effectiveGit
  const cloneEffective = useMemo(
    () => effectiveGit(cloneInput, accelOn, accelInfo, accelPreferred),
    [cloneInput, accelOn, accelInfo, accelPreferred],
  );

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[88vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl">
        <DialogHeader className="shrink-0 gap-2 border-b border-border px-5 py-3.5 pr-12">
          <DialogTitle>安装插件</DialogTitle>
          {/* 目标 profile 单独做成一条：安装最常见的事故就是装到别的 profile 上，
              所以这里把它从说明文字里提出来，用主色底 + 等宽加粗，每个标签页都看得见 */}
          <div className="flex flex-wrap items-center gap-2 rounded-lg border border-primary/25 bg-primary/[0.05] px-2.5 py-1.5">
            <Layers className="h-3.5 w-3.5 shrink-0 text-primary" />
            <span className="text-[11.5px]">目标 profile</span>
            <span className="font-mono text-[12.5px] font-semibold text-foreground">{profile}</span>
            <span className="flex-1" />
            <span className="text-[10.5px] text-muted-foreground">只影响这个 profile</span>
          </div>
          <DialogDescription className="text-[11.5px] leading-relaxed">
            全部通过官方 <span className="font-mono">dsh plugin add</span> 执行，输出实时显示在插件页的内置终端里。
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          <Tabs value={tab} onValueChange={setTab} className="gap-4">
            <TabsList className="w-full">
              <TabsTrigger value="npm" className="text-xs">NPM 包</TabsTrigger>
              <TabsTrigger value="link" className="text-xs">链接直装</TabsTrigger>
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

            {/* ── 2. 链接直装 ───────────────────────── */}
            <TabsContent value="link" className="space-y-4">
              <Field>
                <FieldLabel htmlFor="link-target" className={labelCls}>
                  本地路径 / 仓库插件链接 / .tgz 直链
                </FieldLabel>
                <Input
                  id="link-target"
                  autoFocus
                  className="h-8 font-mono text-xs"
                  placeholder="~/code/my-plugin、https://github.com/owner/repo/tree/main/plugins/x，或 https://…/pkg.tgz"
                  value={linkInput}
                  onChange={(e) => setLinkInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && link.spec) {
                      onInstallMany([link.spec], "install");
                      onClose();
                    }
                  }}
                />
                <FieldDescription className={descCls}>
                  直接装，<strong>不做任何远端探测</strong>。这三类都没有版本渠道：装完只能手动重装同一来源更新；
                  需要 <span className="font-mono">git pull</span> 式更新请用「Clone 仓库」。
                </FieldDescription>
              </Field>

              {link.spec && (
                <div className="space-y-2 rounded-lg border border-border bg-muted/30 p-3">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <Badge
                      variant={link.kind === "link" ? "secondary" : link.kind === "github" ? "info" : "outline"}
                      className="text-[10px]"
                    >
                      {link.kind === "link" ? "本地 link" : link.kind === "github" ? "GitHub 仓库" : link.kind === "tgz" ? "打包产物" : "原样规格"}
                    </Badge>
                    <span className="text-[10.5px] text-muted-foreground">{link.note}</span>
                  </div>
                  <div className="flex items-start gap-2 text-[10.5px]">
                    <span className="shrink-0 pt-0.5 text-muted-foreground">安装规格</span>
                    <span className="min-w-0 flex-1 break-all rounded-md bg-background/80 px-1.5 py-0.5 font-mono">
                      {link.spec}
                    </span>
                  </div>
                  <Button
                    size="sm"
                    className="w-full"
                    onClick={() => {
                      onInstallMany([link.spec], "install");
                      onClose();
                    }}
                  >
                    <Plus /> 安装
                  </Button>
                </div>
              )}

              {!link.spec && (
                <p className="py-2 text-center text-[11.5px] leading-relaxed text-muted-foreground">
                  支持：<span className="font-mono">/path/to/plugin</span>、
                  <span className="font-mono">owner/repo#main&amp;path:plugins/x</span>、
                  <span className="font-mono">https://…/pkg.tgz</span> 或任意 dsh 规格
                </p>
              )}
            </TabsContent>

            {/* ── 3. Clone 仓库 ─────────────────────── */}
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
                    title="先克隆到本地，再扫描工作树里的插件包"
                  >
                    {cloneProbing ? <Loader2 className="animate-spin" /> : <GitBranch />} 探测
                  </Button>
                </div>
                <FieldDescription className={descCls}>
                  克隆到 <span className="font-mono">~/.dsh-launcher/git-plugins/&lt;owner&gt;-&lt;repo&gt;</span>，
                  再以 <span className="font-mono">link:</span> 安装。更新方式 = 在该目录 <span className="font-mono">git pull</span>
                  （插件页「本地克隆仓库」里一键执行）。<strong>探测 = 先克隆再本地扫描</strong>：慢一点，
                  但候选与 <span className="font-mono">lib/</span> 判定就是安装用的那份工作树（不再查 jsDelivr 索引）。
                </FieldDescription>
                {/* git 的 insteadOf 在进程内部改写地址，命令行里永远是原地址 —— 这里明写实际请求 */}
                {cloneEffective && (
                  <p className="font-mono text-[10.5px] leading-relaxed text-muted-foreground">
                    {cloneEffective.kind === "accel" ? (
                      <>
                        实际请求：<span className="text-foreground">{cloneEffective.url}</span>
                        <span className="ml-1">（git 会自行改写，命令行里仍是原地址）</span>
                      </>
                    ) : (
                      <>实际请求：直连原地址 —— {cloneEffective.why}</>
                    )}
                  </p>
                )}
              </Field>

              <div className="space-y-2 rounded-lg border border-border bg-background/50 px-3 py-2.5">
                <div className="flex items-start gap-2.5">
                  <Switch
                    id="clone-accel"
                    checked={accelOn}
                    onCheckedChange={setAccelOn}
                    className="mt-0.5"
                  />
                  <div className="min-w-0 flex-1">
                    <label htmlFor="clone-accel" className="flex items-center gap-1.5 text-[11.5px] font-medium">
                      <Zap className="h-3.5 w-3.5 text-amber-500" /> GitHub 加速（本次 clone）
                    </label>
                    <p className={descCls}>
                      把 github 链接拼到测速最快的<strong>前缀代理</strong>上（如
                      <span className="font-mono"> https://gh-proxy.com/https://github.com/…</span>）：
                      clone / fetch / pull 与 releases 资产下载都会走它，非 github 域名一律不动。
                      结果缓存 6 小时，可直接在设置里固定用哪个前缀或补充自建代理。
                    </p>
                  </div>
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-7 shrink-0 px-2 text-[11px]"
                    disabled={accelLoading}
                    onClick={measureAccel}
                    title="重新拉取 GitHub520 hosts 并测速"
                  >
                    {accelLoading ? <Loader2 className="animate-spin" /> : <Zap />} 测速
                  </Button>
                </div>
                {accelSummary && (
                  <p className="border-t border-border pt-2 font-mono text-[10px] text-muted-foreground">
                    {accelInfo?.cached ? "缓存" : "本次测速"}：{accelSummary}
                  </p>
                )}
                {accelErr && (
                  <p className="border-t border-border pt-2 text-[10.5px] text-amber-600 dark:text-amber-400">
                    测速失败（不挡安装，clone 走普通网络）：{accelErr}
                  </p>
                )}
              </div>

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

              {cloneProbing && (
                <p className="rounded-lg border border-border bg-muted/30 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
                  正在克隆 / 同步仓库并扫描工作树…（首次会顺带测速，仓库大时请稍等）
                </p>
              )}

              {cloneErr && (
                <Alert variant="destructive" className="py-1.5">
                  <TriangleAlert />
                  <AlertDescription className="text-[11.5px] leading-relaxed break-words">
                    {cloneErr}
                  </AlertDescription>
                </Alert>
              )}

              {cloneProbe && (
                <div className="space-y-2 rounded-lg border border-border bg-muted/30 p-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-[11.5px] font-semibold">
                      扫描到 {cloneProbe.candidates.length} 个候选包
                    </span>
                    <Badge variant="outline" className="font-mono text-[10px]">
                      {cloneProbe.dirName}
                    </Badge>
                    {cloneProbe.gitRef && (
                      <Badge variant="info" className="font-mono text-[10px]">#{cloneProbe.gitRef}</Badge>
                    )}
                    {cloneProbe.accel && (
                      <Badge variant="success" className="text-[10px]" title={cloneProbe.accel}>
                        <Zap /> 已加速
                      </Badge>
                    )}
                  </div>
                  <p className="break-all font-mono text-[10px] text-muted-foreground">{cloneProbe.root}</p>
                  <PluginCandidateList
                    candidates={cloneProbe.candidates}
                    selected={clonePath ? [cloneProbe.candidates.find((c) => c.path === clonePath)?.installSpec ?? ""].filter(Boolean) : []}
                    onChange={(specs) => {
                      const hit = cloneProbe.candidates.find((c) => c.installSpec === specs[specs.length - 1]);
                      setClonePath(hit?.path ?? "");
                    }}
                    multiple={false}
                    emptyText="工作树里没有找到插件包（缺 package.json / dsh.bundle）"
                  />
                </div>
              )}
            </TabsContent>
          </Tabs>
        </div>

        {/* 固定脚注：主操作按当前标签页变化 */}
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-t border-border bg-muted/30 px-5 py-2.5">
          {tab === "link" && (
            <Button
              size="sm"
              disabled={!link.spec}
              onClick={() => {
                onInstallMany([link.spec], "install");
                onClose();
              }}
            >
              <Cable /> 直装 {link.kind === "link" ? "本地 link" : link.kind === "github" ? "仓库链接" : link.kind === "tgz" ? "打包产物" : "该规格"}
            </Button>
          )}
          {tab === "clone" && (
            <Button
              size="sm"
              disabled={!cloneInput.trim()}
              onClick={() => {
                onCloneInstall(
                  {
                    url: normalizeGitUrl(cloneInput),
                    gitRef: cloneRef.trim() || refOf(cloneInput),
                    subPath: clonePath.trim() || null,
                    build: cloneBuild,
                  },
                  accelOn,
                );
                onClose();
              }}
            >
              <FolderGit2 /> Clone 并 link 安装到 {profile}
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
