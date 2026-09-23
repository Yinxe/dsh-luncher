#!/usr/bin/env node
// R2 同步第一步：把 Release 的**全部资产**拉下来，并算好完整上传清单。
// 取代原来「gh release download + gh release view --json assets」两步 ——
// 它们都走 releases/tags 滞后接口（0.3.1 连挂两轮的根因）；这里按 releaseId
// 走 /releases/<id>/assets 专用端点，不受影响。
//
// 产物（都在 --dir 里）：
// - 全部安装包 / .sig / dmg（GitHub Release 的 1:1 镜像素材）
// - latest.json —— 已改写为 R2 固定键地址（r2-manifest.mjs 负责，键表仍在那维护）
// - upload.tsv —— `本地文件名 <TAB> 对象键 <TAB> Content-Disposition`，对象键是**完整路径**：
//     latest/<固定键>         更新器与下载页用的稳定地址（带 ?v= 指纹）
//     releases/v<版本>/<原文件名>  GitHub 资产的完整镜像（用户要求：所有资源都上 R2，
//                              断 GitHub 时也能按原始文件名直取）
//   disposition 为 "-" 表示上传时不加该头（镜像键本身就是原始文件名）。
// 上传本体留在 workflow 里做（需要 AWS 凭据），自检见 verify-r2.mjs。
//
// 用法：node scripts/sync-r2.mjs --dir r2-upload --version 0.3.1 \
//          [--release-id 394523678] （缺省按 v<version> 现查）[--skip-download]
//   --skip-download：不下载文件，只按现有 --dir 里的同名文件跑清单/上传表
//   （断点重跑或本地验证逻辑用；--check-dir 会兜底查缺文件）。
// env: GH_TOKEN / GITHUB_REPOSITORY / R2_PUBLIC_BASE

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { api, assetsOf, downloadAsset, fatal, parseArgs, releaseById, releaseByTag, repo } from "./lib/gh.mjs";

const args = parseArgs(process.argv.slice(2), `用法: node scripts/sync-r2.mjs --dir <目录> --version <x.y.z> [--release-id <id>]`);
for (const k of ["dir", "version"]) if (!args[k]) fatal(`缺少 --${k}`);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** 解析出本轮要同步的 release：优先用 publish 作业给的 id（最可靠），退化为按 tag 查 */
async function resolveRelease() {
  if (args["release-id"]) {
    const rel = await releaseById(args["release-id"]);
    if (rel) return rel;
    fatal(`release id ${args["release-id"]} 不存在`);
  }
  const tag = `v${args.version}`;
  const byTag = await releaseByTag(tag);
  if (byTag && !byTag.draft) return byTag;
  const list = await api(`/repos/${repo()}/releases?per_page=100`);
  const hit = list.find((r) => r.tag_name === tag && !r.draft);
  if (!hit) fatal(`找不到已发布的 ${tag}`);
  return hit;
}

/** 资产就绪轮询：id 端点本就一致，这里主要防「刚转正式还没落盘」的极短窗口 */
async function waitForAssets(id, deadlineSec = 600) {
  const t0 = Date.now();
  for (;;) {
    const assets = await assetsOf(id);
    const names = assets.map((a) => a.name);
    if (assets.length >= 10 && names.includes("latest.json")) return assets;
    if (Date.now() - t0 > deadlineSec * 1000) {
      throw new Error(`等待 ${assets.length} 秒后资产仍不完整（${assets.length} 个，latest.json ${names.includes("latest.json") ? "在" : "缺"}）`);
    }
    console.log(`资产未就绪（现有 ${assets.length} 个），5s 后重试…`);
    await sleep(5000);
  }
}

function contentDisposition(name) {
  return `attachment; filename="${name.replace(/["\\]/g, "_")}"`;
}

async function main() {
  const base = (process.env.R2_PUBLIC_BASE || "").replace(/\/+$/, "");
  if (!/^https:\/\//.test(base)) fatal(`需要环境变量 R2_PUBLIC_BASE（https 基址），当前: "${base}"`);

  const rel = await resolveRelease();
  const assets = await waitForAssets(rel.id);
  console.log(`Release ${rel.tag_name}（id=${rel.id}）资产 ${assets.length} 个，共 ${(assets.reduce((s, a) => s + (a.size ?? 0), 0) / 1048576).toFixed(1)} MB`);

  // 下载全部资产（幂等：已存在且字节数对的跳过，方便失败重跑；--skip-download 全跳）
  if (!existsSync(args.dir)) {
    const { mkdirSync } = await import("node:fs");
    mkdirSync(args.dir, { recursive: true });
  }
  if (args["skip-download"]) {
    console.log(`--skip-download：不下载，按 ${args.dir} 现有同名文件核对（缺文件会被 --check-dir 拦下）`);
  } else {
    for (const a of assets) {
      const dest = join(args.dir, a.name);
      if (existsSync(dest) && statSync(dest).size === a.size) {
        console.log(`↷ 跳过 ${a.name}（已在，字节一致）`);
        continue;
      }
      const got = await downloadAsset(a, dest);
      console.log(`↓ ${a.name}（${(got / 1048576).toFixed(1)} MB）`);
    }
  }

  // r2-manifest.mjs 吃 `gh release view --json assets` 的形态：{assets:[{apiUrl,name}]}
  writeFileSync(join(args.dir, "assets.json"), JSON.stringify({ assets: assets.map((a) => ({ apiUrl: a.url, name: a.name })) }, null, 2) + "\n");

  // 清单改写（latest.json → R2 固定键 + 指纹；latest.json 本身被原地替换）
  const r2 = spawnSync(
    process.execPath,
    [
      "scripts/r2-manifest.mjs",
      "--in", join(args.dir, "latest.json"),
      "--assets-map", join(args.dir, "assets.json"),
      "--check-dir", args.dir,
      "--base", base,
      "--list-files", join(args.dir, "upload-fixed.tsv"),
      "--out", join(args.dir, "latest.json"),
    ],
    { encoding: "utf8", maxBuffer: 8 << 20 }
  );
  process.stdout.write(r2.stdout ?? "");
  if (r2.status !== 0) fatal(`r2-manifest.mjs 失败（exit ${r2.status}）：\n${r2.stderr}`);

  // 合并上传清单：固定键（latest/…）+ 全资产镜像（releases/v<ver>/…）
  const rows = new Map();
  for (const line of readFileSync(join(args.dir, "upload-fixed.tsv"), "utf8").trim().split("\n")) {
    const [local, key, cd] = line.split("\t");
    rows.set(`latest/${key}`, `${local}\tlatest/${key}\t${cd}`);
  }
  for (const a of assets) {
    // latest.json 单独走桶根（no-cache），不进带版本目录的镜像键，避免两份清单混淆
    if (a.name === "latest.json") continue;
    const key = `releases/v${args.version}/${a.name}`;
    if (!rows.has(key)) rows.set(key, `${a.name}\t${key}\t-`);
  }
  writeFileSync(join(args.dir, "upload.tsv"), [...rows.values()].join("\n") + "\n");
  const fixedCount = [...rows.keys()].filter((k) => k.startsWith("latest/")).length;
  console.log(`✔ 上传清单 ${join(args.dir, "upload.tsv")}：${rows.size} 个对象（更新固定键 ${fixedCount} + 版本镜像 ${rows.size - fixedCount}）`);
}

main().catch(fatal);
