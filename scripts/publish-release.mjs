#!/usr/bin/env node
// 把「本版本的草稿 release」转正式：门禁 → 刷正文 → 补 git tag → 转正式 → 回查。
// 取代原 release.yml 里那段 gh api + jq 的大 bash（0.3.1 两次发版都在这里踩过坑）。
//
// 关键决策（都有实测依据，详见 scripts/lib/gh.mjs 头注释）：
// - 资产一律走 `/releases/<id>/assets` 专用端点：`releases/tags/*` 与列表内嵌 assets
//   在转正式后会长时间返回滞后的空副本，按它判断会误杀（publish-r2 就这么死了两次）。
// - 认领草稿必须**验证过版本**：多版本草稿并存时，原先「取列表里最新的草稿」会把
//   不相干的转正式（0.3.1 首发就这样发过一个零资产 Release）。验证方式：草稿的
//   latest.json 资产里 manifest.version == 本版本。
// - 转正式前**无条件把 git tag 建出来并让草稿指过去**：tag 不存在时直接 draft=false
//   会走「发布时自动建 tag」路径，v0.1.3 的 Release 因此没有可溯源的 tag。
//
// 用法：node scripts/publish-release.mjs [--adopt-published]
//   --adopt-published：没有草稿时，若 v<版本> 已有正式 Release 就直接认领
//   （force_r2 手动触发用：不重打包，只让 publish-r2 拿着 releaseId 去补同步）。
// env: GH_TOKEN / GITHUB_REPOSITORY / GITHUB_STEP_SUMMARY / GITHUB_OUTPUT

import { spawnSync } from "node:child_process";
import {
  api, appVersion, assetsOf, drafts, fatal, releaseByTag, repo,
  setOutputs, summary,
} from "./lib/gh.mjs";

const args = { adoptPublished: false };
for (const a of process.argv.slice(2)) {
  if (a === "--adopt-published") args.adoptPublished = true;
  else fatal(`未知参数: ${a}（只支持 --adopt-published）`);
}

const R = () => `/repos/${repo()}`;

/** 草稿的 latest.json 里读出版本号；没有该资产或解析失败 → null */
async function draftVersion(rel) {
  const assets = await assetsOf(rel.id);
  const mj = assets.find((a) => a.name === "latest.json");
  if (!mj) return null;
  try {
    const res = await api(new URL(mj.url).pathname, { accept: "application/octet-stream", raw: true });
    return String(JSON.parse(await res.text()).version ?? "");
  } catch {
    return null;
  }
}

/** 转正式前必须资产齐备：latest.json + 各平台安装包/签名（完整 14 个，按 ≥10 判）。
 *  零资产 Release 转正式 = 客户端检查更新拿不到任何包，宁可 CI 红。 */
function gateAssets(assets, label) {
  const names = assets.map((a) => a.name);
  if (!(assets.length >= 10 && names.includes("latest.json"))) {
    throw new Error(
      `${label}资产不完整（${assets.length} 个，latest.json ${names.includes("latest.json") ? "在" : "缺"}），拒绝转正式`
    );
  }
  return `资产齐备（${assets.length} 个）`;
}

async function main() {
  const V = appVersion();
  const REAL = `v${V}`;

  // 幂等：上一轮打包成功、publish 死在中途时，正式版可能已经在了 —— 直接认领，
  // 不再动它（重复 PATCH 一个已发布的 Release 只会把正文/标签改出意外）。
  const already = await releaseByTag(REAL);
  if (already && !already.draft) {
    console.log(`${REAL} 已是正式 Release（id=${already.id}），跳过发布，只做下游 R2 同步`);
    setOutputs({ released: "true", releaseId: String(already.id), tag: REAL, mode: "already-published" });
    summary([`## ${REAL} 已发布`, `Release id：\`${already.id}\`（本轮不重复转正式，只补 R2）。`]);
    return;
  }

  // 1) 找**本版本**的草稿（含 tauri-action 可能留下的 untagged-<sha> 占位 tag）
  const cands = (await drafts()).filter((r) => r.tag_name === REAL || r.tag_name.startsWith("untagged-"));
  const matched = [];
  for (const c of cands) {
    const dv = await draftVersion(c);
    if (dv === V) matched.push(c);
    else console.log(`跳过草稿 ${c.tag_name}（latest.json 版本=${dv ?? "无/解析失败"}，不是 ${V}）`);
  }
  // 多平台并发时理论上可能出现多个同版本草稿（各自带部分资产）——取列表快照里资产
  // 数最多的那个，其余留人工处理（合并草稿超出流水线职责）。
  let draft = null;
  if (matched.length) {
    matched.sort((a, b) => (b.assets?.length ?? 0) - (a.assets?.length ?? 0));
    draft = matched[0];
  }

  // 2) 没有草稿：可能是「版本号没变推 main」（正常空跑），也可能是 force_r2 要补同步
  if (!draft) {
    if (args.adoptPublished) {
      const pub = await releaseByTag(REAL);
      if (pub && !pub.draft) {
        console.log(`无草稿，force 模式认领已发布的 ${REAL}（id=${pub.id}），只补 R2 同步`);
        setOutputs({ released: "true", releaseId: String(pub.id), tag: REAL, mode: "adopted" });
        summary(["## R2 强制同步", `已发布 Release：\`${REAL}\`（id=\`${pub.id}\`），本轮不重新打包。`]);
        return;
      }
      fatal(`force 模式要求 ${REAL} 已正式发布，但没找到（有草稿就先发草稿，什么也没有就正常发版）`);
    }
    console.log(`没有 ${REAL} 的草稿（版本号未变时就是这样），结束`);
    setOutputs({ released: "false", releaseId: "", tag: REAL, mode: "none" });
    return;
  }

  // 3) 转正式前的资产门禁（专用端点重读一遍，不用列表里的滞后快照）
  const rel = await api(`${R()}/releases/${draft.id}`);
  let assets = await assetsOf(rel.id);
  console.log(`草稿 ${rel.tag_name}（id=${rel.id}）：${gateAssets(assets, "草稿")}`);

  // 4) 正文统一按 CHANGELOG 刷一遍：草稿可能由任一平台先创建，保证页面与客户端说明一致
  const notes = spawnSync(process.execPath, ["scripts/release-notes.mjs", "--check"], {
    encoding: "utf8",
    maxBuffer: 8 << 20,
  });
  if (notes.status !== 0) throw new Error(`release-notes.mjs --check 失败：\n${notes.stderr}`);

  // 5) 无条件保证 git tag 存在（打在草稿的 target commit 上；已存在则原样不动），
  //    再把草稿指到真实 tag —— 两步都完成后才允许转正式。
  let sha = rel.target_commitish;
  if (!/^[0-9a-f]{40}$/.test(sha)) {
    sha = (await api(`${R()}/commits/${encodeURIComponent(sha)}`)).sha;
  }
  const existingRef = await api(`${R()}/git/ref/tags/${REAL}`, { allow404: true });
  if (!existingRef) {
    await api(`${R()}/git/refs`, { method: "POST", body: { ref: `refs/tags/${REAL}`, sha } });
    console.log(`已创建 tag ${REAL} → ${sha.slice(0, 8)}`);
  }
  if (rel.tag_name !== REAL) {
    await api(`${R()}/releases/${rel.id}`, { method: "PATCH", body: { tag_name: REAL } });
    console.log(`草稿占位 tag ${rel.tag_name} → ${REAL}`);
  }
  await api(`${R()}/releases/${rel.id}`, {
    method: "PATCH",
    body: { body: notes.stdout, draft: false, prerelease: false },
  });

  // 6) 转正式后回查：资产被吞就红在这里，绝不把坏 Release 留在 latest 上毒害更新链路。
  //    （顺带观察滞后副本：tag 接口何时追上不影响本流水线，publish-r2 用 releaseId。）
  assets = await assetsOf(rel.id);
  console.log(`已发布 ${REAL}：${gateAssets(assets, "转正式后")}`);
  setOutputs({ released: "true", releaseId: String(rel.id), tag: REAL, mode: "published" });
  summary([
    `## 已发布 ${REAL}`,
    `Release id：\`${rel.id}\`（R2 同步按 id 读资产，绕开滞后的 tag 接口）`,
    `资产：${assets.length} 个，共 ${(assets.reduce((s, a) => s + (a.size ?? 0), 0) / 1048576).toFixed(1)} MB`,
  ]);
}

main().catch(fatal);
