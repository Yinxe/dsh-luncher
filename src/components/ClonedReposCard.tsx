import { useCallback, useEffect, useRef, useState } from "react";
import {
  FolderGit2, FolderOpen, GitPullRequest, Loader2, PackagePlus, RefreshCw, Trash2, TriangleAlert,
} from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { api } from "../api";
import type { ClonedPlugin, PluginCandidate } from "../types";

interface Props {
  /** 当前 profile：link 安装会装进它，卡片上要说清 */
  profile: string;
  /** 父级 busy（有任务在跑时禁用操作） */
  busy: boolean;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
  /** git pull 更新某个克隆（父级负责起任务） */
  onPull: (repo: ClonedPlugin, subPath: string | null, build: boolean) => void;
  /** 把克隆里的某子包 link 安装进当前 profile */
  onLinkInstall: (spec: string) => void;
}

/** 确认框里的一行「标签 + 值」（窄列，值可换行不撑破弹窗） */
function DetailRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-start gap-2">
      <span className="w-14 shrink-0 text-[10.5px] leading-relaxed text-muted-foreground">{label}</span>
      <span className="min-w-0 flex-1 text-[11.5px] leading-relaxed break-all">{children}</span>
    </div>
  );
}

/**
 * 本地克隆仓库（clone + link 安装的落点 ~/.dsh-starter/git-plugins）。
 * 更新方式就是 git pull：这里一键执行 pull →（可选）构建 → 重新 link。
 */
export default function ClonedReposCard({ profile, busy, onToast, onPull, onLinkInstall }: Props) {
  const [repos, setRepos] = useState<ClonedPlugin[]>([]);
  const [loading, setLoading] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<ClonedPlugin | null>(null);
  // 待确认的 link 安装：从克隆里挑子包装进 profile 前必须过一次确认框
  const [pendingLink, setPendingLink] = useState<
    { repo: ClonedPlugin; candidate: PluginCandidate } | null
  >(null);
  // 删除 + 重新加载在途守卫：避免并发删除时旧的 load() 把已删的行又填回来
  const deletingRef = useRef(false);
  const [deleting, setDeleting] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setRepos(await api.listClonedPlugins());
    } catch (e) {
      onToast("err", `读取克隆仓库失败: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [onToast]);

  useEffect(() => {
    load();
  }, [load]);

  const reveal = useCallback(async () => {
    try {
      const dir = await api.gitPluginsDir();
      await api.reveal(dir);
    } catch (e) {
      onToast("err", String(e));
    }
  }, [onToast]);

  const doDelete = useCallback(
    async (repo: ClonedPlugin) => {
      if (deletingRef.current) return;
      deletingRef.current = true;
      setDeleting(true);
      try {
        await api.deleteClonedPlugin(repo.dirName);
        onToast("ok", `已删除本地克隆 ${repo.dirName}`);
        await load();
      } catch (e) {
        onToast("err", String(e));
      } finally {
        deletingRef.current = false;
        setDeleting(false);
      }
    },
    [load, onToast],
  );

  const linkLabel = pendingLink
    ? pendingLink.candidate.name ?? (pendingLink.candidate.path || "（根目录）")
    : "";

  return (
    <Card className="p-4">
      <div className="mb-2.5 flex flex-wrap items-center gap-2.5">
        <div className="text-[13px] font-semibold">
          本地克隆仓库
          <span className="ml-2 text-xs font-normal text-muted-foreground">
            ~/.dsh-starter/git-plugins · link 安装会装进 <span className="font-mono text-foreground">{profile}</span> · 更新方式 git pull
          </span>
        </div>
        <span className="flex-1" />
        <Badge variant="outline">{repos.length}</Badge>
        <Button size="sm" variant="outline" onClick={reveal} title="在文件管理器中打开 git-plugins">
          <FolderOpen /> 打开目录
        </Button>
        <Button size="sm" variant="outline" disabled={loading} onClick={load}>
          {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />} 刷新
        </Button>
      </div>

      <div className="space-y-2">
        {repos.map((r) => {
          const plugin = r.candidates.find((c) => c.ready) ?? r.candidates[0];
          return (
            <div key={r.dirName} className="rounded-lg border border-border bg-background/50 px-3 py-2">
              <div className="flex flex-wrap items-center gap-2">
                <FolderGit2 className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                <span className="min-w-0 truncate font-mono text-[12.5px] font-medium" title={r.path}>
                  {r.dirName}
                </span>
                {r.branch && <Badge variant="outline" className="font-mono text-[10px]">{r.branch}</Badge>}
                {r.commit && <Badge variant="secondary" className="font-mono text-[10px]">{r.commit.slice(0, 7)}</Badge>}
                {r.dirty && <Badge variant="warning" className="text-[10px]">有本地改动</Badge>}
                <span className="flex-1" />
                <Button
                  size="sm"
                  variant="outline"
                  className="h-6 px-2 text-[11px]"
                  disabled={busy || deleting}
                  title="git pull --ff-only 后重新 link 安装"
                  onClick={() => onPull(r, plugin?.path || null, !(plugin?.libOk ?? true))}
                >
                  <GitPullRequest /> 更新（git pull）
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-6 px-2 text-[11px] text-destructive hover:bg-destructive/10 hover:text-destructive"
                  disabled={busy || deleting}
                  onClick={() => setPendingDelete(r)}
                  title="删除本地克隆目录（不影响已安装依赖）"
                >
                  <Trash2 /> 删除
                </Button>
              </div>

              <div className="mt-1 truncate text-[10.5px] text-muted-foreground" title={r.url ?? ""}>
                {r.url ?? "（无 origin）"}
                {r.subject ? ` · ${r.subject}` : ""}
              </div>

              {r.candidates.length > 0 && (
                <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
                  {r.candidates.slice(0, 8).map((c) => {
                    const canInstall = c.ready;
                    const label = c.name ?? c.path ?? "（根目录）";
                    return (
                      <span
                        key={c.path || "."}
                        className="inline-flex items-center gap-1 rounded-full border border-border px-2 py-0.5"
                      >
                        <span className="font-mono text-[10.5px]">{label}</span>
                        {!c.ready && <Badge variant="warning" className="text-[9px]">非插件包</Badge>}
                        {c.ready && !c.libOk && (
                          <Badge variant="destructive" className="text-[9px]" title="仓库里没有 lib/ 构建产物">
                            lib/ 缺失
                          </Badge>
                        )}
                        <button
                          type="button"
                          className="text-[10px] text-muted-foreground underline-offset-2 enabled:hover:text-foreground enabled:hover:underline disabled:cursor-not-allowed disabled:opacity-50"
                          disabled={busy || deleting || !canInstall}
                          title={
                            !canInstall
                              ? `该子包没有声明 dsh.bundle，不能作为插件安装`
                              : `link 安装 ${label} 到 ${profile}${
                                  c.libOk ? "" : "（缺 lib/ 构建产物，装后可能校验不通过）"
                                } —— 会先弹确认框`
                          }
                          onClick={() => setPendingLink({ repo: r, candidate: c })}
                        >
                          <PackagePlus className="inline h-3 w-3" /> link
                        </button>
                      </span>
                    );
                  })}
                  {r.candidates.length > 8 && (
                    <span className="text-[10px] text-muted-foreground">+{r.candidates.length - 8}</span>
                  )}
                </div>
              )}
            </div>
          );
        })}
        {repos.length === 0 && !loading && (
          <div className="py-3 text-center text-xs text-muted-foreground">
            还没有克隆仓库。用「安装插件 → Clone 仓库」把远端仓库拉到本地并 link 安装
          </div>
        )}
      </div>

      {/* link 安装确认：装进哪个 profile、装的是哪个子包、来源是哪份克隆，一次说清；
          克隆里缺 lib/ 的后果（装后校验会卸掉）也在这里交代，不再只藏在一个 hover 提示里 */}
      <AlertDialog open={pendingLink != null} onOpenChange={(o) => !o && setPendingLink(null)}>
        <AlertDialogContent className="max-w-[min(92vw,32rem)]! sm:max-w-[32rem]!">
          <AlertDialogHeader>
            <AlertDialogTitle>
              link 安装 <span className="font-mono break-all">{linkLabel}</span>？
            </AlertDialogTitle>
            <AlertDialogDescription>
              走官方 dsh plugin 命令，把这份本地克隆软链进 profile「{profile}」——
              之后改克隆里的源码即时生效，不用重装。
            </AlertDialogDescription>
          </AlertDialogHeader>

          {pendingLink && (
            <div className="space-y-1.5 rounded-lg border border-border bg-muted/30 px-3 py-2.5">
              <DetailRow label="目标 profile">
                <span className="font-mono font-semibold">{profile}</span>
              </DetailRow>
              <DetailRow label="来源仓库">
                <span className="font-mono">{pendingLink.repo.dirName}</span>
                {pendingLink.repo.branch ? ` · ${pendingLink.repo.branch}` : ""}
                {pendingLink.repo.commit ? ` @ ${pendingLink.repo.commit.slice(0, 7)}` : ""}
                {pendingLink.repo.dirty ? " · 有本地改动" : ""}
              </DetailRow>
              <DetailRow label="子包路径">
                <span className="font-mono">
                  {pendingLink.candidate.path || "仓库根目录"}
                  {pendingLink.candidate.version ? ` · v${pendingLink.candidate.version}` : ""}
                </span>
              </DetailRow>
              <DetailRow label="安装规格">
                <span className="font-mono text-[10.5px] text-muted-foreground">
                  {pendingLink.candidate.installSpec}
                </span>
              </DetailRow>
              <DetailRow label="包声明">
                <span className="flex flex-wrap items-center gap-1.5">
                  {pendingLink.candidate.hasBundle ? (
                    <Badge variant="success" className="text-[10px]">dsh 插件包</Badge>
                  ) : (
                    <Badge variant="warning" className="text-[10px]">未声明 dsh.bundle</Badge>
                  )}
                  {pendingLink.candidate.libOk ? (
                    <Badge variant="outline" className="text-[10px]">lib/ 已就绪</Badge>
                  ) : (
                    <Badge variant="destructive" className="text-[10px]">lib/ 缺失</Badge>
                  )}
                  {pendingLink.candidate.workspaceMember && pendingLink.candidate.path !== "" && (
                    <Badge variant="secondary" className="text-[10px]">workspace 子包</Badge>
                  )}
                </span>
              </DetailRow>
            </div>
          )}

          {pendingLink && !pendingLink.candidate.libOk && (
            <Alert variant="destructive" className="py-1.5">
              <TriangleAlert />
              <AlertDescription className="text-[11px] leading-relaxed">
                这个子包在仓库里没有 <span className="font-mono">lib/</span> 构建产物，dsh 加载不了它 ——
                装后校验会判定「没有可加载入口」并当场卸掉。先给这份克隆点「更新（git pull）」（会顺带
                pnpm install + build），或在克隆目录里手动构建，再回来安装。
              </AlertDescription>
            </Alert>
          )}

          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                const target = pendingLink;
                setPendingLink(null);
                if (target) onLinkInstall(target.candidate.installSpec);
              }}
            >
              <PackagePlus /> 安装
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog open={pendingDelete != null} onOpenChange={(o) => !o && setPendingDelete(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除本地克隆 {pendingDelete?.dirName}？</AlertDialogTitle>
            <AlertDialogDescription>
              只删除 ~/.dsh-starter/git-plugins 下的这份克隆（含未提交的本地改动）。
              已 link 安装的插件会因源目录消失而失效，需要先卸载或重新指向其他源。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                const repo = pendingDelete;
                setPendingDelete(null);
                if (repo) doDelete(repo);
              }}
            >
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Card>
  );
}
