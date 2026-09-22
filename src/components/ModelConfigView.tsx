import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Bot, Check, ChevronDown, KeyRound, Loader2, Plus, RefreshCw, RotateCcw, Save, Star, Trash2, TriangleAlert,
} from "lucide-react";
import {
  Alert, AlertDescription, AlertTitle,
} from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import {
  Command, CommandEmpty, CommandGroup, CommandItem, CommandList,
} from "@/components/ui/command";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import {
  InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput,
} from "@/components/ui/input-group";
import { Label } from "@/components/ui/label";
import {
  Popover, PopoverAnchor, PopoverContent,
} from "@/components/ui/popover";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";
import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { cn } from "@/lib/utils";
import { api } from "../api";
import type { ModelConfigInfo, ModelConfigInput, ModelEntryInput, RemoteModelInfo } from "../types";

interface Props {
  onToast: (kind: "ok" | "err" | "info", text: string) => void;
}

const API_OPTIONS = [
  { value: "openai-completions", label: "openai-completions" },
  { value: "openai-responses", label: "openai-responses" },
  { value: "anthropic-messages", label: "anthropic-messages" },
];

const UNSET = "__unset__";
const NONE = "__none__";

type KV = { key: string; value: string };

interface ModelDraft {
  id: string;
  name: string;
  contextWindow: string;
  maxTokens: string;
  reasoningEfforts: KV[];
  /** 输入模态（如 text/image）：本页不编辑，保存时原样透传 */
  input: string[] | null;
  extra: Record<string, unknown> | null;
}

interface ProviderDraft {
  id: string;
  displayName: string;
  api: string;
  baseURL: string;
  /** 密钥引用名（凭据名或环境变量名），保存到 settings.yaml 的 apiKeyEnv */
  keyName: string;
  /** 密钥值：仅当需要创建/更新凭据时填写，保存时回存到「凭据管理」 */
  keyValue: string;
  models: ModelDraft[];
  /** 透传字段：不在本页编辑，保存时原样保留 */
  extra: Record<string, unknown> | null;
  headers: Record<string, unknown> | null;
  compat: Record<string, unknown> | null;
}

interface DefaultDraft {
  provider: string;
  model: string;
  reasoningEffort: string;
  extra: Record<string, unknown> | null;
}

const objToKV = (obj: Record<string, unknown> | null | undefined): KV[] =>
  obj == null ? [] : Object.entries(obj).map(([key, v]) => ({ key, value: v == null ? "" : String(v) }));

const kvToObj = (rows: KV[]): Record<string, string | null> | null => {
  const out: Record<string, string | null> = {};
  for (const r of rows) {
    const k = r.key.trim();
    if (!k) continue; // 空 key 行（留白的空行）直接丢弃
    out[k] = r.value.trim() === "" ? null : r.value.trim();
  }
  return Object.keys(out).length ? out : null;
};

const freshModel = (): ModelDraft => ({
  id: "", name: "", contextWindow: "", maxTokens: "", reasoningEfforts: [], input: null, extra: null,
});

/** 由 provider id 推导手动密钥的凭据名（如 my-gateway → MY_GATEWAY_API_KEY） */
const deriveKeyName = (id: string): string => {
  const t = id
    .toUpperCase()
    .replace(/[^A-Z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
  return t ? `${t}_API_KEY` : "CUSTOM_API_KEY";
};

interface ComboOption {
  value: string;
  /** 右侧灰色说明（如 1M → 1,000,000） */
  hint?: string;
}

/** 解析 token 数量：支持纯数字、千分位/下划线分隔，以及 K/M 后缀（如 128000、128K、1M、1.5M） */
const parseTokenCount = (raw: string): number | null => {
  const v = raw.trim().replace(/[_,\s]/g, "");
  const m = /^(\d+(?:\.\d+)?)([kKmM]?)$/.exec(v);
  if (!m) return null;
  const unit = m[2].toLowerCase();
  const n = Number(m[1]) * (unit === "m" ? 1_000_000 : unit === "k" ? 1_000 : 1);
  if (!Number.isFinite(n)) return null;
  return Math.round(n);
};

/** 数字 → 紧凑写法（1000000 → 1M，128000 → 128K） */
const fmtCompact = (n: number): string => {
  const trim = (x: number) => String(Math.round(x * 10) / 10);
  if (n >= 1_000_000) return `${trim(n / 1_000_000)}M`;
  if (n >= 1_000) return `${trim(n / 1_000)}K`;
  return String(n);
};

/** 输入内容的紧凑提示：仅当写法与输入不同才返回（128000 → 128K；128K → null） */
const fmtShort = (v: string): string | null => {
  const n = parseTokenCount(v);
  if (n == null) return null;
  const c = fmtCompact(n);
  const normalized = v.trim().replace(/[_,\s]/g, "").toLowerCase();
  return c.toLowerCase() === normalized ? null : c;
};

/** 常用上下文窗口 / 最大输出预设 */
const CONTEXT_OPTIONS: ComboOption[] = ["32K", "64K", "128K", "200K", "256K", "384K", "512K", "1M"]
  .map((v) => ({ value: v, hint: parseTokenCount(v)?.toLocaleString("en-US") }));
const MAX_TOKEN_OPTIONS: ComboOption[] = ["4K", "8K", "16K", "32K", "64K", "128K"]
  .map((v) => ({ value: v, hint: parseTokenCount(v)?.toLocaleString("en-US") }));

/** 可输入 + 可选的下拉输入框（Popover + Command 组合）：既能手输任意值，也能从列表选择 */
function EditableCombobox({
  id, value, onChange, options, placeholder, filter = false, mono = true, emptyText,
}: {
  id: string;
  value: string;
  onChange: (v: string) => void;
  options: ComboOption[];
  placeholder: string;
  /** true = 选项随输入内容过滤（凭据名）；false = 始终展示全部预设 */
  filter?: boolean;
  mono?: boolean;
  emptyText?: string;
}) {
  const [open, setOpen] = useState(false);
  const anchorRef = useRef<HTMLDivElement>(null);
  const q = value.trim().toLowerCase();
  const list = filter && q ? options.filter((o) => o.value.toLowerCase().includes(q)) : options;

  /** 点在输入框/下拉按钮上时阻止关闭——否则刚获得焦点弹出的列表会被判为「外部交互」而收起 */
  const keepOpenInside = useCallback(
    (e: { target: EventTarget | null; preventDefault: () => void }) => {
      const t = e.target;
      if (t instanceof Node && anchorRef.current?.contains(t)) e.preventDefault();
    },
    [],
  );

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverAnchor asChild>
        <div ref={anchorRef} className="w-full">
          <InputGroup className="h-8">
            <InputGroupInput
              id={id}
              role="combobox"
              aria-expanded={open}
              aria-autocomplete="list"
              autoComplete="off"
              className={cn("text-xs", mono && "font-mono")}
              placeholder={placeholder}
              value={value}
              onChange={(e) => {
                onChange(e.target.value);
                setOpen(true);
              }}
              onFocus={() => setOpen(true)}
              onKeyDown={(e) => {
                if (e.key === "Escape") setOpen(false);
                else if (e.key === "ArrowDown") setOpen(true);
              }}
            />
            <InputGroupAddon align="inline-end">
              <InputGroupButton
                size="icon-xs"
                aria-label={open ? "收起可选值" : "展开可选值"}
                onClick={() => {
                  const next = !open;
                  setOpen(next);
                  if (next) anchorRef.current?.querySelector("input")?.focus();
                }}
              >
                <ChevronDown className={cn("transition-transform", open && "rotate-180")} />
              </InputGroupButton>
            </InputGroupAddon>
          </InputGroup>
        </div>
      </PopoverAnchor>
      <PopoverContent
        className="w-[var(--radix-popover-anchor-width)] p-0"
        align="start"
        onOpenAutoFocus={(e) => e.preventDefault()}
        onInteractOutside={keepOpenInside}
        onPointerDownOutside={keepOpenInside}
        onFocusOutside={keepOpenInside}
      >
        <Command shouldFilter={false}>
          <CommandList>
            {list.length > 0 ? (
              <CommandGroup>
                {list.map((o) => (
                  <CommandItem
                    key={o.value}
                    value={o.value}
                    onSelect={() => {
                      onChange(o.value);
                      setOpen(false);
                    }}
                    className={cn("text-xs", mono && "font-mono")}
                  >
                    <Check className={cn("h-3.5 w-3.5", o.value === value ? "opacity-100" : "opacity-0")} />
                    <span>{o.value}</span>
                    {o.hint && (
                      <span className="ml-auto text-[10.5px] font-normal text-muted-foreground">{o.hint}</span>
                    )}
                  </CommandItem>
                ))}
              </CommandGroup>
            ) : (
              <CommandEmpty>{emptyText ?? "无匹配选项——可直接输入自定义值"}</CommandEmpty>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

/** 从后端读取结果构造可编辑草稿 */
const toDraft = (cfg: ModelConfigInfo): { provs: ProviderDraft[]; dm: DefaultDraft | null } => ({
  provs: cfg.providers.map((p) => ({
    id: p.id,
    displayName: p.displayName ?? "",
    api: p.api ?? "",
    baseURL: p.baseURL ?? "",
    keyName: p.apiKeyEnv ?? "",
    keyValue: "",
    models: p.models.map((m) => ({
      id: m.id,
      name: m.name ?? "",
      contextWindow: m.contextWindow != null ? String(m.contextWindow) : "",
      maxTokens: m.maxTokens != null ? String(m.maxTokens) : "",
      reasoningEfforts: objToKV(m.reasoningEfforts),
      input: m.input ?? null,
      extra: m.extra ?? null,
    })),
    extra: p.extra ?? null,
    headers: p.headers ?? null,
    compat: p.compat ?? null,
  })),
  dm: cfg.defaultModel
    ? {
        provider: cfg.defaultModel.provider,
        model: cfg.defaultModel.model,
        reasoningEffort: cfg.defaultModel.reasoningEffort ?? "",
        extra: cfg.defaultModel.extra ?? null,
      }
    : null,
});

/** 模型配置视图：参考 dsh 官方 provider 配置布局，结构化编辑 ~/.dsh/settings.yaml 的
 *  llm-pi-ai.providers 与 agent-default-model。保存只重写这两节，文件其余内容与
 *  节外注释由后端逐字节保留；手动输入的密钥回存到「凭据管理」。 */
export default function ModelConfigView({ onToast }: Props) {
  const [loaded, setLoaded] = useState<ModelConfigInfo | null>(null);
  const [provs, setProvs] = useState<ProviderDraft[]>([]);
  const [dm, setDm] = useState<DefaultDraft | null>(null);
  const [credNames, setCredNames] = useState<string[]>([]);
  const [baseline, setBaseline] = useState("");
  const [busy, setBusy] = useState(false);
  /** 展开编辑的 Provider（官方交互：卡片「编辑」按钮进入编辑态） */
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  /** 展开详情的模型行（上下文窗口 / 最大输出 / 思考等级），键为 `${providerId}:${idx}` */
  const [modelDetail, setModelDetail] = useState<Record<string, boolean>>({});
  /** 展开密钥值输入的 Provider（为已有凭据换值时用） */
  const [valueOpen, setValueOpen] = useState<Record<string, boolean>>({});

  const [addOpen, setAddOpen] = useState(false);
  const [newId, setNewId] = useState("");
  const [newApi, setNewApi] = useState(API_OPTIONS[0].value);
  const [newBaseURL, setNewBaseURL] = useState("");
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);

  /** 获取可用模型弹窗：provider id + 拉取结果 + 勾选 */
  const [fetchFor, setFetchFor] = useState<string | null>(null);
  const [fetching, setFetching] = useState(false);
  const [fetched, setFetched] = useState<RemoteModelInfo[]>([]);
  const [fetchErr, setFetchErr] = useState<string | null>(null);
  const [fetchSel, setFetchSel] = useState<string[]>([]);

  const dirty = useMemo(
    () => JSON.stringify({ provs, dm }) !== baseline,
    [provs, dm, baseline],
  );

  const reload = useCallback(async () => {
    try {
      const [cfg, creds] = await Promise.all([
        api.getModelConfig(),
        api.getCredentials().catch(() => null),
      ]);
      setCredNames(creds?.refs.map((r) => r.name) ?? []);
      setLoaded(cfg);
      const d = toDraft(cfg);
      setProvs(d.provs);
      setDm(d.dm);
      setBaseline(JSON.stringify({ provs: d.provs, dm: d.dm }));
    } catch (e) {
      onToast("err", `读取模型配置失败: ${e}`);
    }
  }, [onToast]);

  useEffect(() => {
    reload();
  }, [reload]);

  // ── 校验（与后端 validate 对齐） ────────────────
  const issues = useMemo(() => {
    const list: string[] = [];
    const pids = new Set<string>();
    for (const p of provs) {
      const id = p.id.trim();
      if (!id) list.push("存在空 Provider ID");
      else if (/\s/.test(id)) list.push(`Provider ID「${id}」不能包含空白字符`);
      else if (pids.has(id)) list.push(`Provider ID 重复: ${id}`);
      pids.add(id);
      const b = p.baseURL.trim();
      if (b && !/^https?:\/\//.test(b)) list.push(`Provider「${id}」的 API 地址需以 http:// 或 https:// 开头`);
      const mids = new Set<string>();
      for (const m of p.models) {
        const mid = m.id.trim();
        if (!mid) list.push(`Provider「${id}」存在空模型 ID`);
        else if (/\s/.test(mid)) list.push(`模型 ID「${mid}」不能包含空白字符`);
        else if (mids.has(mid)) list.push(`Provider「${id}」模型 ID 重复: ${mid}`);
        mids.add(mid);
        for (const f of ["contextWindow", "maxTokens"] as const) {
          if (m[f].trim() !== "" && parseTokenCount(m[f]) == null) {
            list.push(`Provider「${id}」模型「${mid}」的 ${f} 需为数字或带 K/M 单位的数量（如 128K、1M）`);
          }
        }
      }
    }
    if (dm) {
      const p = provs.find((x) => x.id.trim() === dm.provider);
      if (!p) list.push(`默认模型指向的 Provider「${dm.provider}」不存在，请重新选择或清除`);
      else if (!p.models.some((m) => m.id.trim() === dm.model)) {
        list.push(`默认模型指向的模型「${dm.model}」在 Provider「${dm.provider}」中不存在`);
      }
    }
    return list;
  }, [provs, dm]);

  const totalModels = useMemo(() => provs.reduce((n, p) => n + p.models.length, 0), [provs]);
  const defaultDangling =
    dm != null &&
    !provs.some((p) => p.id.trim() === dm.provider && p.models.some((m) => m.id.trim() === dm.model));

  // ── Provider / 模型草稿编辑 ─────────────────
  const patchProvider = useCallback((idx: number, patch: Partial<ProviderDraft>) => {
    setProvs((ps) => ps.map((p, i) => (i === idx ? { ...p, ...patch } : p)));
  }, []);

  const patchModel = useCallback((pIdx: number, mIdx: number, patch: Partial<ModelDraft>) => {
    setProvs((ps) =>
      ps.map((p, i) =>
        i === pIdx
          ? { ...p, models: p.models.map((m, j) => (j === mIdx ? { ...m, ...patch } : m)) }
          : p,
      ),
    );
  }, []);

  const addModel = useCallback((pIdx: number) => {
    setProvs((ps) =>
      ps.map((p, i) => (i === pIdx ? { ...p, models: [...p.models, freshModel()] } : p)),
    );
  }, []);

  const deleteModel = useCallback((pIdx: number, mIdx: number) => {
    setProvs((ps) =>
      ps.map((p, i) => (i === pIdx ? { ...p, models: p.models.filter((_, j) => j !== mIdx) } : p)),
    );
    // 不能在 setDm 的 updater 里调用 onToast：StrictMode 下 updater 会跑两次 → 重复 toast，
    // 且属于渲染阶段的副作用。直接用当前 provs/dm 判断，命中才清默认并提示。
    const p = provs[pIdx];
    const removed = p?.models[mIdx];
    if (removed && dm && dm.provider === p.id && dm.model === removed.id.trim()) {
      setDm(null);
      onToast("info", "默认模型已被删除，默认模型一并清除");
    }
  }, [provs, dm, onToast]);

  const addProvider = useCallback(() => {
    const id = newId.trim();
    if (!id || /\s/.test(id) || provs.some((p) => p.id === id)) return;
    setProvs((ps) => [
      ...ps,
      {
        id, displayName: "", api: newApi, baseURL: newBaseURL.trim(),
        keyName: "", keyValue: "",
        models: [], extra: null, headers: null, compat: null,
      },
    ]);
    setExpanded((m) => ({ ...m, [id]: true }));
    setAddOpen(false);
    setNewId("");
    setNewBaseURL("");
    onToast("info", `已添加 Provider「${id}」，填写密钥与模型后点「保存」写入文件`);
  }, [newId, newApi, newBaseURL, provs, onToast]);

  const confirmDeleteProvider = useCallback(() => {
    if (!pendingDelete) return;
    const id = pendingDelete;
    setPendingDelete(null);
    setProvs((ps) => ps.filter((p) => p.id !== id));
    // 不在 setDm 的 updater 里调 onToast：StrictMode 下 updater 跑两次会重复弹提示，
    // 且属于渲染阶段副作用。直接用当前 dm 判断，命中才清默认并提示（同 deleteModel）。
    if (dm && dm.provider === id) {
      setDm(null);
      onToast("info", "默认模型指向的 Provider 已删除，默认模型一并清除");
    }
  }, [pendingDelete, dm, onToast]);

  // ── 默认模型联动 ────────────────────────────
  const dmProvider = useMemo(
    () => (dm ? provs.find((p) => p.id.trim() === dm.provider) ?? null : null),
    [dm, provs],
  );
  const dmModel = useMemo(
    () => (dm && dmProvider ? dmProvider.models.find((m) => m.id.trim() === dm.model) ?? null : null),
    [dm, dmProvider],
  );
  const dmEffortKeys = useMemo(
    () => [
      ...new Set((dmModel?.reasoningEfforts ?? []).map((r) => r.key.trim()).filter(Boolean)),
    ],
    [dmModel],
  );

  const setDefaultProvider = useCallback((pid: string) => {
    if (pid === NONE) {
      setDm(null);
      return;
    }
    const first = provs.find((x) => x.id === pid)?.models[0];
    setDm({
      provider: pid,
      model: first?.id.trim() ?? "",
      // 不自动带思考等级：档位语义因模型而异，留给用户显式选择（不选 = 保持默认）
      reasoningEffort: "",
      extra: dm?.extra ?? null,
    });
  }, [provs, dm]);

  const setDefaultModel = useCallback((mid: string) => {
    setDm((cur) => (cur ? { ...cur, model: mid, reasoningEffort: "" } : cur));
  }, []);

  const setDefaultByStar = useCallback((pid: string, mid: string) => {
    setDm({ provider: pid, model: mid, reasoningEffort: "", extra: dm?.extra ?? null });
    onToast("info", `默认模型已选为「${pid} / ${mid}」，点「保存」写入文件`);
  }, [dm, onToast]);

  // ── 获取可用模型 ────────────────────────────
  const openFetch = useCallback(async (p: ProviderDraft) => {
    setFetchFor(p.id);
    setFetching(true);
    setFetched([]);
    setFetchErr(null);
    setFetchSel([]);
    try {
      const list = await api.fetchProviderModels(
        p.baseURL.trim(),
        p.api.trim() || "openai-completions",
        p.keyName.trim(),
        p.keyValue.trim() || undefined,
      );
      setFetched(list);
    } catch (e) {
      setFetchErr(String(e));
    } finally {
      setFetching(false);
    }
  }, []);

  const confirmFetchAdd = useCallback(() => {
    if (!fetchFor) return;
    const chosen = fetched.filter((m) => fetchSel.includes(m.id));
    setProvs((ps) =>
      ps.map((p) => {
        if (p.id !== fetchFor) return p;
        const existing = new Set(p.models.map((m) => m.id.trim()));
        const add = chosen
          .filter((m) => !existing.has(m.id))
          .map((m) => ({ ...freshModel(), id: m.id, name: m.name && m.name !== m.id ? m.name : "" }));
        return { ...p, models: [...p.models, ...add] };
      }),
    );
    setFetchFor(null);
    if (chosen.length > 0) {
      onToast("ok", `已添加 ${chosen.length} 个模型，点「保存」写入文件`);
    }
  }, [fetchFor, fetched, fetchSel, onToast]);

  // ── 保存 ──────────────────────────────────
  const save = useCallback(async () => {
    setBusy(true);
    try {
      // 1) 填了密钥值的引用名 → 创建/更新凭据（整表合并写，其他凭据原样保留）
      const upserts = provs
        .map((p) => ({ name: p.keyName.trim(), value: p.keyValue.trim() }))
        .filter((e) => e.name !== "" && e.value !== "");
      if (upserts.length > 0) {
        const creds = await api.getCredentials();
        // 注释必须原样带上：凭据写的是整表，漏掉 note 等于把用户写的注释全删了
        const refs = creds.refs.map((r) => ({ name: r.name, value: r.value, note: r.note }));
        for (const e of upserts) {
          const hit = refs.find((r) => r.name === e.name);
          if (hit) hit.value = e.value;
          else refs.push({ name: e.name, value: e.value, note: null });
        }
        await api.writeCredentialRefs(refs, creds.fingerprint);
      }
      // 2) 写 settings.yaml 两节（apiKeyEnv = 引用名）
      const input: ModelConfigInput = {
        providers: provs.map((p): ModelConfigInput["providers"][number] => ({
          id: p.id.trim(),
          displayName: p.displayName.trim() || null,
          api: p.api.trim() || null,
          baseURL: p.baseURL.trim() || null,
          apiKeyEnv: p.keyName.trim() || null,
          headers: p.headers,
          compat: p.compat,
          models: p.models.map((m): ModelEntryInput => ({
            id: m.id.trim(),
            name: m.name.trim() || null,
            contextWindow: parseTokenCount(m.contextWindow),
            maxTokens: parseTokenCount(m.maxTokens),
            input: m.input,
            reasoningEfforts: kvToObj(m.reasoningEfforts),
            extra: m.extra,
          })),
          extra: p.extra,
        })),
        defaultModel: dm
          ? {
              provider: dm.provider,
              model: dm.model,
              reasoningEffort: dm.reasoningEffort || null,
              extra: dm.extra,
            }
          : null,
      };
      await api.setModelConfig(input);
      onToast("ok", "模型配置已保存（其余配置与节外注释原样保留）");
      await reload();
    } catch (e) {
      onToast("err", String(e));
    } finally {
      setBusy(false);
    }
  }, [provs, dm, reload, onToast]);

  if (!loaded) {
    return (
      <div className="flex h-40 items-center justify-center gap-2 text-muted-foreground">
        <Loader2 className="h-4 w-4 animate-spin" /> 正在读取模型配置…
      </div>
    );
  }

  const newIdOk = newId.trim() !== "" && !/\s/.test(newId.trim()) && !provs.some((p) => p.id === newId.trim());
  const fetchProvider = fetchFor ? provs.find((p) => p.id === fetchFor) ?? null : null;

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="text-base font-semibold">模型配置</h2>
        <Badge variant="outline" className="font-mono" title={loaded.path}>
          ~/.dsh/settings.yaml
        </Badge>
        <Badge variant="secondary">{provs.length} 个 Provider</Badge>
        <Badge variant="secondary">{totalModels} 个模型</Badge>
        {dirty && <Badge variant="warning">未保存</Badge>}
        <span className="flex-1" />
        <Button size="sm" variant="outline" disabled={busy || !dirty} onClick={reload}>
          <RotateCcw /> 还原
        </Button>
        <Button
          size="sm"
          disabled={busy || !dirty || issues.length > 0 || !!loaded.parseError}
          onClick={save}
        >
          {busy && <Loader2 className="animate-spin" />} <Save /> 保存（自动备份）
        </Button>
      </div>
      <div className="rounded-lg border border-amber-500/25 bg-amber-500/5 p-2.5 text-[11.5px] leading-relaxed text-muted-foreground">
        编辑全局配置的模型两节：<span className="font-mono text-foreground">llm-pi-ai.providers</span> 与
        <span className="font-mono text-foreground"> agent-default-model</span>
        （默认模型），保存只重写这两节，其余内容与节外注释逐字节保留，写前自动备份。
        API 密钥可选择已有凭据或手动输入——手动输入的密钥保存时回存到「凭据管理」；
        上下文窗口 / 最大输出可直接填数字，也可用 128K、1M 这类写法；
        模型的思考等级不填则不写入（保持 dsh 默认）。
      </div>

      {loaded.parseError && (
        <Alert variant="destructive">
          <TriangleAlert />
          <AlertTitle>settings.yaml 解析失败，已锁定保存</AlertTitle>
          <AlertDescription>
            {loaded.parseError}——请到「配置文件」页修复语法后回来重试。
          </AlertDescription>
        </Alert>
      )}

      {!loaded.exists && (
        <Alert>
          <Bot />
          <AlertTitle>settings.yaml 尚不存在</AlertTitle>
          <AlertDescription>
            dsh 首次运行后会自动创建；现在直接配置并保存，启动器会创建该文件（只含模型两节）。
          </AlertDescription>
        </Alert>
      )}

      {issues.length > 0 && (
        <Alert variant="destructive">
          <TriangleAlert />
          <AlertTitle>{issues.length} 个问题需要修正后才能保存</AlertTitle>
          <AlertDescription>
            <ul className="ml-4 list-disc space-y-0.5">
              {issues.map((it, i) => <li key={i}>{it}</li>)}
            </ul>
          </AlertDescription>
        </Alert>
      )}

      {/* 默认模型 */}
      <Card className="p-4">
        <div className="mb-1 flex items-center gap-2">
          <span className="eyebrow">默认模型 · agent-default-model</span>
          {dm && !defaultDangling && (
            <Badge variant="success" className="font-mono">
              {dm.provider} / {dm.model}
              {dm.reasoningEffort ? ` · ${dm.reasoningEffort}` : ""}
            </Badge>
          )}
          {!dm && <span className="text-[11.5px] text-muted-foreground">未设置——Agent 走 dsh 内置默认</span>}
        </div>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Select value={dm?.provider ?? NONE} onValueChange={setDefaultProvider}>
            <SelectTrigger className="w-56 font-mono text-xs">
              <SelectValue placeholder="Provider" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NONE} className="text-muted-foreground">（未设置）</SelectItem>
              {provs.map((p) => (
                <SelectItem key={p.id} value={p.id} className="font-mono text-xs">{p.id}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <span className="text-muted-foreground">/</span>
          <Select
            value={dm ? dm.model || NONE : NONE}
            onValueChange={(v) => v !== NONE && setDefaultModel(v)}
            disabled={!dmProvider}
          >
            <SelectTrigger className="w-64 font-mono text-xs">
              <SelectValue placeholder="模型" />
            </SelectTrigger>
            <SelectContent>
              {dm && dm.model === "" && (
                <SelectItem value={NONE} className="text-muted-foreground">（先添加模型）</SelectItem>
              )}
              {dmProvider?.models.map((m) => (
                <SelectItem key={m.id} value={m.id.trim()} className="font-mono text-xs">
                  {m.id.trim()}
                  {m.name ? ` · ${m.name}` : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <span className="text-muted-foreground">/</span>
          <Select
            value={dm?.reasoningEffort || NONE}
            onValueChange={(v) => setDm((cur) => (cur ? { ...cur, reasoningEffort: v === NONE ? "" : v } : cur))}
            disabled={!dmModel || dmEffortKeys.length === 0}
          >
            <SelectTrigger className="w-40 font-mono text-xs">
              <SelectValue placeholder="思考等级" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NONE} className="text-muted-foreground">（保持默认）</SelectItem>
              {dmEffortKeys.map((k) => (
                <SelectItem key={k} value={k} className="font-mono text-xs">
                  {k}
                  {dmModel?.reasoningEfforts?.find((r) => r.key.trim() === k)?.value
                    ? ` → ${dmModel.reasoningEfforts.find((r) => r.key.trim() === k)?.value}`
                    : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {dm && (
            <Button variant="ghost" size="icon" title="清除默认模型" onClick={() => setDm(null)}>
              <Trash2 className="text-destructive" />
            </Button>
          )}
        </div>
        {dm && dmProvider && dmProvider.models.length === 0 && (
          <p className="mt-2 text-[11.5px] text-amber-500">
            Provider「{dmProvider.id}」还没有模型——先在下方卡片添加模型，再回来选择。
          </p>
        )}
        {dmModel && dmEffortKeys.length === 0 && (
          <p className="mt-2 text-[11.5px] text-muted-foreground">
            模型「{dmModel.id}」未定义思考等级，等级选择不可用（保持默认）。
          </p>
        )}
      </Card>

      {/* Providers 列表：每张卡片一个 Provider（官方布局） */}
      {provs.map((p, pIdx) => {
        const isOpen = !!expanded[p.id];
        const isDefaultProvider = dm?.provider === p.id;
        const keyName = p.keyName.trim();
        const hasKey = keyName !== "";
        const keyDesc = !keyName ? "未设置"
          : credNames.includes(keyName) ? `凭据 ${keyName}`
          : p.keyValue.trim() ? `凭据 ${keyName}（待保存）`
          : `环境变量 ${keyName}`;
        return (
          <Card key={p.id} className="gap-0 py-0">
            <div className="flex flex-row items-center gap-2.5 px-4 py-3">
              <span
                className={`h-2 w-2 shrink-0 rounded-full ${hasKey ? "bg-emerald-500" : "bg-muted-foreground/30"}`}
                title={hasKey ? "已配置密钥" : "未配置密钥"}
              />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-1.5">
                  <span className="text-[13.5px] font-semibold">{p.displayName || p.id}</span>
                  {p.displayName && <Badge variant="outline" className="font-mono">{p.id}</Badge>}
                  {p.api && <Badge variant="info" className="font-mono">{p.api}</Badge>}
                  {isDefaultProvider && (
                    <Badge variant="success">
                      <Star className="h-3 w-3 fill-current" /> 默认
                    </Badge>
                  )}
                </div>
                <div className="truncate font-mono text-[10.5px] text-muted-foreground">
                  {p.baseURL || "（未设置 API 地址）"}
                  {` · ${p.models.length} 个模型 · 密钥：${keyDesc}`}
                </div>
              </div>
              <Button
                size="sm"
                variant={isOpen ? "secondary" : "outline"}
                onClick={() => setExpanded((m) => ({ ...m, [p.id]: !m[p.id] }))}
              >
                {isOpen ? "收起" : "编辑"}
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="text-destructive hover:text-destructive"
                title={`删除 Provider「${p.id}」`}
                onClick={() => setPendingDelete(p.id)}
              >
                <Trash2 />
              </Button>
            </div>

            {isOpen && (
              <div className="space-y-4 border-t border-border px-4 pb-4 pt-3">
                {/* API 密钥：可输入 + 下拉选择已有凭据（Combobox） */}
                <div className="space-y-1.5">
                  <Label className="flex items-center gap-1.5 text-xs">
                    <KeyRound className="h-3 w-3" /> API 密钥
                  </Label>
                  <EditableCombobox
                    id={`key-${p.id}`}
                    value={p.keyName}
                    onChange={(v) => patchProvider(pIdx, { keyName: v })}
                    options={credNames.map((n) => ({ value: n }))}
                    placeholder={`选择已有凭据或输入名称（如 ${deriveKeyName(p.id)}）`}
                    filter
                    emptyText="无匹配凭据——直接按输入的名称创建新凭据"
                  />
                  {keyName === "" ? (
                    <p className="text-[11px] text-muted-foreground">
                      未设置——从下拉选择已有凭据，或输入新凭据名称；部分本地网关可免密钥
                    </p>
                  ) : credNames.includes(keyName) && !valueOpen[p.id] ? (
                    <p className="text-[11px] text-muted-foreground">
                      将使用凭据「{keyName}」已保存的值（值在「凭据管理」中维护）
                      <Button
                        variant="link"
                        size="sm"
                        className="ml-1 h-auto p-0 text-[11px]"
                        onClick={() => setValueOpen((m) => ({ ...m, [p.id]: true }))}
                      >
                        更换值
                      </Button>
                    </p>
                  ) : (
                    <Input
                      type="password"
                      className="font-mono text-xs"
                      placeholder={
                        credNames.includes(keyName)
                          ? `新密钥值（保存时更新凭据 ${keyName}）`
                          : "密钥值——保存时将创建为新凭据"
                      }
                      value={p.keyValue}
                      onChange={(e) => patchProvider(pIdx, { keyValue: e.target.value })}
                    />
                  )}
                </div>

                {/* 自定义设置 */}
                <Collapsible
                  defaultOpen={!!(p.displayName || p.baseURL || p.api)}
                  className="rounded-lg border border-border/70 px-3 py-2.5"
                >
                  <CollapsibleTrigger className="group flex w-full items-center gap-1.5 text-left">
                    <ChevronDown className="h-3.5 w-3.5 transition-transform group-data-[state=open]:rotate-180" />
                    <span className="eyebrow">自定义设置</span>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                    <div className="grid gap-x-4 gap-y-3 pt-3 sm:grid-cols-2">
                      <div className="space-y-1.5">
                        <Label className="text-xs">显示名称</Label>
                        <Input
                          className="h-8 text-xs"
                          placeholder="可选，如 OpenCode Zen"
                          value={p.displayName}
                          onChange={(e) => patchProvider(pIdx, { displayName: e.target.value })}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label className="text-xs">API 协议</Label>
                        <Select
                          value={p.api && API_OPTIONS.some((o) => o.value === p.api) ? p.api : p.api || UNSET}
                          onValueChange={(v) => patchProvider(pIdx, { api: v === UNSET ? "" : v })}
                        >
                          <SelectTrigger className="h-8 font-mono text-xs">
                            <SelectValue placeholder="（未设置）" />
                          </SelectTrigger>
                          <SelectContent>
                            {p.api && !API_OPTIONS.some((o) => o.value === p.api) && (
                              <SelectItem value={p.api} className="font-mono text-xs">{p.api}</SelectItem>
                            )}
                            {API_OPTIONS.map((o) => (
                              <SelectItem key={o.value} value={o.value} className="font-mono text-xs">
                                {o.label}
                              </SelectItem>
                            ))}
                            <SelectItem value={UNSET} className="text-muted-foreground">（未设置）</SelectItem>
                          </SelectContent>
                        </Select>
                      </div>
                      <div className="space-y-1.5 sm:col-span-2">
                        <Label className="text-xs">API 地址</Label>
                        <Input
                          className="h-8 font-mono text-xs"
                          placeholder="https://api.example.com/v1"
                          value={p.baseURL}
                          onChange={(e) => patchProvider(pIdx, { baseURL: e.target.value })}
                        />
                      </div>
                    </div>
                  </CollapsibleContent>
                </Collapsible>

                {/* 模型列表 */}
                <div className="space-y-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="eyebrow">模型 · {p.models.length}</span>
                    <span className="flex-1" />
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={!p.baseURL.trim() || fetching}
                      title={!p.baseURL.trim() ? "先填写 API 地址" : `从 ${p.baseURL.trim()}/models 拉取可用模型`}
                      onClick={() => openFetch(p)}
                    >
                      {fetching && fetchFor === p.id
                        ? <Loader2 className="animate-spin" />
                        : <RefreshCw />} 获取可用模型
                    </Button>
                    <Button size="sm" variant="outline" onClick={() => addModel(pIdx)}>
                      <Plus /> 添加模型
                    </Button>
                  </div>
                  {p.models.map((m, mIdx) => {
                    const detailKey = `${p.id}:${mIdx}`;
                    const detailOpen = !!modelDetail[detailKey];
                    const isDefault = dm?.provider === p.id && dm?.model === m.id.trim();
                    return (
                      <div key={mIdx} className="space-y-2.5 rounded-lg border p-2.5">
                        <div className="flex items-center gap-2">
                          <Input
                            className="h-8 flex-1 font-mono text-xs"
                            placeholder="模型 ID"
                            value={m.id}
                            onChange={(e) => patchModel(pIdx, mIdx, { id: e.target.value })}
                          />
                          <Input
                            className="h-8 flex-1 text-xs"
                            placeholder="显示名（可选）"
                            value={m.name}
                            onChange={(e) => patchModel(pIdx, mIdx, { name: e.target.value })}
                          />
                          <Button
                            variant="ghost"
                            size="icon"
                            className={`h-8 w-8 shrink-0 ${isDefault ? "text-amber-500" : "text-muted-foreground"}`}
                            title={isDefault ? "当前默认模型" : "设为默认模型"}
                            onClick={() => setDefaultByStar(p.id, m.id.trim())}
                          >
                            <Star className={`h-3.5 w-3.5 ${isDefault ? "fill-current" : ""}`} />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 shrink-0"
                            title={detailOpen ? "收起详情" : "展开详情（上下文窗口 / 最大输出 / 思考等级）"}
                            onClick={() =>
                              setModelDetail((mm) => ({ ...mm, [detailKey]: !mm[detailKey] }))
                            }
                          >
                            <ChevronDown className={`h-4 w-4 transition-transform ${detailOpen ? "rotate-180" : ""}`} />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-8 w-8 shrink-0 text-destructive hover:text-destructive"
                            title="删除该模型"
                            onClick={() => deleteModel(pIdx, mIdx)}
                          >
                            <Trash2 />
                          </Button>
                        </div>
                        {detailOpen && (
                          <div className="grid gap-x-4 gap-y-3 sm:grid-cols-2">
                            <div className="space-y-1.5">
                              <Label className="text-xs">
                                上下文窗口
                                {fmtShort(m.contextWindow) && (
                                  <span className="ml-1.5 font-mono text-[10px] text-muted-foreground">
                                    ≈ {fmtShort(m.contextWindow)}
                                  </span>
                                )}
                              </Label>
                              <EditableCombobox
                                id={`ctx-${p.id}-${mIdx}`}
                                value={m.contextWindow}
                                onChange={(v) => patchModel(pIdx, mIdx, { contextWindow: v })}
                                options={CONTEXT_OPTIONS}
                                placeholder="如 128K 或 1000000"
                              />
                            </div>
                            <div className="space-y-1.5">
                              <Label className="text-xs">
                                最大输出 token
                                {fmtShort(m.maxTokens) && (
                                  <span className="ml-1.5 font-mono text-[10px] text-muted-foreground">
                                    ≈ {fmtShort(m.maxTokens)}
                                  </span>
                                )}
                              </Label>
                              <EditableCombobox
                                id={`max-${p.id}-${mIdx}`}
                                value={m.maxTokens}
                                onChange={(v) => patchModel(pIdx, mIdx, { maxTokens: v })}
                                options={MAX_TOKEN_OPTIONS}
                                placeholder="如 32K 或 32000"
                              />
                            </div>
                            <div className="space-y-1.5 sm:col-span-2">
                              <div className="flex items-center justify-between">
                                <Label className="text-xs">
                                  思考等级
                                  <span className="ml-1.5 text-[10.5px] font-normal text-muted-foreground">
                                    不填则不写入，保持 dsh 默认
                                  </span>
                                </Label>
                                <Button
                                  size="sm"
                                  variant="outline"
                                  onClick={() =>
                                    patchModel(pIdx, mIdx, {
                                      reasoningEfforts: [...m.reasoningEfforts, { key: "", value: "" }],
                                    })
                                  }
                                >
                                  <Plus /> 添加等级
                                </Button>
                              </div>
                              {m.reasoningEfforts.map((r, ri) => (
                                <div key={ri} className="flex items-center gap-1.5">
                                  <Input
                                    className="h-8 flex-1 font-mono text-xs"
                                    placeholder="等级名（如 high）"
                                    value={r.key}
                                    onChange={(e) =>
                                      patchModel(pIdx, mIdx, {
                                        reasoningEfforts: m.reasoningEfforts.map((x, j) =>
                                          (j === ri ? { ...x, key: e.target.value } : x)),
                                      })
                                    }
                                  />
                                  <span className="shrink-0 text-xs text-muted-foreground">→</span>
                                  <Input
                                    className="h-8 flex-1 font-mono text-xs"
                                    placeholder="API 参数值（留空 = null）"
                                    value={r.value}
                                    onChange={(e) =>
                                      patchModel(pIdx, mIdx, {
                                        reasoningEfforts: m.reasoningEfforts.map((x, j) =>
                                          (j === ri ? { ...x, value: e.target.value } : x)),
                                      })
                                    }
                                  />
                                  <Button
                                    variant="ghost"
                                    size="icon"
                                    className="h-8 w-8 shrink-0 text-destructive hover:text-destructive"
                                    title="删除该等级"
                                    onClick={() =>
                                      patchModel(pIdx, mIdx, {
                                        reasoningEfforts: m.reasoningEfforts.filter((_, j) => j !== ri),
                                      })
                                    }
                                  >
                                    <Trash2 />
                                  </Button>
                                </div>
                              ))}
                            </div>
                          </div>
                        )}
                      </div>
                    );
                  })}
                  {p.models.length > 0 && (
                    <Button
                      size="sm"
                      variant="outline"
                      className="w-full border-dashed text-muted-foreground hover:text-foreground"
                      onClick={() => addModel(pIdx)}
                    >
                      <Plus /> 添加模型
                    </Button>
                  )}
                  {p.models.length === 0 && (
                    <div className="rounded-lg border border-dashed px-4 py-6 text-center text-xs text-muted-foreground">
                      暂无模型——「获取可用模型」从服务方拉取，或「添加模型」手动新建
                    </div>
                  )}
                </div>
              </div>
            )}
          </Card>
        );
      })}
      {provs.length === 0 && (
        <Card className="p-10 text-center text-sm text-muted-foreground">
          暂无 Provider——点下方按钮新建，或在「配置文件」页按
          <span className="font-mono"> llm-pi-ai.providers </span>结构手写后回到这里继续编辑
          <div className="mt-3">
            <Button size="sm" variant="outline" disabled={!!loaded.parseError} onClick={() => setAddOpen(true)}>
              <Plus /> 添加 Provider
            </Button>
          </div>
        </Card>
      )}
      {provs.length > 0 && (
        <div>
          <Button size="sm" variant="outline" disabled={!!loaded.parseError} onClick={() => setAddOpen(true)}>
            <Plus /> 添加 Provider
          </Button>
        </div>
      )}

      {/* 添加 Provider */}
      <Dialog open={addOpen} onOpenChange={(o) => !o && setAddOpen(false)}>
        <DialogContent className="grid-cols-[minmax(0,1fr)] sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Bot className="h-4 w-4" /> 添加 Provider
            </DialogTitle>
            <DialogDescription>
              ID 是 settings.yaml 中的配置键（如 my-gateway），创建后不可改名；其余字段在卡片内编辑。
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1.5">
              <Label htmlFor="prov-id">ID</Label>
              <Input
                id="prov-id"
                autoFocus
                className="font-mono"
                placeholder="例如 my-gateway"
                value={newId}
                onChange={(e) => setNewId(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && newIdOk && addProvider()}
              />
              {newId.trim() !== "" && !newIdOk && (
                <p className="text-[11.5px] text-destructive">
                  ID 不能为空、包含空白，或与现有 Provider 重复
                </p>
              )}
            </div>
            <div className="space-y-1.5">
              <Label>API 协议</Label>
              <Select value={newApi} onValueChange={setNewApi}>
                <SelectTrigger className="font-mono text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {API_OPTIONS.map((o) => (
                    <SelectItem key={o.value} value={o.value} className="font-mono text-xs">{o.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="prov-url">API 地址（可留空稍后填）</Label>
              <Input
                id="prov-url"
                className="font-mono"
                placeholder="https://api.example.com/v1"
                value={newBaseURL}
                onChange={(e) => setNewBaseURL(e.target.value)}
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setAddOpen(false)}>取消</Button>
            <Button disabled={!newIdOk} onClick={addProvider}>添加</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 获取可用模型 */}
      <Dialog
        open={fetchFor != null}
        onOpenChange={(o) => !o && setFetchFor(null)}
      >
        <DialogContent className="grid-cols-[minmax(0,1fr)] sm:max-w-lg">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <RefreshCw className="h-4 w-4" /> 获取可用模型
              {fetchProvider && (
                <span className="truncate font-mono text-xs text-muted-foreground">@ {fetchProvider.id}</span>
              )}
            </DialogTitle>
            <DialogDescription>
              从 <span className="font-mono">{fetchProvider?.baseURL.trim()}/models</span> 拉取，
              勾选要添加的模型（已有的会被跳过）。
            </DialogDescription>
          </DialogHeader>
          {fetching ? (
            <div className="flex h-32 items-center justify-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="h-4 w-4 animate-spin" /> 正在请求服务方…
            </div>
          ) : fetchErr ? (
            <Alert variant="destructive">
              <TriangleAlert />
              <AlertTitle>获取失败</AlertTitle>
              <AlertDescription className="break-all">{fetchErr}</AlertDescription>
            </Alert>
          ) : fetched.length === 0 ? (
            <div className="py-8 text-center text-sm text-muted-foreground">服务方未返回任何模型</div>
          ) : (
            <ToggleGroup
              type="multiple"
              orientation="vertical"
              spacing={0}
              variant="outline"
              className="max-h-80 w-full flex-col overflow-y-auto"
              value={fetchSel}
              onValueChange={setFetchSel}
            >
              {fetched.map((m) => {
                const exists = fetchProvider?.models.some((x) => x.id.trim() === m.id) ?? false;
                return (
                  <ToggleGroupItem
                    key={m.id}
                    value={m.id}
                    disabled={exists}
                    className="w-full justify-start gap-2 px-3 py-2 text-left"
                  >
                    {fetchSel.includes(m.id) && <Check className="h-3.5 w-3.5 shrink-0 text-emerald-500" />}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-mono text-xs">{m.id}</span>
                      {m.name && (
                        <span className="block truncate text-[11px] text-muted-foreground">{m.name}</span>
                      )}
                    </span>
                    {exists && <Badge variant="secondary" className="shrink-0">已存在</Badge>}
                  </ToggleGroupItem>
                );
              })}
            </ToggleGroup>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setFetchFor(null)}>取消</Button>
            {!fetching && !fetchErr && fetched.length > 0 && (
              <Button disabled={fetchSel.length === 0} onClick={confirmFetchAdd}>
                添加所选（{fetchSel.length}）
              </Button>
            )}
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 删除 Provider 确认 */}
      <AlertDialog open={pendingDelete != null} onOpenChange={(o) => !o && setPendingDelete(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除 Provider「{pendingDelete}」？</AlertDialogTitle>
            <AlertDialogDescription>
              将从列表移除该 Provider 及其全部模型，点「保存」后写入文件（原文件自动备份，仅保留一份）。
              保存前可用「还原」放弃全部修改。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={confirmDeleteProvider}>
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
