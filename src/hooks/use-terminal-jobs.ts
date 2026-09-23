import { useCallback, useEffect, useRef, useState } from "react";
import { events } from "../api";
import type {
  InstallFinishedEvent,
  RuntimeFinishedEvent,
  SystemTask,
} from "../types";

/** 日志逐行流入，攒 80ms 再合并进状态（同 use-plugin-jobs：逐行 setState 会反复重建列表） */
const FLUSH_MS = 80;
/** 系统任务日志缓冲上限（原来 install 400 / runtime 20，统一取 400） */
const MAX_LINES = 400;

export interface TerminalJobsApi {
  tasks: SystemTask[];
  /** 在途的 dsh 版本安装任务（null = 没有） */
  installTask: SystemTask | null;
  /** 在途的 Node 安装任务（null = 没有；页面进度条只认运行中的） */
  nodeTask: SystemTask | null;
  /** 开始/恢复一个 dsh 版本安装任务（id 用版本号；事件早于调用时会自动补建） */
  startInstall: (version: string) => void;
  /** 开始一个 Node 安装任务 */
  startNode: () => void;
  /** invoke 直接失败且不会有 finished 事件时，把在途任务落为失败终态 */
  fail: (kind: SystemTask["kind"], id: string, message: string) => void;
  /** 清掉全部终态任务 */
  clearFinished: () => void;
}

interface Options {
  /** dsh 版本安装结束（toast / 设为当前版本 / 刷新列表等副作用留在 App） */
  onInstallFinished?: (e: InstallFinishedEvent) => void;
  /** Node 安装结束 */
  onRuntimeFinished?: (e: RuntimeFinishedEvent) => void;
}

function newTask(kind: SystemTask["kind"], id: string, label: string): SystemTask {
  return {
    kind, id, label,
    running: true, ok: null, message: null,
    startedAt: Date.now(), finishedAt: null,
    received: 0, total: 0,
    lines: [], dropped: 0,
  };
}

/**
 * 系统任务流（dsh 版本安装 / Node 安装）：订阅 install-log/install-finished/
 * runtime-log/runtime-progress/runtime-finished，汇成带终态记录的 SystemTask[]。
 * 供通用终端面板展示；页面内进度读 installTask/nodeTask。
 */
export function useTerminalJobs({ onInstallFinished, onRuntimeFinished }: Options = {}): TerminalJobsApi {
  const [tasks, setTasks] = useState<SystemTask[]>([]);
  const pending = useRef<Map<string, string[]>>(new Map());
  const timer = useRef<number | null>(null);
  const cbRef = useRef({ onInstallFinished, onRuntimeFinished });
  cbRef.current = { onInstallFinished, onRuntimeFinished };

  const applyPending = useCallback((prev: SystemTask[]): SystemTask[] => {
    if (pending.current.size === 0) return prev;
    const batch = pending.current;
    pending.current = new Map();
    return prev.map((t) => {
      const lines = batch.get(t.id);
      if (!lines || lines.length === 0) return t;
      const merged = [...t.lines, ...lines];
      const dropped = t.dropped + Math.max(0, merged.length - MAX_LINES);
      return { ...t, lines: merged.slice(-MAX_LINES), dropped };
    });
  }, []);

  const flush = useCallback(() => {
    timer.current = null;
    setTasks(applyPending);
  }, [applyPending]);

  const schedule = useCallback(() => {
    if (timer.current != null) return;
    timer.current = window.setTimeout(flush, FLUSH_MS);
  }, [flush]);

  const pushLine = useCallback((id: string, line: string) => {
    const arr = pending.current.get(id);
    if (arr) arr.push(line);
    else pending.current.set(id, [line]);
    // 单个任务攒太多行时提前落盘，避免内存里堆超长批
    if ((pending.current.get(id)?.length ?? 0) > MAX_LINES) flush();
    else schedule();
  }, [flush, schedule]);

  /** 事件可能早于 start*（后端自动触发/重挂载竞态）：找不到在途任务就补建一条 */
  const ensureRunning = useCallback(
    (list: SystemTask[], kind: SystemTask["kind"], id: string, label: string): SystemTask[] => {
      const t = list.find((x) => x.id === id);
      if (!t) return [newTask(kind, id, label), ...list];
      if (!t.running) return list; // 已有终态记录：迟到的日志行不再复活任务
      return list;
    },
    [],
  );

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let alive = true;
    const track = (p?: Promise<() => void>) =>
      p?.then((u) => { if (alive) unlisteners.push(u); else u(); }).catch(() => undefined);

    track(events.onInstallLog?.((e) => {
      const id = e.version;
      setTasks((prev) => applyPending(ensureRunning(prev, "dshInstall", id, `安装 dsh ${id}`)));
      pushLine(id, e.line);
    }));
    track(events.onInstallFinished?.((e) => {
      setTasks((prev) =>
        applyPending(prev).map((t) =>
          t.kind === "dshInstall" && t.id === e.version && t.running
            ? { ...t, running: false, ok: e.success, message: e.message, finishedAt: Date.now() }
            : t
        )
      );
      cbRef.current.onInstallFinished?.(e);
    }));
    track(events.onRuntimeLog?.((line) => {
      setTasks((prev) => applyPending(ensureRunning(prev, "nodeInstall", "node", "安装内置 Node")));
      pushLine("node", line);
    }));
    track(events.onRuntimeProgress?.((e) => {
      setTasks((prev) =>
        prev.map((t) => (t.kind === "nodeInstall" && t.running ? { ...t, received: e.received, total: e.total } : t))
      );
    }));
    track(events.onRuntimeFinished?.((e) => {
      setTasks((prev) =>
        applyPending(prev).map((t) =>
          t.kind === "nodeInstall" && t.running
            ? { ...t, running: false, ok: e.ok, message: e.message, finishedAt: Date.now() }
            : t
        )
      );
      cbRef.current.onRuntimeFinished?.(e);
    }));

    return () => {
      alive = false;
      if (timer.current != null) window.clearTimeout(timer.current);
      timer.current = null;
      unlisteners.forEach((u) => u());
    };
  }, [applyPending, ensureRunning, flush, pushLine, schedule]);

  const start = useCallback((kind: SystemTask["kind"], id: string, label: string) => {
    setTasks((prev) => {
      const t = prev.find((x) => x.id === id);
      if (t) {
        if (t.running) return prev;
        // 重名重开（同版本重试）：复活为运行中并清空上轮日志
        return prev.map((x) => (x.id === id ? { ...newTask(kind, id, label) } : x));
      }
      return [newTask(kind, id, label), ...prev];
    });
  }, []);

  const startInstall = useCallback((version: string) => start("dshInstall", version, `安装 dsh ${version}`), [start]);
  const startNode = useCallback(() => start("nodeInstall", "node", "安装内置 Node"), [start]);

  const fail = useCallback((kind: SystemTask["kind"], id: string, message: string) => {
    setTasks((prev) =>
      prev.map((t) =>
        t.kind === kind && t.id === id && t.running
          ? { ...t, running: false, ok: false, message, finishedAt: Date.now() }
          : t
      )
    );
  }, []);

  const clearFinished = useCallback(() => setTasks((prev) => prev.filter((t) => t.running)), []);

  return {
    tasks,
    installTask: tasks.find((t) => t.kind === "dshInstall" && t.running) ?? null,
    nodeTask: tasks.find((t) => t.kind === "nodeInstall" && t.running) ?? null,
    startInstall,
    startNode,
    fail,
    clearFinished,
  };
}
