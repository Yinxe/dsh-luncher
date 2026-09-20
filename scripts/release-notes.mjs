#!/usr/bin/env node
/**
 * 从 CHANGELOG.md 提取某个版本的发布说明。
 *
 * CHANGELOG.md 是**唯一来源**：GitHub Release 正文、客户端「发现新版本」里展示的
 * 新特性说明、docs/releases/ 归档快照，全部由它派生，避免出现「Release 里只写一句
 * 安装包格式」这种没有信息量的说明。
 *
 * 用法：
 *   node scripts/release-notes.mjs                 # 打印当前版本（tauri.conf.json）的说明
 *   node scripts/release-notes.mjs 0.1.1           # 打印指定版本
 *   node scripts/release-notes.mjs --write         # 同时写入 docs/releases/vX.Y.Z.md
 *   node scripts/release-notes.mjs --all --write   # 归档所有已发布版本
 *   node scripts/release-notes.mjs --check         # 发版门禁：版本号三处一致 + 本版本有说明
 *
 * 退出码：0 成功；1 校验失败 / 版本段缺失（CI 会直接 fail 并打印怎么补）。
 */
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CHANGELOG = join(ROOT, "CHANGELOG.md");
const ARCHIVE_DIR = join(ROOT, "docs", "releases");
const REPO = "https://github.com/Yinxe/dsh-luncher";

const PACKAGE_FILES = [
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "package.json",
];

const FOOTER = [
  "---",
  "",
  `**安装包**：Windows \`.exe\` / \`.msi\`　·　Linux \`.deb\` / \`.rpm\` / \`.AppImage\`　·　macOS \`universal .dmg\``,
  `**完整更新日志**：[CHANGELOG.md](${REPO}/blob/main/CHANGELOG.md)`,
].join("\n");

function fail(msg) {
  console.error(`\n✖ ${msg}\n`);
  process.exit(1);
}

/** 读取三处版本号：tauri.conf.json / Cargo.toml / package.json */
function readVersions() {
  const conf = JSON.parse(readFileSync(join(ROOT, "src-tauri/tauri.conf.json"), "utf8"));
  const cargo = readFileSync(join(ROOT, "src-tauri/Cargo.toml"), "utf8");
  const cargoVer = (cargo.match(/^\s*version\s*=\s*"([^"]+)"/m) || [])[1];
  const pkg = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
  return { conf: conf.version, cargo: cargoVer, pkg: pkg.version };
}

/** 解析 CHANGELOG：返回 [{ version, date, body }]（保持文件顺序） */
function parseChangelog() {
  if (!existsSync(CHANGELOG)) fail(`找不到 ${CHANGELOG}`);
  const lines = readFileSync(CHANGELOG, "utf8").split(/\r?\n/);
  const headRe = /^##\s+\[([^\]]+)\](?:\s*-\s*(\S+))?\s*$/;
  const out = [];
  let cur = null;
  for (const line of lines) {
    const m = line.match(headRe);
    if (m) {
      if (cur) out.push(cur);
      cur = { version: m[1], date: m[2] || null, lines: [] };
      continue;
    }
    if (cur) cur.lines.push(line);
  }
  if (cur) out.push(cur);
  for (const s of out) {
    // 版本段落到下一个 `## [` 为止；文件末尾的 `[0.1.0]: https://…` 参考链接
    // 属于文档脚手架，不属于任何版本说明，这里剥掉
    const lines = s.lines.join("\n").replace(/\s+$/, "").split("\n");
    while (lines.length) {
      const last = lines[lines.length - 1];
      if (/^\s*\[[^\]]+\]:\s*\S+\s*$/.test(last) || !last.trim()) lines.pop();
      else break;
    }
    s.body = lines.join("\n").replace(/^\s*\n+/, "");
  }
  return out;
}

/** 版本号一致性：三处必须相同，否则 CI 直接失败（发版前 7 分钟就能发现） */
function checkVersions(v) {
  const bad = PACKAGE_FILES.filter((f, i) => [v.conf, v.cargo, v.pkg][i] !== v.conf);
  if (bad.length) {
    fail(
      `版本号不一致：\n` +
        `    src-tauri/tauri.conf.json = ${v.conf}\n` +
        `    src-tauri/Cargo.toml      = ${v.cargo}\n` +
        `    package.json              = ${v.pkg}\n` +
        `  请把这三个文件都改成同一个版本号（见 docs/RELEASING.md）。`
    );
  }
}

function archivePath(version) {
  return join(ARCHIVE_DIR, `v${version}.md`);
}

function archiveContent(version, date, body) {
  return [
    `# DSH Launcher v${version} 发布说明`,
    "",
    date ? `发布日期：${date}` : "",
    `GitHub Release：${REPO}/releases/tag/v${version}`,
    "",
    "> 本文件由 `npm run notes:archive` 从 CHANGELOG.md 生成，请勿手改。",
    "",
    body,
    "",
    FOOTER,
    "",
  ]
    .filter((l) => l !== undefined)
    .join("\n")
    .replace(/\n{3,}/g, "\n\n");
}

function main() {
  const argv = process.argv.slice(2);
  const flags = new Set(argv.filter((a) => a.startsWith("--")));
  const positional = argv.filter((a) => !a.startsWith("--"));

  const sections = parseChangelog();
  const current = readVersions().conf;
  const wantAll = flags.has("--all");
  const check = flags.has("--check");
  const write = flags.has("--write");

  if (check || (!wantAll && !positional.length && write)) checkVersions(readVersions());

  if (check) {
    const sec = sections.find((s) => s.version === current);
    if (!sec) {
      fail(
        `CHANGELOG.md 里没有 v${current} 的段落。\n` +
          `  发版前请在 CHANGELOG.md 顶部（[Unreleased] 之下）加上：\n\n` +
          `    ## [${current}] - ${new Date().toISOString().slice(0, 10)}\n\n` +
          `    ### 新增 / 变更 / 修复\n    - 用一句话说清用户能看到的变化\n`
      );
    }
    if (!/^\s*[-*]\s+\S/m.test(sec.body)) {
      fail(
        `CHANGELOG.md 里 v${current} 的段落没有任何条目。\n` +
          `  Release 正文与客户端展示的新特性都取自这里，请至少写一条「- 」开头的用户可见变化。`
      );
    }
    if (!existsSync(archivePath(current))) {
      warn(
        `docs/releases/v${current}.md 归档快照还没生成，合进 main 前请执行：npm run notes:archive`
      );
    }
    console.error(`✔ CHANGELOG 校验通过：v${current} 的发布说明已就绪（stdout 即发布正文）`);
  }

  if (wantAll) {
    if (!write) fail(`--all 需要配合 --write 使用（会把每个版本写入 docs/releases/）`);
    mkdirSync(ARCHIVE_DIR, { recursive: true });
    const published = sections.filter((s) => s.version !== "Unreleased" && /^\d/.test(s.version));
    for (const s of published) {
      writeFileSync(archivePath(s.version), archiveContent(s.version, s.date, s.body), "utf8");
      console.log(`✔ 归档 docs/releases/v${s.version}.md`);
    }
    return;
  }

  const version = positional[0] || current;
  const sec = sections.find((s) => s.version === version);
  if (!sec) {
    fail(
      `CHANGELOG.md 里找不到 v${version}。可选版本：` +
        sections.map((s) => s.version).join(", ")
    );
  }
  if (sec.version === "Unreleased") {
    fail(`[Unreleased] 是未发布草稿，不能作为发布说明。请先把版本段落移到 ## [${current}] 下。`);
  }

  const body = `${sec.body}\n\n${FOOTER}\n`;
  if (write) {
    mkdirSync(ARCHIVE_DIR, { recursive: true });
    writeFileSync(archivePath(version), archiveContent(version, sec.date, sec.body), "utf8");
    console.error(`✔ 归档 docs/releases/v${version}.md`);
  }
  process.stdout.write(body);
}

function warn(msg) {
  // GitHub Actions 里显示成 annotation，本地就是普通提示
  console.error(process.env.GITHUB_ACTIONS ? `::warning::${msg}` : `⚠ ${msg}`);
}

main();
