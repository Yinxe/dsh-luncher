// GitHub Releases REST 极简客户端：发布流水线（release-gate / publish-release / sync-r2 / verify-r2）共用。
//
// 为什么不用 gh CLI（0.3.1 发版实测踩的坑）：
// `releases/tags/<tag>` 与 releases 列表里内嵌的 assets 数组，在「草稿转正式」后
// 会长时间（实测 1h+）停留在**滞后的读副本**上 —— 明明 14 个资产的 Release，
// 这两个接口报 0 个；`gh release download` / `gh release view` 恰好都走滞后接口，
// 于是 publish-r2 两次死在「no assets to download」。
// 而 `/releases/<id>` 与其专用子资源 `/releases/<id>/assets` 始终一致。
// 所以本库的铁律：**凡读资产，必先拿 release id，再走 /releases/<id>/assets**。
//
// 鉴权：CI 里用作业注入的 GH_TOKEN / GITHUB_TOKEN；本地跑用 `gh auth token` 兜底，
// 这样这些脚本同时也是手工补传/排查工具（见 docs/RELEASING.md 第 7 节）。

import { execFileSync } from "node:child_process";
import { appendFileSync, createWriteStream, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

// Node ≥24 起 fetch 会按 NODE_USE_ENV_PROXY 参考 *_{http,https}_proxy 环境变量，
// 本机走代理时本地调试才连得上 api.github.com（CI 无代理变量，不受影响）。
process.env.NODE_USE_ENV_PROXY ??= "1";

export const API = "https://api.github.com";

/** 仓库标识（owner/repo），CI 里 GITHUB_REPOSITORY 天然存在 */
export function repo() {
  const r = process.env.GITHUB_REPOSITORY || process.env.GH_REPO;
  if (!r || !r.includes("/")) {
    throw new Error("缺少 GITHUB_REPOSITORY（本地调试可 export GH_REPO=owner/repo）");
  }
  return r;
}

export function token() {
  const t = process.env.GH_TOKEN || process.env.GITHUB_TOKEN;
  if (t) return t;
  try {
    return execFileSync("gh", ["auth", "token"], { encoding: "utf8" }).trim();
  } catch {
    throw new Error("拿不到 GitHub token：CI 里注入 GH_TOKEN，本地先 `gh auth login`");
  }
}

/**
 * 发一次 REST 请求。
 * opts.method / opts.body（对象自动 JSON 化）/ opts.accept（下载资产用 octet-stream）
 * opts.raw → 返回 Response 本身；opts.allow404 → 不存在时返回 null 而不是抛错。
 */
export async function api(path, opts = {}) {
  const res = await fetch(API + path, {
    method: opts.method ?? "GET",
    headers: {
      Authorization: `Bearer ${token()}`,
      Accept: opts.accept ?? "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
      "User-Agent": "dsh-starter-release-scripts",
      ...(opts.body ? { "Content-Type": "application/json" } : {}),
    },
    body: opts.body ? JSON.stringify(opts.body) : undefined,
  });
  if (res.status === 404 && opts.allow404) return null;
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(`GitHub API ${opts.method ?? "GET"} ${path} → HTTP ${res.status} ${text.slice(0, 400)}`);
  }
  if (opts.raw) return res;
  return res.json();
}

/** 数组型接口翻完所有分页（发布流水线的数据量最多两三页） */
export async function listAll(pathBase) {
  const out = [];
  for (let page = 1; ; page++) {
    const sep = pathBase.includes("?") ? "&" : "?";
    const batch = await api(`${pathBase}${sep}per_page=100&page=${page}`);
    out.push(...batch);
    if (batch.length < 100) return out;
  }
}

/** 按 tag 取 release —— 只信它的身份字段（id/tag_name/draft），**内嵌 assets 可能是滞后的** */
export async function releaseByTag(tag) {
  return api(`/repos/${repo()}/releases/tags/${encodeURIComponent(tag)}`, { allow404: true });
}

export async function releaseById(id) {
  return api(`/repos/${repo()}/releases/${id}`, { allow404: true });
}

/** 资产唯一可信来源：/releases/<id>/assets 专用端点 */
export async function assetsOf(releaseId) {
  return listAll(`/repos/${repo()}/releases/${releaseId}/assets`);
}

/** 草稿（含 tauri-action 的 untagged-<sha> 占位 tag）列表 */
export async function drafts() {
  return (await listAll(`/repos/${repo()}/releases`)).filter((r) => r.draft);
}

/**
 * 下载一个资产到 dest，并核对字节数（API 报告的 size 与落盘大小不一致就报错 ——
 * 半截包传进 R2 比不传恶劣得多）。
 */
export async function downloadAsset(asset, dest) {
  const res = await api(new URL(asset.url).pathname, {
    accept: "application/octet-stream",
    raw: true,
  });
  await pipeline(Readable.fromWeb(res.body), createWriteStream(dest));
  const got = statSync(dest).size;
  if (asset.size != null && got !== asset.size) {
    throw new Error(`下载 ${asset.name} 字节数不符：期望 ${asset.size}，实得 ${got}`);
  }
  return got;
}

/** 把 key=value 追加到 $GITHUB_OUTPUT（本地没有该环境变量时静默跳过） */
export function setOutputs(obj) {
  const file = process.env.GITHUB_OUTPUT;
  if (file) appendFileSync(file, Object.entries(obj).map(([k, v]) => `${k}=${v}`).join("\n") + "\n");
}

/** 往步骤摘要里写几行 markdown */
export function summary(lines) {
  const file = process.env.GITHUB_STEP_SUMMARY;
  if (file) appendFileSync(file, lines.join("\n") + "\n");
}

/** 统一错误出口：打中文错误并退出码 1 */
export function fatal(e) {
  console.error(`✖ ${e instanceof Error ? e.message : e}`);
  process.exit(1);
}

/** 仓库根（scripts/ 的上一级），版本文件按它解析，cwd 在哪都行 */
const ROOT = new URL("../..", import.meta.url).pathname;

/** 读取 src-tauri/tauri.conf.json 的版本号（全流水线唯一版本事实源） */
export function appVersion() {
  const p = join(ROOT, "src-tauri", "tauri.conf.json");
  let conf;
  try {
    conf = JSON.parse(readFileSync(p, "utf8"));
  } catch (e) {
    throw new Error(`读不了 ${p}：${e.message}`);
  }
  return conf.version;
}

/** 极简参数解析：--key value 收进 out.key；后面不跟值的（或到了行尾）记为 true */
export function parseArgs(argv, spec) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith("--")) throw new Error(`未知参数: ${a}\n${spec}`);
    const val = argv[i + 1];
    if (val === undefined || val.startsWith("--")) out[a.slice(2)] = true;
    else {
      out[a.slice(2)] = val;
      i++;
    }
  }
  return out;
}
