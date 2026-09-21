/**
 * dsh 版本号比较（前端侧，与 Rust `semver::compare` 语义对齐但更宽松）：
 * 先比 `x.y.z` 数字段，再比预发布后缀 —— 无后缀 > 有后缀，后缀之间按字典序
 * （alpha < beta < rc 的常见顺序因此天然成立）。
 * 版本页排序与更新日志的版本列表都用它。
 */
export function cmpVer(a: string, b: string): number {
  const core = (v: string) => v.replace(/^v/, "").split("-")[0].split(".").map((x) => parseInt(x, 10) || 0);
  const ca = core(a); const cb = core(b);
  for (let i = 0; i < 3; i++) if ((ca[i] ?? 0) !== (cb[i] ?? 0)) return (ca[i] ?? 0) - (cb[i] ?? 0);
  const pa = a.includes("-") ? a.split("-").slice(1).join("-") : "";
  const pb = b.includes("-") ? b.split("-").slice(1).join("-") : "";
  if (pa === pb) return 0;
  if (!pa) return 1;
  if (!pb) return -1;
  return pa.localeCompare(pb);
}
