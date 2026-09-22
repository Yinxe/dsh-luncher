import { useEffect, useState } from "react";
import { Loader2, CopyPlus } from "lucide-react";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api } from "../api";

interface Props {
  /** 要复制的源 profile 名；null = 关闭 */
  source: string | null;
  /** 打开时预填的新实例名（版本风险框的「复制试用」入口用）；空 = 留空手输 */
  initialName?: string;
  /** 现有 profile 名列表（重名即时校验） */
  existing: string[];
  onClose: () => void;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
  onCopied: (name: string) => void;
}

/** 复制 profile 实例：手动输入新实例名，后端整目录拷贝（跳过 node_modules / cache） */
export default function CopyProfileDialog({ source, initialName = "", existing, onClose, onToast, onCopied }: Props) {
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);

  // 每次打开都重置输入：默认空，调用方可预填建议名
  useEffect(() => {
    if (source != null) setName(initialName);
  }, [source, initialName]);

  const trimmed = name.trim();
  const duplicated = trimmed !== "" && existing.includes(trimmed);
  const canSubmit = trimmed !== "" && !duplicated && !busy;

  const confirm = async () => {
    if (!source || !canSubmit) return;
    setBusy(true);
    try {
      const port = await api.copyProfile(source, trimmed);
      onToast(
        "ok",
        port != null
          ? `已复制为「${trimmed}」，web 端口已自动错开为 ${port}（node_modules / cache 不复制，首次启动自动重装依赖）`
          : `已复制为「${trimmed}」（node_modules / cache 不复制，首次启动自动重装依赖）`
      );
      onCopied(trimmed);
      onClose();
    } catch (e) {
      onToast("err", String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={source != null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <CopyPlus className="h-4 w-4" /> 复制 profile「{source}」
          </DialogTitle>
          <DialogDescription>
            把该 profile 的配置目录整份拷贝为新实例。实例名需手动输入；
            node_modules / cache 等可重建产物不复制，首次启动自动重装依赖。
            若源实例配置了 web 端口，副本会自动换一个邻近的空闲端口，可直接并行运行。
          </DialogDescription>
        </DialogHeader>
        <Input
          autoFocus
          className="font-mono"
          placeholder="新实例名，例如 web-test"
          value={name}
          disabled={busy}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && confirm()}
        />
        {duplicated && (
          <p className="text-[11.5px] text-destructive">实例「{trimmed}」已存在，请换一个名字</p>
        )}
        <DialogFooter>
          <Button variant="outline" disabled={busy} onClick={onClose}>取消</Button>
          <Button disabled={!canSubmit} onClick={confirm}>
            {busy && <Loader2 className="animate-spin" />} 复制
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
