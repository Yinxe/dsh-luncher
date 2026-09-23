#!/usr/bin/env node
// 发版门禁：v<版本> 已经正式发布过 → skip=true，publish-tauri 之后的构建步骤全部跳过
// （版本号没改就推 main，不再白跑 7 分钟三端打包）。
// force_r2 手动触发时同样 skip=true：强制模式只补 R2 同步，不重新打包。
//
// 用法（CI）：node scripts/release-gate.mjs
//   env: GH_TOKEN / GITHUB_REPOSITORY / GITHUB_OUTPUT / FORCE_R2
// 本地跑一下看看当前版本发布状态：直接执行，skip 会打印出来。

import { appVersion, fatal, releaseByTag, setOutputs } from "./lib/gh.mjs";

try {
  const tag = `v${appVersion()}`;
  const force = process.env.FORCE_R2 === "true";
  const rel = await releaseByTag(tag);
  const published = Boolean(rel && !rel.draft);
  const skip = force || published;
  const why = force ? "强制模式（只补 R2 同步）" : published ? `${tag} 已正式发布` : `${tag} 未发布，走完整流程`;
  console.log(`release-gate: skip=${skip}（${why}）`);
  setOutputs({ skip: String(skip) });
} catch (e) {
  fatal(e);
}
