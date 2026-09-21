import { useCallback, useEffect, useState } from "react";
import { Loader2, Trash2, Undo2 } from "lucide-react";
import {
  Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api } from "../api";
import type { DeletedProfile } from "../types";

interface Props {
  open: boolean;
  onClose: () => void;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
  /** 还原 / 彻底删除后通知外层刷新 profile 列表 */
  onChanged: () => void;
}

/** 回收站：删除的 profile 只是被移入 ~/.dsh-starter/deleted-profiles，可还原或彻底删除 */
export default function TrashDialog({ open, onClose, onToast, onChanged }: Props) {
  const [items, setItems] = useState<DeletedProfile[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [purgeTarget, setPurgeTarget] = useState<DeletedProfile | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await api.listDeletedProfiles());
    } catch (e) {
      onToast("err", `读取回收站失败: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [onToast]);

  useEffect(() => {
    if (open) reload();
  }, [open, reload]);

  const restore = async (it: DeletedProfile) => {
    setBusy(it.dirName);
    try {
      const path = await api.restoreDeletedProfile(it.dirName);
      onToast("ok", `已还原 profile「${it.name}」到 ${path}`);
      onChanged();
      await reload();
    } catch (e) {
      onToast("err", `还原失败: ${e}`);
    } finally {
      setBusy(null);
    }
  };

  const purge = async () => {
    if (!purgeTarget) return;
    const it = purgeTarget;
    setPurgeTarget(null);
    setBusy(it.dirName);
    try {
      await api.purgeDeletedProfile(it.dirName);
      onToast("ok", `已彻底删除「${it.name}」，不可再恢复`);
      await reload();
    } catch (e) {
      onToast("err", `彻底删除失败: ${e}`);
    } finally {
      setBusy(null);
    }
  };

  const fmtTime = (ts: number) => (ts > 0 ? new Date(ts).toLocaleString() : "未知时间");

  return (
    <>
      <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Trash2 className="h-4 w-4" /> 回收站
              {items.length > 0 && <Badge variant="secondary">{items.length}</Badge>}
            </DialogTitle>
            <DialogDescription>
              删除 profile 时不会直接销毁，而是移入 <span className="font-mono">~/.dsh-starter/deleted-profiles/</span>。
              可在这里还原（回到 profiles 目录）或彻底删除（不可恢复）。还原时若已存在同名
              profile 会被拒绝，避免覆盖。
            </DialogDescription>
          </DialogHeader>

          {loading ? (
            <div className="flex items-center justify-center gap-2 py-8 text-xs text-muted-foreground">
              <Loader2 className="h-3.5 w-3.5 animate-spin" /> 读取中…
            </div>
          ) : items.length === 0 ? (
            <div className="py-8 text-center text-xs text-muted-foreground">回收站是空的</div>
          ) : (
            <div className="max-h-[50vh] overflow-y-auto rounded-md border border-border">
              <Table>
                <TableHeader>
                  <TableRow className="bg-muted/40 hover:bg-muted/40">
                    <TableHead className="h-9 pl-4 text-[10.5px] uppercase tracking-wider">原 profile</TableHead>
                    <TableHead className="h-9 text-[10.5px] uppercase tracking-wider">删除时间</TableHead>
                    <TableHead className="h-9 pr-4 text-right text-[10.5px] uppercase tracking-wider">操作</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {items.map((it) => (
                    <TableRow key={it.dirName}>
                      <TableCell className="pl-4">
                        <div className="font-mono text-xs">{it.name}</div>
                        <div className="truncate text-[10.5px] text-muted-foreground" title={it.path}>
                          {it.dirName}
                        </div>
                      </TableCell>
                      <TableCell className="text-[11.5px] text-muted-foreground">{fmtTime(it.deletedAt)}</TableCell>
                      <TableCell className="pr-4 text-right">
                        <div className="flex items-center justify-end gap-2">
                          <Button
                            size="sm"
                            variant="outline"
                            disabled={busy != null}
                            onClick={() => restore(it)}
                            title="还原回 profiles 目录"
                          >
                            {busy === it.dirName ? <Loader2 className="animate-spin" /> : <Undo2 />} 还原
                          </Button>
                          <Button
                            size="sm"
                            variant="destructive"
                            disabled={busy != null}
                            onClick={() => setPurgeTarget(it)}
                            title="彻底删除，不可恢复"
                          >
                            <Trash2 /> 彻底删除
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
        </DialogContent>
      </Dialog>

      <AlertDialog open={purgeTarget != null} onOpenChange={(o) => !o && setPurgeTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>彻底删除「{purgeTarget?.name}」？</AlertDialogTitle>
            <AlertDialogDescription>
              该目录会被永久删除、<b>无法恢复</b>（它现在还在回收站里，可以改用「还原」）。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={purge}>
              彻底删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
