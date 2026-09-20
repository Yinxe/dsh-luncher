import { useCallback, useEffect, useState } from "react";
import {
  FolderGit2, FolderOpen, GitPullRequest, Loader2, PackagePlus, RefreshCw, Trash2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { api } from "../api";
import type { ClonedPlugin } from "../types";

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

/**
 * 本地克隆仓库（clone + link 安装的落点 ~/.dsh-launcher/git-plugins）。
 * 更新方式就是 git pull：这里一键执行 pull →（可选）构建 → 重新 link。
 */
export default function ClonedReposCard({ profile, busy, onToast, onPull, onLinkInstall }: Props) {
  const [repos, setRepos] = useState<ClonedPlugin[]>([]);
  const [loading, setLoading] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<ClonedPlugin | null>(null);

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
      try {
        await api.deleteClonedPlugin(repo.dirName);
        onToast("ok", `已删除本地克隆 ${repo.dirName}`);
        await load();
      } catch (e) {
        onToast("err", String(e));
      }
    },
    [load, onToast],
  );

  return (
    <Card className="p-4">
      <div className="mb-2.5 flex flex-wrap items-center gap-2.5">
        <div className="text-[13px] font-semibold">
          本地克隆仓库
          <span className="ml-2 text-xs font-normal text-muted-foreground">
            ~/.dsh-launcher/git-plugins · link 安装会装进 <span className="font-mono text-foreground">{profile}</span> · 更新方式 git pull
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
                  disabled={busy}
                  title="git pull --ff-only 后重新 link 安装"
                  onClick={() => onPull(r, plugin?.path || null, !(plugin?.libOk ?? true))}
                >
                  <GitPullRequest /> 更新（git pull）
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-6 px-2 text-[11px] text-destructive hover:bg-destructive/10 hover:text-destructive"
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
                  {r.candidates.slice(0, 8).map((c) => (
                    <span
                      key={c.path || "."}
                      className="inline-flex items-center gap-1 rounded-full border border-border px-2 py-0.5"
                    >
                      <span className="font-mono text-[10.5px]">{c.name ?? c.path ?? "（根目录）"}</span>
                      {!c.ready && <Badge variant="warning" className="text-[9px]">非插件包</Badge>}
                      <button
                        className="text-[10px] text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                        disabled={busy}
                        title={`dsh plugin --profile ${profile} add ${c.installSpec}（装进 ${profile}）`}
                        onClick={() => onLinkInstall(c.installSpec)}
                      >
                        <PackagePlus className="inline h-3 w-3" /> link
                      </button>
                    </span>
                  ))}
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

      <AlertDialog open={pendingDelete != null} onOpenChange={(o) => !o && setPendingDelete(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除本地克隆 {pendingDelete?.dirName}？</AlertDialogTitle>
            <AlertDialogDescription>
              只删除 ~/.dsh-launcher/git-plugins 下的这份克隆（含未提交的本地改动）。
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
