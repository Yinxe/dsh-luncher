import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Copy, Eye, EyeOff, FolderOpen, KeyRound, Loader2, MessageSquare, Pencil, Plus, RotateCcw, Save,
  Trash2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { api } from "../api";
import type { CredentialFile, CredentialRef } from "../types";

interface Props {
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

/** 凭据名称合法性：非空、无空白/控制字符（与后端 write_refs 校验一致，首尾空白会被 trim） */
function validName(n: string): boolean {
  return n.length > 0 && n.length <= 128 && !/[\s\u0000-\u001f\u007f]/.test(n);
}

/**
 * 注释规范化：去掉首尾空白与开头的 `#`（用户可能顺手带上），空串 = 没有注释。
 * 换行会被压成空格 —— 注释在文件里是**一行** `# …`，多行会把 YAML 注释块撑坏。
 */
function normalizeNote(n: string): string | null {
  const t = n.replace(/\s+/g, " ").trim().replace(/^#+\s*/, "").trim();
  return t === "" ? null : t;
}

/** 管理 $DSH_HOME/.credentials.yaml：表格列 key（名称）+ 注释，值仅在详情/编辑弹窗中显示 */
export default function CredentialsView({ onToast }: Props) {
  const [data, setData] = useState<CredentialFile | null>(null);
  const [refs, setRefs] = useState<CredentialRef[]>([]);
  const [baseline, setBaseline] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [addName, setAddName] = useState("");
  const [addValue, setAddValue] = useState("");
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  /** 详情/编辑弹窗：detailName 非空即打开；view 只读展示值，edit 编辑名称与值 */
  const [detailName, setDetailName] = useState<string | null>(null);
  const [detailMode, setDetailMode] = useState<"view" | "edit">("view");
  const [detailRevealed, setDetailRevealed] = useState(false);
  const [editName, setEditName] = useState("");
  const [editValue, setEditValue] = useState("");
  const [editNote, setEditNote] = useState("");
  const [addNote, setAddNote] = useState("");
  /** 列表里就地编辑注释：editNoteRow 非空即该行在编辑 */
  const [editNoteRow, setEditNoteRow] = useState<string | null>(null);
  const [noteDraft, setNoteDraft] = useState("");

  const dirty = useMemo(() => JSON.stringify(refs) !== baseline, [refs, baseline]);

  const reload = useCallback(async () => {
    try {
      const d = await api.getCredentials();
      setData(d);
      setRefs(d.refs);
      setBaseline(JSON.stringify(d.refs));
    } catch (e) {
      onToast("err", `读取凭据失败: ${e}`);
    }
  }, [onToast]);

  useEffect(() => {
    reload();
  }, [reload]);

  const save = useCallback(async () => {
    setBusy(true);
    try {
      await api.writeCredentialRefs(refs);
      onToast("ok", "凭据已保存（原文件已备份为 .credentials.starter-bak）");
      await reload();
    } catch (e) {
      onToast("err", String(e));
    } finally {
      setBusy(false);
    }
  }, [refs, onToast, reload]);

  const create = useCallback(async () => {
    setBusy(true);
    try {
      await api.writeCredentialRefs([]);
      onToast("ok", "已创建空凭据文件");
      await reload();
    } catch (e) {
      onToast("err", String(e));
    } finally {
      setBusy(false);
    }
  }, [onToast, reload]);

  const confirmDelete = useCallback(() => {
    if (!pendingDelete) return;
    const name = pendingDelete;
    setPendingDelete(null);
    setRefs((rs) => rs.filter((r) => r.name !== name));
  }, [pendingDelete]);

  const confirmAdd = useCallback(() => {
    const name = addName.trim();
    if (!validName(name) || refs.some((r) => r.name === name)) return;
    setRefs((rs) => [...rs, { name, value: addValue, note: normalizeNote(addNote) }]);
    setAddOpen(false);
    onToast("info", `已添加「${name}」，点「保存」写入文件`);
  }, [addName, addValue, addNote, refs, onToast]);

  const openDetail = useCallback((name: string) => {
    setDetailName(name);
    setDetailMode("view");
    setDetailRevealed(false);
  }, []);

  const startEdit = useCallback(() => {
    const cur = refs.find((r) => r.name === detailName);
    if (!cur) return;
    setEditName(cur.name);
    setEditValue(cur.value);
    setEditNote(cur.note ?? "");
    setDetailMode("edit");
  }, [refs, detailName]);

  const applyEdit = useCallback(() => {
    const oldName = detailName;
    if (!oldName) return;
    const name = editName.trim();
    if (!validName(name) || refs.some((r) => r.name === name && r.name !== oldName)) return;
    setRefs((rs) =>
      rs.map((r) => (r.name === oldName ? { name, value: editValue, note: normalizeNote(editNote) } : r))
    );
    setDetailName(null);
    onToast(
      "info",
      name === oldName
        ? `已修改「${name}」，点「保存」写入文件`
        : `已重命名为「${name}」，点「保存」写入文件`
    );
  }, [detailName, editName, editValue, editNote, refs, onToast]);

  /** 列表里就地改注释：空字符串 = 不要注释（保存时会删掉那行 `# …`） */
  const commitNote = useCallback((name: string) => {
    const next = normalizeNote(noteDraft);
    setRefs((rs) => rs.map((r) => (r.name === name ? { ...r, note: next } : r)));
    setEditNoteRow(null);
  }, [noteDraft]);

  const copyText = useCallback(async (text: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text);
      onToast("ok", `${label} 已复制到剪贴板`);
    } catch {
      onToast("err", "复制失败：请在详情中显示明文后手动复制");
    }
  }, [onToast]);

  const current = detailName != null ? refs.find((r) => r.name === detailName) ?? null : null;
  const editTrimmed = editName.trim();
  const editDuplicated =
    editTrimmed !== "" && refs.some((r) => r.name === editTrimmed && r.name !== detailName);
  const editNameOk = validName(editTrimmed) && !editDuplicated;

  if (!data) {
    return (
      <div className="flex h-40 items-center justify-center gap-2 text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" /> 正在读取凭据…
      </div>
    );
  }

  const dirPath = data.path.replace(/[/\\][^/\\]+$/, "");

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="text-base font-semibold">凭据管理</h2>
        <Badge variant="outline" className="font-mono" title={data.path}>
          ~/.dsh/.credentials.yaml
        </Badge>
        {data.version != null && (
          <Badge variant="secondary" className="font-mono">schema v{data.version}</Badge>
        )}
        {dirty && <Badge variant="warning">未保存</Badge>}
        <span className="flex-1" />
        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          title={`在文件管理器中打开：${dirPath}`}
          onClick={() => api.reveal(dirPath).catch((e) => onToast("err", String(e)))}
        >
          <FolderOpen /> 打开所在目录
        </Button>
        <Button size="sm" variant="outline" disabled={busy || !dirty} onClick={reload}>
          <RotateCcw /> 还原
        </Button>
        <Button size="sm" disabled={busy || !dirty} onClick={save}>
          {busy && <Loader2 className="animate-spin" />} <Save /> 保存（自动备份）
        </Button>
      </div>
      <div className="rounded-lg border border-amber-500/25 bg-amber-500/5 p-2.5 text-[11.5px] leading-relaxed text-muted-foreground">
        dsh 的凭据存储（$DSH_HOME/.credentials.yaml）：
        <span className="font-mono text-foreground">refs</span>
        {" "}是各处以名字引用的 API Key / 令牌（如 DEEPSEEK_API_KEY、QQ_APP_SECRET），支持增删改，
        列表只显示名称，值仅在「详情 / 编辑」弹窗中可见；
        <span className="text-foreground">注释</span>
        {" "}取自文件里键<b className="font-semibold text-foreground">正上方那一行</b>注释（<span className="font-mono"># …</span>），
        可以在列表里直接点着改：填内容就会写成键上方的一行注释，清空则删掉那一行；
        <span className="font-mono text-foreground">records</span>
        {" "}是 dsh 内部会话凭据（如 web 连接密钥），由 dsh 自行维护，此处只读展示。
        保存为整表覆写 refs，records / version 等其余内容原样保留；写前自动备份（仅保留一份），
        文件权限自动收紧为仅本用户可读写。dsh 实例运行中也可能更新凭据，建议此时避免编辑保存（后保存者生效）。
      </div>

      {!data.exists ? (
        <Card className="p-10 text-center text-muted-foreground">
          凭据文件不存在——dsh 首次运行后会自动创建。
          <div className="mt-3">
            <Button size="sm" variant="outline" disabled={busy} onClick={create}>
              <Plus /> 创建空凭据文件
            </Button>
          </div>
        </Card>
      ) : (
        <>
          <Card className="gap-0 py-0">
            <div className="flex items-center gap-2 border-b border-border px-4 py-2.5">
              <KeyRound className="h-3.5 w-3.5 text-muted-foreground" />
              <span className="eyebrow">refs · 命名凭据</span>
              <Badge variant="secondary">{refs.length}</Badge>
              <span className="flex-1" />
              <Button
                size="sm"
                variant="outline"
                onClick={() => { setAddName(""); setAddValue(""); setAddNote(""); setAddOpen(true); }}
              >
                <Plus /> 添加凭据
              </Button>
            </div>
            {refs.length > 0 ? (
              <Table>
                <TableHeader>
                  <TableRow className="bg-muted/40 hover:bg-muted/40">
                    <TableHead className="h-9 pl-4 text-[10.5px] uppercase tracking-wider">名称</TableHead>
                    <TableHead className="h-9 text-[10.5px] uppercase tracking-wider">
                      注释（键上方那一行）
                    </TableHead>
                    <TableHead className="h-9 text-[10.5px] uppercase tracking-wider">长度</TableHead>
                    <TableHead className="h-9 pr-4 text-right text-[10.5px] uppercase tracking-wider">操作</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {refs.map((r) => (
                    <TableRow key={r.name}>
                      <TableCell className="max-w-[420px] pl-4">
                        <Button
                          variant="ghost"
                          className="h-6 max-w-full justify-start px-1.5 font-mono text-[12.5px] font-semibold"
                          title={`查看详情：${r.name}`}
                          onClick={() => openDetail(r.name)}
                        >
                          <span className="truncate">{r.name}</span>
                        </Button>
                      </TableCell>
                      <TableCell className="max-w-[380px] min-w-[180px]">
                        {editNoteRow === r.name ? (
                          <Input
                            autoFocus
                            className="h-7 text-[11.5px]"
                            placeholder="留空 = 不要注释"
                            value={noteDraft}
                            onChange={(e) => setNoteDraft(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") {
                                e.preventDefault();
                                commitNote(r.name);
                              } else if (e.key === "Escape") {
                                e.preventDefault();
                                setEditNoteRow(null);
                              }
                            }}
                            onBlur={() => commitNote(r.name)}
                          />
                        ) : (
                          <button
                            type="button"
                            className="group/note flex h-6 w-full max-w-full items-center gap-1.5 rounded px-1.5 text-left transition-colors hover:bg-muted/60"
                            title={
                              r.note
                                ? `${r.note}\n\n点击修改（清空即删掉这行注释）`
                                : "点击添加注释：会写成这个键上方的一行注释"
                            }
                            onClick={() => {
                              setEditNoteRow(r.name);
                              setNoteDraft(r.note ?? "");
                            }}
                          >
                            {r.note ? (
                              <span className="truncate text-[11.5px] leading-relaxed">{r.note}</span>
                            ) : (
                              <span className="text-[11.5px] text-muted-foreground/60">添加注释…</span>
                            )}
                            <Pencil className="ml-auto size-3 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover/note:opacity-70" />
                          </button>
                        )}
                      </TableCell>
                      <TableCell className="font-mono text-xs text-muted-foreground">
                        {r.value.length > 0 ? `${r.value.length} 字符` : "空"}
                      </TableCell>
                      <TableCell className="pr-4 text-right">
                        <div className="inline-flex items-center gap-0.5">
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8"
                            title="查看详情"
                            onClick={() => openDetail(r.name)}
                          >
                            <Eye />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8"
                            title="复制值（不显示明文）"
                            onClick={() => copyText(r.value, r.name)}
                          >
                            <Copy />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 text-destructive hover:text-destructive"
                            title="删除该凭据"
                            onClick={() => setPendingDelete(r.name)}
                          >
                            <Trash2 />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            ) : (
              <div className="px-4 py-8 text-center text-sm text-muted-foreground">
                暂无凭据——点「添加凭据」新建（名称即凭据的引用键，如 DEEPSEEK_API_KEY；注释可选，
                会写成该键上方的一行注释）
              </div>
            )}
          </Card>

          {data.records.length > 0 && (
            <Card className="gap-0 py-0">
              <div className="flex items-center gap-2 border-b border-border px-4 py-2.5">
                <Badge variant="outline" className="font-mono">records</Badge>
                <span className="text-[11.5px] text-muted-foreground">
                  dsh 内部凭据记录（自管理，只读）
                </span>
                <span className="flex-1" />
                <Badge variant="secondary">{data.records.length}</Badge>
              </div>
              <Table>
                <TableHeader>
                  <TableRow className="bg-muted/40 hover:bg-muted/40">
                    <TableHead className="h-9 pl-4 text-[10.5px] uppercase tracking-wider">记录键</TableHead>
                    <TableHead className="h-9 text-[10.5px] uppercase tracking-wider">kind</TableHead>
                    <TableHead className="h-9 text-[10.5px] uppercase tracking-wider">secret</TableHead>
                    <TableHead className="h-9 pr-4 text-[10.5px] uppercase tracking-wider">payload 字段</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {data.records.map((rec) => (
                    <TableRow key={rec.key}>
                      <TableCell className="max-w-72 truncate pl-4 font-mono text-[12.5px]" title={rec.key}>
                        {rec.key}
                      </TableCell>
                      <TableCell>
                        {rec.kind ? <Badge variant="outline" className="font-mono">{rec.kind}</Badge> : "—"}
                      </TableCell>
                      <TableCell className="font-mono text-xs text-muted-foreground">
                        {rec.secretLength != null ? `••••••（${rec.secretLength} 字符）` : "—"}
                      </TableCell>
                      <TableCell className="pr-4 font-mono text-xs text-muted-foreground">
                        {rec.payloadKeys.length > 0 ? rec.payloadKeys.join(", ") : "—"}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </Card>
          )}
        </>
      )}

      {/* 详情 / 编辑 */}
      <Dialog
        open={detailName != null && current != null}
        onOpenChange={(o) => !o && setDetailName(null)}
      >
        {/* grid 轨道锁 minmax(0,1fr)：Textarea 的 field-sizing-content 以长令牌撑爆列宽 */}
        <DialogContent className="grid-cols-[minmax(0,1fr)] sm:max-w-lg">
          {detailMode === "view" && current && (
            <>
              <DialogHeader>
                <DialogTitle className="flex items-center gap-2 font-mono">
                  <KeyRound className="h-4 w-4 shrink-0" />
                  <span className="truncate">{current.name}</span>
                </DialogTitle>
                <DialogDescription>凭据详情：值默认隐藏，可显示明文或直接复制。</DialogDescription>
              </DialogHeader>
              {current.note && (
                <div className="flex items-start gap-2 rounded-md border border-border bg-muted/30 px-2.5 py-2">
                  <MessageSquare className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                  <div className="min-w-0 flex-1">
                    <div className="text-[11px] text-muted-foreground">
                      注释（写在 <span className="font-mono">{current.name}</span> 上方那一行）
                    </div>
                    <div className="text-[12.5px] leading-relaxed break-words">{current.note}</div>
                  </div>
                </div>
              )}
              <div className="space-y-1.5">
                <div className="text-[11.5px] text-muted-foreground">值 · {current.value.length} 字符</div>
                <div className="max-h-60 min-h-20 overflow-y-auto rounded-md border bg-muted/30 p-3 font-mono text-xs break-all whitespace-pre-wrap">
                  {detailRevealed
                    ? current.value
                    : current.value.length > 0
                    ? `••••••••（${current.value.length} 字符）`
                    : "（空值）"}
                </div>
              </div>
              <DialogFooter>
                <div className="flex w-full flex-wrap items-center justify-between gap-2">
                  <div className="flex gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={current.value.length === 0}
                      onClick={() => setDetailRevealed((v) => !v)}
                    >
                      {detailRevealed ? <EyeOff /> : <Eye />} {detailRevealed ? "隐藏明文" : "显示明文"}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => copyText(current.value, current.name)}
                    >
                      <Copy /> 复制
                    </Button>
                  </div>
                  <div className="flex gap-2">
                    <Button variant="outline" size="sm" onClick={() => setDetailName(null)}>
                      关闭
                    </Button>
                    <Button size="sm" onClick={startEdit}>
                      <Pencil /> 编辑
                    </Button>
                  </div>
                </div>
              </DialogFooter>
            </>
          )}
          {detailMode === "edit" && (
            <>
              <DialogHeader>
                <DialogTitle className="flex items-center gap-2">
                  <Pencil className="h-4 w-4" /> 编辑凭据
                </DialogTitle>
                <DialogDescription>
                  修改仅更新本地列表，点页面右上「保存（自动备份）」才写入文件；改名称即重命名该凭据。
                </DialogDescription>
              </DialogHeader>
              <div className="space-y-3">
                <div className="space-y-1.5">
                  <Label htmlFor="cred-edit-name">名称</Label>
                  <Input
                    id="cred-edit-name"
                    autoFocus
                    className="font-mono"
                    value={editName}
                    onChange={(e) => setEditName(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && applyEdit()}
                  />
                  {editTrimmed !== "" && !validName(editTrimmed) && (
                    <p className="text-[11.5px] text-destructive">名称不能为空或包含空白 / 控制字符</p>
                  )}
                  {editDuplicated && (
                    <p className="text-[11.5px] text-destructive">凭据「{editTrimmed}」已存在</p>
                  )}
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="cred-edit-note">注释（可选）</Label>
                  <Input
                    id="cred-edit-note"
                    placeholder="例如：DeepSeek 官方 key（会写成键上方的一行注释）"
                    value={editNote}
                    onChange={(e) => setEditNote(e.target.value)}
                  />
                  <p className="text-[11px] text-muted-foreground">
                    留空即删掉这个键上方的注释行；文件里更上面的注释（如小节说明）不受影响。
                  </p>
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="cred-edit-value">值</Label>
                  <Textarea
                    id="cred-edit-value"
                    className="min-h-28 max-h-48 overflow-y-auto font-mono text-xs"
                    placeholder="API Key / 令牌内容（可为长文本，如 Cookie）"
                    value={editValue}
                    onChange={(e) => setEditValue(e.target.value)}
                  />
                </div>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => { setDetailMode("view"); setDetailRevealed(false); }}>
                  返回详情
                </Button>
                <Button disabled={!editNameOk} onClick={applyEdit}>应用</Button>
              </DialogFooter>
            </>
          )}
        </DialogContent>
      </Dialog>

      {/* 添加凭据 */}
      <Dialog open={addOpen} onOpenChange={(o) => !o && setAddOpen(false)}>
        <DialogContent className="grid-cols-[minmax(0,1fr)] sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <KeyRound className="h-4 w-4" /> 添加凭据
            </DialogTitle>
            <DialogDescription>
              名称即凭据的引用键（如 DEEPSEEK_API_KEY），不能含空白或控制字符；
              值可为任意长文本（如 Cookie）；注释会写成该键上方的一行 <span className="font-mono"># …</span>。
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1.5">
              <Label htmlFor="cred-name">名称</Label>
              <Input
                id="cred-name"
                autoFocus
                className="font-mono"
                placeholder="例如 DEEPSEEK_API_KEY"
                value={addName}
                onChange={(e) => setAddName(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && confirmAdd()}
              />
              {(() => {
                const t = addName.trim();
                if (t === "") return null;
                if (!validName(t)) {
                  return <p className="text-[11.5px] text-destructive">名称不能为空或包含空白 / 控制字符</p>;
                }
                if (refs.some((r) => r.name === t)) {
                  return <p className="text-[11.5px] text-destructive">凭据「{t}」已存在</p>;
                }
                return null;
              })()}
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="cred-note">注释（可选）</Label>
              <Input
                id="cred-note"
                placeholder="例如：DeepSeek 官方 key"
                value={addNote}
                onChange={(e) => setAddNote(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="cred-value">值</Label>
              <Textarea
                id="cred-value"
                className="min-h-20 max-h-48 overflow-y-auto font-mono text-xs"
                placeholder="API Key / 令牌内容（可为长文本，如 Cookie）"
                value={addValue}
                onChange={(e) => setAddValue(e.target.value)}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setAddOpen(false)}>取消</Button>
            <Button
              disabled={!validName(addName.trim()) || refs.some((r) => r.name === addName.trim())}
              onClick={confirmAdd}
            >
              添加
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 删除确认 */}
      <AlertDialog open={pendingDelete != null} onOpenChange={(o) => !o && setPendingDelete(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除凭据 {pendingDelete}？</AlertDialogTitle>
            <AlertDialogDescription>
              将从列表移除该凭据，点「保存」后写入文件（原文件自动备份，仅保留一份）。保存前可用「还原」放弃全部修改。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={confirmDelete}>
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
