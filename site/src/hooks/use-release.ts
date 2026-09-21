import { useCallback, useEffect, useMemo, useState } from "react";

import {
  ASSETS,
  EMPTY_RELEASE,
  fetchRelease,
  resolveDownload,
  sizeOf,
  type AssetDef,
  type ReleaseState,
  type SourceId,
} from "@/lib/release";

const STORAGE_KEY = "dsh-site:source";

/** 源优先级：URL 上的 ?source= → 上次的选择 → 默认自建源（客户端默认也是它） */
function initialSource(): SourceId {
  if (typeof window === "undefined") return "r2";
  const fromUrl = new URLSearchParams(window.location.search).get("source");
  if (fromUrl === "github" || fromUrl === "r2") return fromUrl;
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === "github" || saved === "r2") return saved;
  } catch {
    /* 隐私模式下 localStorage 会抛，忽略 */
  }
  return "r2";
}

/** 一个包 + 它在「当前源」下的最终地址 */
export interface DownloadRow extends AssetDef {
  url: string | null;
  unavailable?: string;
  bytes: number | null;
}

export interface ReleaseController {
  state: ReleaseState;
  source: SourceId;
  setSource: (s: SourceId) => void;
  reload: () => void;
  loading: boolean;
  rows: DownloadRow[];
}

export function useRelease(): ReleaseController {
  const [state, setState] = useState<ReleaseState>(EMPTY_RELEASE);
  const [source, setSourceState] = useState<SourceId>(initialSource);
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    let alive = true;
    setState((s) => ({ ...s, settled: false }));
    fetchRelease()
      .then((next) => {
        if (alive) setState(next);
      })
      .catch(() => {
        if (alive) setState({ ...EMPTY_RELEASE, settled: true });
      });
    return () => {
      alive = false;
    };
  }, [nonce]);

  const setSource = useCallback((next: SourceId) => {
    setSourceState(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      /* 存不下就算了，不影响本次使用 */
    }
    const url = new URL(window.location.href);
    if (next === "r2") url.searchParams.delete("source");
    else url.searchParams.set("source", next);
    window.history.replaceState(null, "", url.toString());
  }, []);

  const rows = useMemo<DownloadRow[]>(() => {
    if (!state.settled) return [];
    return ASSETS.map((asset) => {
      const link = resolveDownload(asset, source, state.version);
      return {
        ...asset,
        url: link.url,
        unavailable: link.unavailable,
        bytes: sizeOf(state, asset, state.version),
      };
    });
  }, [state, source]);

  return {
    state,
    source,
    setSource,
    reload: () => setNonce((n) => n + 1),
    loading: !state.settled,
    rows,
  };
}
