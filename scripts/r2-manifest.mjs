#!/usr/bin/env node
// 把 tauri-action 生成的 latest.json 改写成「自建源（Cloudflare R2）」版本。
//
// 两个要点：
// 1. tauri-action 的清单里用的是 GitHub **资产 API 地址**
//    （`https://api.github.com/repos/…/releases/assets/<id>`），搬到 R2 后必须改写成
//    R2 的文件地址，否则客户端会绕回 GitHub 下载 —— 那就白搬了；
// 2. 地址用**固定键**（`<base>/latest/<平台>.扩展名`），路径不带版本号：每次发布覆盖
//    同名对象，桶里永远只有一份「当前最新」，旧版自然消失，也不需要任何清理逻辑；
// 3. 但固定路径的内容会变，**长缓存会把旧包喂给客户端**（与清单里的签名对不上、更新
//    直接失败）。所以清单里再补一个与版本绑定的查询串 `?v=<版本>` 当缓存指纹：
//    路径对人始终是「latest」，对 CDN 却是「每个版本一个新对象」—— 对象因此可以安全地
//    用 immutable 长缓存，下载才快。
//
// 签名（signature）字段原样保留：安装包字节没变，客户端照常验签。
//
// 用法：
//   node scripts/r2-manifest.mjs --in latest.json --base https://pub-xxx.r2.dev \
//        --assets-map assets.json [--out latest.r2.json] [--check-dir 目录] \
//        [--list-files upload.tsv]
//
//   --in          tauri-action 生成的 latest.json
//   --base        自建源公开基址（末尾不要带 /）
//   --assets-map  资产映射：`gh release view vX --json assets` 的输出，或 {apiUrl: 文件名}
//                 清单里是资产 API 地址时**必须**给，否则解析不出文件名
//                 （会直接报错，绝不猜：猜错等于把用户指向 404）
//   --out         输出路径（默认打印到 stdout）
//   --check-dir   可选：确认引用的安装包都在这个目录里
//   --list-files  可选：写出「本地文件名 <TAB> R2 对象键」的 TSV，供 CI 逐行上传
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
    else if (a === "--stable-keys") {
      /* 兼容旧调用（现在恒为稳定键模式） */
    }
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

/**
 * 平台键 → R2 上的**固定对象键**。
 *
 * 用固定键（而不是带版本号的文件名）是为了「桶里只保留一份 latest」：
 * 客户端清单里的地址永远长这样，版本一变就覆盖同名对象。
 * 同名不同包（如 darwin 三个架构共用 universal 包）指向同一个键，避免重复上传。
 */
const STABLE_KEYS = {
  "windows-x86_64": "windows-x64.msi",
  "windows-x86_64-msi": "windows-x64.msi",
  "windows-x86_64-nsis": "windows-x64-setup.exe",
  "linux-x86_64": "linux-x86_64.AppImage",
  "linux-x86_64-appimage": "linux-x86_64.AppImage",
  "linux-x86_64-deb": "linux-x86_64.deb",
  "linux-x86_64-rpm": "linux-x86_64.rpm",
  "darwin-aarch64": "darwin-universal.app.tar.gz",
  "darwin-aarch64-app": "darwin-universal.app.tar.gz",
  "darwin-x86_64": "darwin-universal.app.tar.gz",
  "darwin-x86_64-app": "darwin-universal.app.tar.gz",
  "darwin-universal": "darwin-universal.app.tar.gz",
  "darwin-universal-app": "darwin-universal.app.tar.gz",
};

/** 压缩/安装包的复合扩展名（.app.tar.gz 要整体保留） */
const COMPOUND_EXT = /(\.app\.tar\.gz|\.tar\.gz|\.tar\.xz|\.AppImage|\.[A-Za-z0-9]{2,6})$/;

/**
 * 取平台键对应的固定对象键。
 * 表里没有的新平台（以后加了 target）退化为「平台键 + 原扩展名」——
 * 依然不带版本号，依然是稳定地址。
 */
function stableKey(target, originalName) {
  if (STABLE_KEYS[target]) return STABLE_KEYS[target];
  const ext = (originalName.match(COMPOUND_EXT) || [".bin"])[0];
  return `${target}${ext}`;
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
const resolved = new Map(); // R2 对象键 → { local, targets[] }
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
  const key = stableKey(e.target, name);
  // ?v= 是缓存指纹：路径稳定、字节可 immutable，换版本就换 URL
  const fingerprint = encodeURIComponent(String(manifest.version ?? "dev"));
  e.info.url = `${base}/latest/${encodeURIComponent(key)}?v=${fingerprint}`;
  if (!resolved.has(key)) resolved.set(key, { local: name, targets: [] });
  resolved.get(key).targets.push(e.target);
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
  const tsv = [...resolved.entries()]
    .map(([key, v]) => `${v.local}\t${key}`)
    .join("\n");
  writeFileSync(args.listFiles, tsv + "\n");
  console.log(`✔ 已写出待上传清单 ${args.listFiles}（${resolved.size} 个对象）`);
}
console.log(`   基址: ${base}（固定键，每次发布覆盖）`);
console.log(`   上传映射（本地文件 → R2 键）：`);
for (const [key, v] of resolved) {
  console.log(`     ${v.local}  →  latest/${key}   （${v.targets.length} 个平台键）`);
}
if (problems.length) {
  console.error("\n✖ 校验不通过：");
  for (const p of problems) console.error(`   - ${p}`);
  process.exit(2);
}
