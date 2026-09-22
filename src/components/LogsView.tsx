import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Activity, Copy, FolderOpen, RefreshCw, Trash2 } from "lucide-react";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { api } from "../api";
import type { DiagnosticsExport, Settings, SystemLogsInfo } from "../types";

interface Props {
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
  settings: Settings;
  /** 保存日志级别（App 里乐观更新 + 失败回滚；后端运行期立即生效） */
  onSaveLogLevel: (v: string) => Promise<void>;
  onReveal: (path: string) => void;
}

/** 一条日志（可能含多行：panic 回溯等续行并入上一条） */
interface LogLine {
  level: "DEBUG" | "INFO" | "WARN" | "ERROR" | "";
  text: string;
}

/** diag.rs 的行格式：`<ISO>Z run=<id> pid=<pid> +<ms>ms LEVEL 正文` */
const LINE_RE = /^\S+Z run=\d+ pid=\d+ \+\d+ms (DEBUG|INFO|WARN|ERROR)\s/;

function parseLines(content: string): LogLine[] {
  const out: LogLine[] = [];
  for (const raw of content.split("\n")) {
    if (!raw) continue;
    const m = raw.match(LINE_RE);
    if (m) {
      out.push({ level: m[1] as LogLine["level"], text: raw });
    } else if (out.length > 0) {
      // 多行正文（回溯 / 命令输出）：并入上一条，级别随上一条过滤
      out[out.length - 1].text += `\n${raw}`;
    } else {
      out.push({ level: "", text: raw });
    }
  }
  return out;
}

function fmtSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

const LEVEL_OPTIONS = ["debug", "info", "warn", "error"] as const;
const LEVEL_FILTERS = ["all", "DEBUG", "INFO", "WARN", "ERROR"] as const;
// 单次最多渲染的日志行数：256KB 尾部可达数千行，全量渲染在低配 webview 上会卡
const RENDER_CAP = 800;

/** 系统日志页：按分类浏览 diag 日志尾部 + 级别/关键词过滤 + 日志级别设置 */
export default function LogsView({ onToast, settings, onSaveLogLevel, onReveal }: Props) {
  const [info, setInfo] = useState<SystemLogsInfo | null>(null);
  const [cat, setCat] = useState("app");
  const [lines, setLines] = useState<LogLine[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [levelFilter, setLevelFilter] = useState<(typeof LEVEL_FILTERS)[number]>("all");
  const [keyword, setKeyword] = useState("");
  const [auto, setAuto] = useState(false);
  const [loading, setLoading] = useState(false);
  const [levelBusy, setLevelBusy] = useState(false);
  const [diagBusy, setDiagBusy] = useState(false);
  // 诊断包弹窗：生成后直接在应用内查看全文（顶部可复制）
  const [diag, setDiag] = useState<DiagnosticsExport | null>(null);
  // 清理确认框的目标："all" = 全部类别，其余为分类名，null = 关闭
  const [clearTarget, setClearTarget] = useState<string | null>(null);
  const [clearing, setClearing] = useState(false);
  // 自动刷新的静默失败不打 toast（否则坏了会每 2 秒炸一次），只首报一次
  const autoErrOnce = useRef(false);
  const preRef = useRef<HTMLPreElement>(null);
  // 请求序号：切分类/连续刷新时丢弃过期响应，避免旧分类内容落到新分类页签下
  const contentSeq = useRef(0);
  // 用户是否停在底部附近（自动刷新时只在这种情况下跟随滚到底）
  const stickBottom = useRef(true);

  const loadInfo = useCallback(async () => {
    try {
      setInfo(await api.listSystemLogs());
    } catch (e) {
      onToast("err", `读取日志清单失败：${e}`);
    }
  }, [onToast]);

  const loadContent = useCallback(
    async (category: string, silent: boolean) => {
      const seq = ++contentSeq.current;
      setLoading(true);
      try {
        const r = await api.readSystemLog(category);
        if (seq !== contentSeq.current) return; // 已有更新的请求，丢弃过期响应
        setLines(parseLines(r.content));
        setTruncated(r.truncated);
        autoErrOnce.current = false;
      } catch (e) {
        if (seq !== contentSeq.current) return;
        if (!silent) onToast("err", `读取 ${category}.log 失败：${e}`);
        else if (!autoErrOnce.current) {
          autoErrOnce.current = true;
          onToast("err", `自动刷新失败：${e}`);
        }
      } finally {
        if (seq === contentSeq.current) setLoading(false);
      }
    },
    [onToast],
  );

  useEffect(() => {
    loadInfo();
  }, [loadInfo]);

  useEffect(() => {
    stickBottom.current = true; // 切分类回到末尾再跟随
    loadContent(cat, false);
  }, [cat, loadContent]);

  // 自动刷新：2 秒轮询当前分类（新内容都在文件尾部）
  useEffect(() => {
    if (!auto) return;
    const t = setInterval(() => {
      if (!loading) void loadContent(cat, true);
    }, 2000);
    return () => clearInterval(t);
  }, [auto, cat, loading, loadContent]);

  // 内容更新后跟随到底部；用户上翻阅历史时不打断（自动刷新每 2 秒一次）
  useEffect(() => {
    const el = preRef.current;
    if (el && stickBottom.current) el.scrollTop = el.scrollHeight;
  }, [lines]);

  const shown = useMemo(() => {
    const kw = keyword.trim().toLowerCase();
    return lines.filter((l) => {
      if (levelFilter !== "all" && l.level && l.level !== levelFilter) return false;
      if (kw && !l.text.toLowerCase().includes(kw)) return false;
      return true;
    });
  }, [lines, levelFilter, keyword]);

  const curFile = info?.categories.find((c) => c.category === cat) ?? null;

  // 确认框里预告将释放的空间
  const clearBytes = useMemo(() => {
    if (!info || !clearTarget) return 0;
    const pick =
      clearTarget === "all"
        ? info.categories
        : info.categories.filter((c) => c.category === clearTarget);
    return pick.reduce((s, c) => s + c.size + (c.rotatedSize ?? 0), 0);
  }, [info, clearTarget]);

  const copyScreen = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(shown.map((l) => l.text).join("\n"));
      onToast("ok", `已复制当前屏幕 ${shown.length} 条日志`);
    } catch {
      onToast("err", "复制失败（剪贴板不可用）");
    }
  }, [shown, onToast]);

  const makeDiagnostics = useCallback(async () => {
    setDiagBusy(true);
    try {
      setDiag(await api.exportDiagnostics());
      loadInfo();
    } catch (e) {
      onToast("err", `生成诊断包失败：${e}`);
    } finally {
      setDiagBusy(false);
    }
  }, [onToast, loadInfo]);

  const copyDiagnostics = useCallback(async () => {
    if (!diag) return;
    try {
      await navigator.clipboard.writeText(diag.content);
      onToast("ok", "已复制诊断包全文，报 bug 时直接粘贴发送");
    } catch {
      onToast("err", "复制失败（剪贴板不可用）");
    }
  }, [diag, onToast]);

  const changeLevel = useCallback(
    async (v: string) => {
      if (levelBusy || v === settings.logLevel) return;
      setLevelBusy(true);
      try {
        await onSaveLogLevel(v);
        loadInfo();
      } finally {
        setLevelBusy(false);
      }
    },
    [levelBusy, settings.logLevel, onSaveLogLevel, loadInfo],
  );

  const runClear = useCallback(async () => {
    if (clearing || !clearTarget) return;
    setClearing(true);
    try {
      const freed = await api.clearSystemLogs(
        clearTarget === "all" ? undefined : [clearTarget],
      );
      onToast(
        "ok",
        `已清除${clearTarget === "all" ? "全部类别" : ` ${clearTarget} 分类`}的日志，释放 ${fmtSize(freed)}`,
      );
      setLines([]);
      setTruncated(false);
      setClearTarget(null);
      void loadContent(clearTarget === "all" ? cat : clearTarget, true);
      loadInfo();
    } catch (e) {
      onToast("err", `清除日志失败：${e}`);
    } finally {
      setClearing(false);
    }
  }, [clearing, clearTarget, cat, loadContent, loadInfo, onToast]);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="text-base font-semibold">系统日志</h2>
        <Badge variant="outline" className="max-w-[320px] truncate font-mono" title={info?.logsDir}>
          {info?.logsDir ?? "…"}
        </Badge>
        {info && (
          <Badge variant={info.logLevelPinned ? "warning" : "info"} className="uppercase">
            {info.logLevel}
            {info.logLevelPinned ? "（环境变量锁定）" : "（生效中）"}
          </Badge>
        )}
        <span className="flex-1" />
        <Button size="sm" variant="outline" onClick={() => onReveal(info?.logsDir ?? "")} disabled={!info}>
          <FolderOpen /> 打开日志目录
        </Button>
        <Button size="sm" variant="outline" disabled={diagBusy} onClick={() => void makeDiagnostics()}>
          <Activity /> {diagBusy ? "生成中…" : "生成诊断包"}
        </Button>
      </div>

      {info?.logLevelPinned && (
        <Alert>
          <AlertDescription>
            环境变量 <span className="font-mono">DSH_STARTER_LOG</span> 已把级别锁定为{" "}
            <span className="font-mono uppercase">{info.logLevel}</span>
            ：应用内的级别设置会照常保存，但运行期以环境变量为准。清掉该环境变量后重启，即可完全交给应用内设置。
          </AlertDescription>
        </Alert>
      )}

      {/* 分类切换：一个文件一类问题（清单来自后端 CATEGORIES 表） */}
      <ToggleGroup
        type="single"
        variant="outline"
        size="sm"
        className="flex-wrap"
        value={cat}
        onValueChange={(v) => v && setCat(v)}
      >
        {(info?.categories ?? []).map((c) => (
          <ToggleGroupItem key={c.category} value={c.category} title={c.description}>
            {c.category}
            {c.size > 0 && (
              <span className="ml-1 text-[10px] text-muted-foreground">{fmtSize(c.size)}</span>
            )}
          </ToggleGroupItem>
        ))}
      </ToggleGroup>

      {/* 工具条：级别/关键词过滤 + 自动刷新 + 日志级别设置 */}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <ToggleGroup
          type="single"
          variant="outline"
          size="sm"
          value={levelFilter}
          onValueChange={(v) => v && setLevelFilter(v as (typeof LEVEL_FILTERS)[number])}
        >
          {LEVEL_FILTERS.map((l) => (
            <ToggleGroupItem key={l} value={l}>
              {l === "all" ? "全部级别" : l}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <Input
          className="h-8 w-[180px]"
          placeholder="按关键词过滤…"
          value={keyword}
          onChange={(e) => setKeyword(e.target.value)}
        />
        <div className="flex items-center gap-2">
          <Switch id="logs-auto" checked={auto} onCheckedChange={setAuto} />
          <Label htmlFor="logs-auto" className="text-[12px] font-normal">自动刷新（2s）</Label>
        </div>
        <div className="flex items-center gap-2">
          <Label htmlFor="logs-level" className="text-[12px] font-normal text-muted-foreground">
            日志级别
          </Label>
          <Select value={settings.logLevel} onValueChange={(v) => void changeLevel(v)} disabled={levelBusy}>
            <SelectTrigger id="logs-level" className="h-8 w-[130px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {LEVEL_OPTIONS.map((l) => (
                <SelectItem key={l} value={l} className="uppercase">{l}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Button
          size="sm"
          variant="outline"
          disabled={loading}
          title="重新读取当前分类"
          onClick={() => {
            void loadContent(cat, false);
            loadInfo();
          }}
        >
          <RefreshCw className={loading ? "animate-spin" : ""} /> 刷新
        </Button>
        <Button size="sm" variant="outline" disabled={shown.length === 0} onClick={() => void copyScreen()}>
          <Copy /> 复制本屏
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="text-destructive hover:text-destructive"
          disabled={!curFile || (curFile.size === 0 && (curFile.rotatedSize ?? 0) === 0)}
          title={`删除 ${cat}.log 与滚动出的 .1（下次写入自动重建）`}
          onClick={() => setClearTarget(cat)}
        >
          <Trash2 /> 清除此分类
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="text-destructive hover:text-destructive"
          disabled={!info?.categories.some((c) => c.size > 0 || (c.rotatedSize ?? 0) > 0)}
          title="删除全部类别的日志文件"
          onClick={() => setClearTarget("all")}
        >
          <Trash2 /> 全部清除
        </Button>
      </div>

      {curFile && (
        <div className="text-[11px] leading-relaxed text-muted-foreground">
          {curFile.category}.log —— {curFile.description}
          {curFile.rotatedSize != null && `（已滚动一代：另含 .1 共 ${fmtSize(curFile.rotatedSize)}）`}
          {curFile.modifiedMs != null && ` · 最后写入 ${new Date(curFile.modifiedMs).toLocaleString()}`}
          {" · "}共 {lines.length} 条，当前显示 {shown.length} 条
        </div>
      )}

      <Card className="min-h-0 gap-0 overflow-hidden p-0">
        {truncated && (
          <div className="border-b border-border bg-muted/40 px-3 py-1 text-[11px] text-muted-foreground">
            文件超出读取上限，仅显示末尾部分 —— 「生成诊断包」会带更多上下文
          </div>
        )}
        <pre
          ref={preRef}
          className="h-[52vh] overflow-auto p-3 font-mono text-[11.5px] leading-relaxed"
          onScroll={(e) => {
            const el = e.currentTarget;
            stickBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
          }}
        >
          {shown.length === 0 ? (
            <span className="text-muted-foreground">
              {loading ? "读取中…" : "没有匹配的日志（换个分类、级别或关键词试试）"}
            </span>
          ) : (
            <>
              {shown.length > RENDER_CAP && (
                <div className="mb-1 text-[11px] text-muted-foreground">
                  结果较多，仅渲染最近 {RENDER_CAP} 条（用关键词或级别过滤可进一步缩小范围）
                </div>
              )}
              {shown.map((l, i) =>
                i < shown.length - RENDER_CAP ? null : (
                  <div
                    key={i}
                    className={
                      l.level === "ERROR"
                        ? "break-words text-destructive whitespace-pre-wrap"
                        : l.level === "WARN"
                          ? "break-words text-amber-600 dark:text-amber-400 whitespace-pre-wrap"
                          : l.level === "DEBUG"
                            ? "break-words text-muted-foreground whitespace-pre-wrap"
                            : "break-words whitespace-pre-wrap"
                    }
                  >
                    {l.text}
                  </div>
                ),
              )}
            </>
          )}
        </pre>
      </Card>

      {/* 诊断包弹窗：生成后在应用内查看全文，顶部一键复制，不必去文件夹里找文件 */}
      <Dialog open={diag !== null} onOpenChange={(o) => !o && setDiag(null)}>
        <DialogContent className="flex max-h-[85vh] w-full max-w-3xl flex-col">
          <DialogHeader className="shrink-0 pr-8">
            <DialogTitle className="truncate font-mono text-sm">
              {diag?.path.split(/[\\/]/).pop()}
            </DialogTitle>
            <DialogDescription className="truncate font-mono">
              已保存到 {diag?.path}
            </DialogDescription>
          </DialogHeader>
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <Button size="sm" variant="outline" onClick={() => void copyDiagnostics()}>
              <Copy /> 复制全文
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => diag && onReveal(diag.path)}
            >
              <FolderOpen /> 打开所在目录
            </Button>
            <span className="text-[11px] text-muted-foreground">
              报 bug 时发这一份就够（凭据已脱敏）
            </span>
          </div>
          <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-words rounded-md border border-border bg-muted/30 p-3 font-mono text-[11.5px] leading-relaxed">
            {diag?.content}
          </pre>
        </DialogContent>
      </Dialog>

      {/* 清理是删文件的不可逆动作：AlertDialog 二次确认（AGENTS：确认类交互用 AlertDialog） */}
      <AlertDialog
        open={clearTarget !== null}
        onOpenChange={(o) => {
          if (!o && !clearing) setClearTarget(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {clearTarget === "all" ? "清除全部系统日志？" : `清除「${clearTarget ?? ""}」分类日志？`}
            </AlertDialogTitle>
            <AlertDialogDescription>
              将删除{clearTarget === "all" ? ` ${info?.categories.length ?? 0} 个分类` : ` ${clearTarget}.log`}的当前文件与滚动出的
              .1（约 {fmtSize(clearBytes)}）。日志系统会在下次写入时自动重建文件，不影响运行中的
              dsh 实例；此操作无法撤销。建议先「生成诊断包」再清除，排查记录不会丢。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={clearing}>取消</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={clearing}
              onClick={(e) => {
                e.preventDefault();
                void runClear();
              }}
            >
              {clearing ? "清除中…" : "清除"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
