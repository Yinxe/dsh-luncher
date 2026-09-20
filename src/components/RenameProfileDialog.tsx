import { useEffect, useState } from "react";
import { Loader2, Pencil } from "lucide-react";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api } from "../api";

interface Props {
  /** 要改名的 profile 名；null = 关闭 */
  name: string | null;
  /** 现有 profile 名列表（重名即时校验） */
  existing: string[];
  onClose: () => void;
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
  onRenamed: (newName: string) => void;
}

/** 重命名 profile：只改目录名，配置内容原样保留（dsh 内置保留 profile 不会走到这里） */
export default function RenameProfileDialog({ name, existing, onClose, onToast, onRenamed }: Props) {
  const [next, setNext] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (name != null) setNext("");
  }, [name]);

  const trimmed = next.trim();
  const duplicated = trimmed !== "" && existing.includes(trimmed);
  const invalid = trimmed !== "" && (trimmed.includes("/") || trimmed.includes("\\") || trimmed.includes("..") || trimmed.startsWith("."));
  const canSubmit = trimmed !== "" && trimmed !== name && !duplicated && !invalid && !busy;

  const confirm = async () => {
    if (!name || !canSubmit) return;
    setBusy(true);
    try {
      const created = await api.renameProfile(name, trimmed);
      onToast("ok", `profile「${name}」已重命名为「${created}」`);
      onRenamed(created);
      onClose();
    } catch (e) {
      onToast("err", String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={name != null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Pencil className="h-4 w-4" /> 重命名 profile「{name}」
          </DialogTitle>
          <DialogDescription>
            只改实例目录名，配置内容（cordis.patch.yml / package.json 等）原样保留。
            如果「默认 profile」指向它，会一并跟随改名。实例正在运行时不允许改名。
          </DialogDescription>
        </DialogHeader>
        <Input
          autoFocus
          className="font-mono"
          placeholder="新名字，例如 web-test2"
          value={next}
          disabled={busy}
          onChange={(e) => setNext(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && confirm()}
        />
        {duplicated && (
          <p className="text-[11.5px] text-destructive">实例「{trimmed}」已存在，请换一个名字</p>
        )}
        {invalid && <p className="text-[11.5px] text-destructive">名字不能含 / \ .. 且不能以 . 开头</p>}
        <DialogFooter>
          <Button variant="outline" disabled={busy} onClick={onClose}>取消</Button>
          <Button disabled={!canSubmit} onClick={confirm}>
            {busy && <Loader2 className="animate-spin" />} 重命名
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
