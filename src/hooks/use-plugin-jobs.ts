import { useCallback, useEffect, useRef, useState } from "react";
import { api, events } from "../api";
import type { PluginJob, PluginJobEvent, PluginLogEvent } from "../types";

/** 日志逐行流入，攒 80ms 再合并进状态：一次 pnpm 安装可能几百上千行，
 *  逐行 setState 会让 React 反复重建整棵任务列表。 */
const FLUSH_MS = 80;

export interface PluginJobsApi {
  jobs: PluginJob[];
  activeId: number | null;
  setActiveId: (id: number | null) => void;
  /** 运行中的任务数 */
  runningCount: number;
  cancel: (id: number) => void;
  clear: () => void;
}

interface Options {
  /** 某个任务结束时回调（用于刷新 profile 详情 / 提示） */
  onFinished?: (e: PluginJobEvent) => void;
}

/** 把后端快照和本地实时状态按 id 合并。
 *  快照是在 `await` 之前抓的，本地可能在等待期间已经收到过 plugin-job 终态事件——
 *  绝不能用更早的快照把已结束的任务退回「运行中」（否则转圈 / busy 锁死、onFinished 不再触发）。
 *  规则：本地已到终态则保留本地的状态字段；日志取较多的一方；本地有而快照没有的任务保留。 */
function mergeJobs(prev: PluginJob[], list: PluginJob[]): PluginJob[] {
  const prevById = new Map(prev.map((j) => [j.id, j]));
  const merged: PluginJob[] = list.map((j) => {
    const local = prevById.get(j.id);
    if (!local) return j;
    const keepStatus = !local.running; // 终态单调：不再回到运行中
    return {
      ...j,
      ...(keepStatus
        ? {
            running: local.running,
            ok: local.ok,
            exitCode: local.exitCode,
            cancelled: local.cancelled,
            finishedAt: local.finishedAt,
            hint: local.hint,
            pendingBuilds: local.pendingBuilds,
            label: local.label,
          }
        : {}),
      lines: local.lines.length > j.lines.length ? local.lines : j.lines,
      dropped: Math.max(local.dropped, j.dropped),
    };
  });
  const inList = new Set(list.map((j) => j.id));
  const extras = prev.filter((j) => !inList.has(j.id)); // 刚发出快照请求就收到的新任务
  return [...extras, ...merged];
}

/**
 * 插件任务流：挂载时拉一次后端快照（重挂载/切换视图后历史不丢），
 * 之后靠 plugin-log / plugin-job 事件增量更新。
 */
export function usePluginJobs({ onFinished }: Options = {}): PluginJobsApi {
  const [jobs, setJobs] = useState<PluginJob[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const pending = useRef<PluginLogEvent[]>([]);
  const timer = useRef<number | null>(null);
  const refreshSeq = useRef(0);
  const finishedRef = useRef(onFinished);
  finishedRef.current = onFinished;

  const flush = useCallback(() => {
    timer.current = null;
    const batch = pending.current;
    if (batch.length === 0) return;
    pending.current = [];
    setJobs((prev) => {
      const next = prev.map((j) => ({ ...j }));
      const byId = new Map(next.map((j) => [j.id, j]));
      for (const e of batch) {
        const job = byId.get(e.jobId);
        if (!job) continue;
        job.lines = [...job.lines, { stream: e.stream, text: e.line, at: Date.now() }];
        if (job.lines.length > 4000) job.lines = job.lines.slice(-4000);
      }
      return next;
    });
  }, []);

  const schedule = useCallback(() => {
    if (timer.current != null) return;
    timer.current = window.setTimeout(flush, FLUSH_MS);
  }, [flush]);

  const refresh = useCallback(async () => {
    const my = ++refreshSeq.current;
    let list: PluginJob[];
    try {
      list = await api.listPluginJobs();
    } catch {
      return; /* 后端尚未就绪时静默 */
    }
    if (refreshSeq.current !== my) return; // 有更新的刷新在途，丢弃这次过期快照
    setJobs((prev) => mergeJobs(prev, list));
    setActiveId((cur) => {
      if (cur != null) return cur; // 已有聚焦就不动（合并不会凭空删掉当前任务）
      const first = list[0];
      return first ? first.id : null;
    });
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    let alive = true;

    events
      .onPluginLog((e) => {
        pending.current.push(e);
        if (pending.current.length > 400) flush();
        else schedule();
      })
      .then((u) => (alive ? unlisteners.push(u) : u()));

    events
      .onPluginJob((e) => {
        flush();
        setJobs((prev) => {
          const idx = prev.findIndex((j) => j.id === e.jobId);
          if (idx < 0) {
            // 任务开始事件早于快照刷新：补一条占位，后续 refresh 会补全
            const fresh: PluginJob = {
              id: e.jobId,
              profile: e.profile,
              kind: e.kind,
              label: e.label,
              command: "",
              startedAt: e.startedAt,
              finishedAt: e.finishedAt,
              running: e.running,
              ok: e.ok,
              exitCode: e.exitCode,
              cancelled: e.cancelled,
              hint: e.hint,
              argv: [],
              pendingBuilds: e.pendingBuilds,
              lines: [],
              dropped: 0,
            };
            return [fresh, ...prev];
          }
          const next = prev.map((j) => ({ ...j }));
          const job = next[idx];
          job.running = e.running;
          job.ok = e.ok;
          job.exitCode = e.exitCode;
          job.cancelled = e.cancelled;
          job.hint = e.hint;
          job.pendingBuilds = e.pendingBuilds;
          job.finishedAt = e.finishedAt;
          job.label = e.label;
          return next;
        });
        // 新任务自动聚焦
        if (e.running) setActiveId(e.jobId);
        else finishedRef.current?.(e);
      })
      .then((u) => (alive ? unlisteners.push(u) : u()));

    return () => {
      alive = false;
      if (timer.current != null) window.clearTimeout(timer.current);
      timer.current = null;
      unlisteners.forEach((u) => u());
    };
  }, [flush, schedule]);

  const cancel = useCallback(
    (id: number) => {
      // 后端返回 false = 任务已不存在 / 已结束（此时不会有 plugin-job 事件来纠正本地状态），
      // 拉一次快照对齐，避免转圈与 busy 锁死；异常同样用快照兜底。
      api
        .cancelPluginJob(id)
        .then((ok) => {
          if (!ok) void refresh();
        })
        .catch(() => void refresh());
    },
    [refresh],
  );

  const clear = useCallback(() => {
    api
      .clearPluginJobs()
      .then(() => setJobs((prev) => prev.filter((j) => j.running)))
      .catch(() => void refresh());
  }, [refresh]);

  return {
    jobs,
    activeId,
    setActiveId,
    runningCount: jobs.filter((j) => j.running).length,
    cancel,
    clear,
  };
}
