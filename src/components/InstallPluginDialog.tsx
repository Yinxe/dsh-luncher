import { useCallback, useEffect, useState } from "react";
import { ExternalLink, GitBranch, Loader2, Plus, Search } from "lucide-react";
import {
  Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api } from "../api";
import type { GitHubRepoInfo, PackageSearchItem } from "../types";

interface Props {
  profile: string;
  open: boolean;
  onClose: () => void;
  /** 执行安装（父组件持有 busy / 日志流 / 重载） */
  onInstall: (spec: string) => void;
}

/**
 * 插件安装对话框：正式的「先搜索 → 看描述 → 再安装」流程。
 * - npm：registry 关键字搜索（含版本/描述/发布时间），也可手动输入精确规格
 * - GitHub：owner/repo 或仓库 URL → 预览描述/星标 → 安装
 *   （dsh plugin add 原样透传 pnpm 规格，github:owner/repo 与 #分支 均可）
 */
export default function InstallPluginDialog({ profile, open, onClose, onInstall }: Props) {
  const [tab, setTab] = useState<string>("npm");
  const [query, setQuery] = useState("");
  const [searching, setSearching] = useState(false);
  const [searchErr, setSearchErr] = useState<string | null>(null);
  const [results, setResults] = useState<PackageSearchItem[]>([]);
  const [manualSpec, setManualSpec] = useState("");
  const [ghInput, setGhInput] = useState("");
  const [ghPath, setGhPath] = useState("");
  const [ghLoading, setGhLoading] = useState(false);
  const [ghErr, setGhErr] = useState<string | null>(null);
  const [ghInfo, setGhInfo] = useState<GitHubRepoInfo | null>(null);

  // 打包产物直链：插件路径输入不适用
  const isTarballInput =
    /\.(tgz|tar\.gz|tar)$/i.test(ghInput.trim()) || ghInput.includes("/releases/download/");

  // 每次打开重置状态
  useEffect(() => {
    if (open) {
      setQuery("");
      setSearchErr(null);
      setResults([]);
      setManualSpec("");
      setGhInput("");
      setGhPath("");
      setGhErr(null);
      setGhInfo(null);
      setTab("npm");
    }
  }, [open]);

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

  const previewGithub = useCallback(async () => {
    let s = ghInput.trim();
    if (!s) return;
    // 独立的插件路径输入框 → 拼成 pnpm 的 #path: 参数（已有 fragment 用 & 组合）
    const p = ghPath.trim();
    if (p && !isTarballInput) {
      s = s.includes("#") ? `${s}&path:${p}` : `${s}#path:${p}`;
    }
    setGhLoading(true);
    setGhErr(null);
    setGhInfo(null);
    try {
      setGhInfo(await api.fetchGithubRepo(s));
    } catch (e) {
      setGhErr(String(e));
    } finally {
      setGhLoading(false);
    }
  }, [ghInput, ghPath, isTarballInput]);

  const install = (spec: string) => {
    if (!spec.trim()) return;
    onInstall(spec.trim());
    onClose();
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>安装插件</DialogTitle>
          <DialogDescription>
            安装到 profile「{profile}」（官方 dsh plugin 命令）。先搜索看描述，确认再装。
          </DialogDescription>
        </DialogHeader>

        <Tabs value={tab} onValueChange={setTab} className="gap-3">
          <TabsList>
            <TabsTrigger value="npm" className="text-xs">NPM 包</TabsTrigger>
            <TabsTrigger value="github" className="text-xs">GitHub 仓库</TabsTrigger>
          </TabsList>

          <TabsContent value="npm" className="space-y-3">
            <div className="flex items-center gap-2">
              <Input
                autoFocus
                className="h-8 font-mono text-xs"
                placeholder="搜索关键词，如 mcwiki、search、tts…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && search()}
              />
              <Button size="sm" variant="outline" disabled={searching || !query.trim()} onClick={search}>
                {searching ? <Loader2 className="animate-spin" /> : <Search />} 搜索
              </Button>
            </div>
            {searchErr && <p className="text-[11.5px] text-destructive">{searchErr}</p>}

            {results.length > 0 && (
              <div className="max-h-72 space-y-1.5 overflow-y-auto pr-1">
                {results.map((r) => (
                  <div key={`${r.name}@${r.version}`} className="rounded-lg border border-border bg-background/50 p-2.5">
                    <div className="flex items-center gap-2">
                      <span className="min-w-0 flex-1 truncate font-mono text-[12.5px] font-semibold">{r.name}</span>
                      <Badge variant="outline" className="font-mono text-[10px]">{r.version}</Badge>
                      {r.publishedAt && (
                        <span className="shrink-0 text-[10px] text-muted-foreground">{r.publishedAt.slice(0, 10)}</span>
                      )}
                      <Button size="sm" variant="outline" className="h-6 px-2 text-[11px]" onClick={() => install(`${r.name}@${r.version}`)}>
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
              <p className="py-2 text-center text-[11.5px] text-muted-foreground">输入关键词搜索 registry</p>
            )}

            <Separator />
            <div className="flex items-center gap-2">
              <Label className="shrink-0 text-[11px] text-muted-foreground">精确规格</Label>
              <Input
                className="h-7 flex-1 font-mono text-xs"
                placeholder="如 @dshp/mcwiki-search@1.2.3"
                value={manualSpec}
                onChange={(e) => setManualSpec(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && install(manualSpec)}
              />
              <Button size="sm" variant="outline" disabled={!manualSpec.trim()} onClick={() => install(manualSpec)}>
                <Plus /> 安装
              </Button>
            </div>
          </TabsContent>

          <TabsContent value="github" className="space-y-3">
            <div className="space-y-1.5">
              <div className="flex items-center gap-2">
                <Input
                  autoFocus
                  className="h-8 font-mono text-xs"
                  placeholder="owner/repo、仓库链接（含 /tree/ 路径）或打包产物 .tgz 直链"
                  value={ghInput}
                  onChange={(e) => setGhInput(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && previewGithub()}
                />
                <Button size="sm" variant="outline" disabled={ghLoading || !ghInput.trim()} onClick={previewGithub}>
                  {ghLoading ? <Loader2 className="animate-spin" /> : <GitBranch />} 预览
                </Button>
              </div>
              <div className="flex items-center gap-2">
                <Input
                  className="h-7 flex-1 font-mono text-xs"
                  placeholder="插件在仓库中的路径（monorepo 必填），如 plugins/mcwiki-search"
                  value={ghPath}
                  disabled={isTarballInput}
                  onChange={(e) => setGhPath(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && previewGithub()}
                />
              </div>
              <p className="text-[10px] leading-relaxed text-muted-foreground">
                按插件规范，插件根目录（仓库根或上述路径）必须含有 lib/ 目录才能正常安装，预览时会自动校验；
                打包产物链接（.tgz/.tar.gz）则直接安装该文件。
              </p>
            </div>
            {ghErr && <p className="text-[11.5px] text-destructive">{ghErr}</p>}

            {ghInfo && (
              <div className="space-y-2 rounded-lg border border-border bg-background/50 p-3">
                <div className="flex items-center gap-2">
                  <GitBranch className="h-4 w-4 shrink-0" />
                  <span className="min-w-0 flex-1 truncate font-mono text-[12.5px] font-semibold">{ghInfo.fullName}</span>
                  <Badge variant="outline" className="font-mono text-[10px]">★ {ghInfo.stars}</Badge>
                  {ghInfo.license && <Badge variant="secondary" className="text-[10px]">{ghInfo.license}</Badge>}
                  {ghInfo.gitRef && <Badge variant="info" className="font-mono text-[10px]">#{ghInfo.gitRef}</Badge>}
                </div>
                <p className="line-clamp-3 text-[11px] leading-relaxed text-muted-foreground">
                  {ghInfo.description || "（无描述）"}
                </p>
                <div className="flex flex-wrap items-center gap-2 text-[10.5px]">
                  {ghInfo.pluginPath && (
                    <span className="font-mono text-muted-foreground">插件根目录：{ghInfo.pluginPath}</span>
                  )}
                  {ghInfo.libOk === true && <Badge variant="success" className="text-[10px]">已校验 lib/ 目录</Badge>}
                  {ghInfo.libOk === false && (
                    <Badge variant="destructive" className="text-[10px]">未找到 lib/ 目录</Badge>
                  )}
                  {ghInfo.libOk == null && (
                    <Badge variant="outline" className="text-[10px]">打包产物直装</Badge>
                  )}
                </div>
                {ghInfo.libOk === false && (
                  <p className="text-[11px] text-destructive">
                    按规范插件根目录必须包含 lib/ 目录，否则无法正常安装。请确认路径是否指向了插件根目录。
                  </p>
                )}
                {ghInfo.pushedAt && (
                  <p className="text-[10px] text-muted-foreground">最近推送：{ghInfo.pushedAt.slice(0, 10)}</p>
                )}
                <Button
                  size="sm"
                  className="w-full"
                  disabled={ghInfo.libOk === false}
                  onClick={() => install(ghInfo.installSpec)}
                >
                  <Plus /> 安装 {ghInfo.installSpec}
                </Button>
              </div>
            )}
            {!ghInfo && !ghErr && (
              <p className="py-2 text-center text-[11.5px] text-muted-foreground">
                输入仓库后先预览描述并校验 lib/ 目录，再决定安装；也可直接粘贴打包产物链接直装
              </p>
            )}
          </TabsContent>
        </Tabs>

        <p className="text-[10.5px] leading-relaxed text-muted-foreground">
          GitHub 等托管插件若带 prepare 构建脚本，pnpm 会要求放行：按 dsh 输出的提示把对应键加进
          pnpm-workspace.yaml 的 allowBuilds 后重跑安装即可。
        </p>
      </DialogContent>
    </Dialog>
  );
}
