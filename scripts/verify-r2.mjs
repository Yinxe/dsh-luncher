#!/usr/bin/env node
// R2 上传后的公网自检（取代原 workflow 里那段内嵌 node -e）：
// 1. 公开地址的 latest.json 必须是**本版本**、所有平台地址都指向固定键；
// 2. upload.tsv 里每个对象都要能从公网 HEAD 到 —— 固定键还要核对
//    Content-Disposition 保留了原始文件名（人类直链下载时别拿到 windows-x64-setup.exe 这种无名包）。
// 任何一项不过就退出码 1：CI 红，而不是把坏源留给客户端。
//
// 用法：node scripts/verify-r2.mjs --dir r2-upload --version 0.3.1
// env: R2_PUBLIC_BASE

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fatal, parseArgs } from "./lib/gh.mjs";

const args = parseArgs(process.argv.slice(2), "用法: node scripts/verify-r2.mjs --dir <目录> --version <x.y.z>");
for (const k of ["dir", "version"]) if (!args[k]) fatal(`缺少 --${k}`);
args.version = String(args.version).replace(/^v/, ""); // --version v0.3.1 与 0.3.1 都接受
const base = (process.env.R2_PUBLIC_BASE || "").replace(/\/+$/, "");
if (!/^https:\/\//.test(base)) fatal(`需要环境变量 R2_PUBLIC_BASE（https 基址），当前: "${base}"`);

const encKey = (key) => key.split("/").map(encodeURIComponent).join("/");
const urlOf = (key) =>
  key.startsWith("latest/")
    ? `${base}/${encKey(key)}?probe=${Date.now()}` // 绕边缘缓存，探的是回源结果
    : `${base}/${encKey(key)}`;

const problems = [];
const manifest = await fetch(`${base}/latest.json?probe=${Date.now()}`, { cache: "no-store" })
  .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`latest.json → HTTP ${r.status}`))))
  .catch((e) => { problems.push(e.message); return null; });

if (manifest) {
  if (String(manifest.version) !== args.version) {
    problems.push(`清单版本是 ${manifest.version}，不是 ${args.version}（没传上去？还是缓存？）`);
  }
  const platforms = Object.entries(manifest.platforms ?? {});
  if (!platforms.length) problems.push("清单里没有平台键");
  for (const [k, v] of platforms) {
    if (typeof v?.url !== "string" || !v.url.startsWith(`${base}/latest/`)) {
      problems.push(`${k} 的地址不是自建源固定键: ${v?.url}`);
    }
  }
  console.log(`✔ 清单 version=${manifest.version}，${platforms.length} 个平台键都指向 ${base}`);
}

// upload.tsv: 本地文件名 <TAB> 对象键（完整路径） <TAB> Content-Disposition（"-" 表示无）
const rows = readFileSync(join(args.dir, "upload.tsv"), "utf8").trim().split("\n").map((l) => l.split("\t"));
let okCount = 0;
for (const [local, key, cd] of rows) {
  const url = urlOf(key);
  const res = await fetch(url, { method: "HEAD", cache: "no-store" });
  if (!res.ok) { problems.push(`${key} → HTTP ${res.status}`); continue; }
  if (cd !== "-") {
    const head = res.headers.get("content-disposition") ?? "";
    if (!head.includes(`filename="${local}"`)) {
      problems.push(`${key} 的 Content-Disposition 里没有原始文件名 ${local}（实际: ${head || "缺失"}）`);
      continue;
    }
  }
  okCount++;
}

if (problems.length) {
  console.error(`✖ 自检失败（${problems.length} 项）：`);
  for (const p of problems) console.error(`   - ${p}`);
  process.exit(1);
}
const fixed = rows.filter(([, k]) => k.startsWith("latest/")).length;
const mirror = rows.length - fixed;
console.log(`✔ ${okCount}/${rows.length} 个对象公网可取（固定键 ${fixed}、版本镜像 ${mirror}），文件名与清单地址全部核对通过`);
