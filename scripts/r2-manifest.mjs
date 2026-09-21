#!/usr/bin/env node
// 把 tauri-action 生成的 latest.json 改写成「自建源（Cloudflare R2）」版本。
//
// 为什么需要：Tauri 更新器清单里的下载地址是绝对 URL，而 tauri-action 用的是
// GitHub 的**资产 API 地址**（`https://api.github.com/repos/…/releases/assets/<id>`）。
// 对象搬到 R2 之后，地址必须改成 R2 的公开文件地址，否则客户端会绕回 GitHub 下载
// —— 那就白搬了。签名（signature）字段原样保留：安装包字节没变，客户端照常验签。
//
// 用法：
//   node scripts/r2-manifest.mjs --in latest.json --base https://pub-xxx.r2.dev \
//        --assets-map assets.json [--out latest.r2.json] [--check-dir 目录]
//
//   --in          tauri-action 生成的 latest.json
//   --base        自建源公开基址（末尾不要带 /）
//   --assets-map  资产映射，二选一：
//                   · `gh release view vX --json assets` 的输出（数组，含 apiUrl/name）
//                   · 或自己写的 {"<apiUrl>": "<文件名>"}
//                 清单里是资产 API 地址时**必须**给，否则解析不出文件名（会直接报错，
//                 绝不猜：猜错等于把用户指向 404）
//   --out         输出路径（默认打印到 stdout）
//   --check-dir   可选：确认每个平台引用的安装包确实在这个目录里（防止清单指向没上传的文件）
//
// 退出码：0 成功；1 参数/结构有问题；2 校验不通过。

import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { basename, join } from "node:path";

const USAGE =
  "用法: node scripts/r2-manifest.mjs --in <latest.json> --base <https://自建源基址> " +
  "[--assets-map <assets.json>] [--out <路径>] [--check-dir <目录>]";

function parseArgs(argv) {
  const out = { in: "", base: "", out: "", checkDir: "", assetsMap: "", listFiles: "" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--in") out.in = argv[++i] ?? "";
    else if (a === "--base") out.base = argv[++i] ?? "";
    else if (a === "--out") out.out = argv[++i] ?? "";
    else if (a === "--check-dir") out.checkDir = argv[++i] ?? "";
    else if (a === "--assets-map") out.assetsMap = argv[++i] ?? "";
    else if (a === "--list-files") out.listFiles = argv[++i] ?? "";
    else if (a === "--help" || a === "-h") {
      console.log(USAGE);
      process.exit(0);
    } else {
      console.error(`未知参数: ${a}\n${USAGE}`);
      process.exit(1);
    }
  }
  if (!out.in || !out.base) {
    console.error(USAGE);
    process.exit(1);
  }
  return out;
}

/** 读资产映射：兼容 gh 的数组形态与手写的对象形态 */
function loadAssetsMap(path) {
  if (!path) return new Map();
  const raw = JSON.parse(readFileSync(path, "utf8"));
  const map = new Map();
  // 三种形态都认：gh 的裸数组、`gh … --json assets` 的 {assets:[…]}、手写的 {url:name}
  const list = Array.isArray(raw) ? raw : Array.isArray(raw?.assets) ? raw.assets : null;
  if (list) {
    for (const a of list) {
      if (a && a.apiUrl && a.name) map.set(String(a.apiUrl), String(a.name));
    }
  } else if (raw && typeof raw === "object") {
    for (const [k, v] of Object.entries(raw)) {
      if (typeof v === "string") map.set(String(k), v);
    }
  }
  if (map.size === 0) {
    console.error(`--assets-map ${path} 里没解析出任何映射（需要数组或 {apiUrl: 文件名}）`);
    process.exit(1);
  }
  return map;
}

/** 这个 basename 像不像真实文件名（而不是资产 id / 数字） */
function looksLikeFileName(name) {
  return Boolean(name) && /\.[A-Za-z0-9]{2,8}$/.test(name) && !/^\d+$/.test(name);
}

const args = parseArgs(process.argv.slice(2));
const base = args.base.replace(/\/+$/, "");
if (!/^https:\/\//.test(base)) {
  console.error(`--base 必须是 https 地址：${base}`);
  process.exit(1);
}

let manifest;
try {
  manifest = JSON.parse(readFileSync(args.in, "utf8"));
} catch (e) {
  console.error(`读不了 ${args.in}：${e.message}`);
  process.exit(1);
}
const assets = loadAssetsMap(args.assetsMap);

// Tauri 清单结构：{ version, notes, pub_date, platforms: { "<target>": { signature, url } } }
// 也兼容单平台形态 { version, url, signature }
const entries = [];
if (manifest.platforms && typeof manifest.platforms === "object") {
  for (const [target, info] of Object.entries(manifest.platforms)) {
    entries.push({ target, info, where: `platforms.${target}` });
  }
} else if (manifest.url) {
  entries.push({ target: manifest.target ?? "(单平台)", info: manifest, where: "(顶层)" });
}
if (entries.length === 0) {
  console.error("清单里没有 platforms / url —— 这不像 Tauri 更新器格式的 latest.json");
  process.exit(1);
}

const problems = [];
const resolved = new Map(); // 文件名 → 被哪些平台引用
for (const e of entries) {
  if (!e.info || typeof e.info.url !== "string") {
    problems.push(`${e.where}: 缺少 url`);
    continue;
  }
  if (typeof e.info.signature !== "string" || !e.info.signature.trim()) {
    problems.push(`${e.where}: 缺少 signature（客户端无法验签）`);
    continue;
  }
  const rawUrl = e.info.url;
  let name = "";
  const fromMap = assets.get(rawUrl);
  if (fromMap) {
    name = fromMap;
  } else {
    let tail = rawUrl;
    try {
      tail = basename(new URL(rawUrl).pathname);
    } catch {
      tail = basename(rawUrl);
    }
    if (looksLikeFileName(tail)) name = tail;
  }
  if (!name) {
    problems.push(
      `${e.where}: 解析不出文件名（url=${rawUrl}）—— 这是 GitHub 资产 API 地址，` +
        `请用 --assets-map 传入映射`
    );
    continue;
  }
  e.info.url = `${base}/${encodeURIComponent(name)}`;
  if (!resolved.has(name)) resolved.set(name, []);
  resolved.get(name).push(e.target);
  if (args.checkDir && !existsSync(join(args.checkDir, name))) {
    problems.push(`${e.where}: ${args.checkDir} 里没有 ${name}`);
  }
}

const text = JSON.stringify(manifest, null, 2) + "\n";
if (args.out) {
  writeFileSync(args.out, text);
  console.log(`✔ 已写出 ${args.out}（version=${manifest.version ?? "?"}）`);
} else {
  process.stdout.write(text);
}

if (args.listFiles) {
  writeFileSync(args.listFiles, [...resolved.keys()].join("\n") + "\n");
  console.log(`✔ 已写出待上传清单 ${args.listFiles}（${resolved.size} 个安装包）`);
}
console.log(`   基址: ${base}`);
console.log(`   需要上传的对象（去掉 latest.json 就是安装包清单）：`);
for (const [name, targets] of resolved) {
  console.log(`     ${name}   ← ${targets.length} 个平台键`);
}
if (problems.length) {
  console.error("\n✖ 校验不通过：");
  for (const p of problems) console.error(`   - ${p}`);
  process.exit(2);
}
