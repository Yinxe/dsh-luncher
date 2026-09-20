# 发版手册

适用对象：维护 DSH Launcher 的人（包括 AI Agent）。**CHANGELOG.md 是发布说明的唯一来源** ——
GitHub Release 正文、`latest.json` 里的 `notes`、客户端「发现新版本」弹窗里看到的特性说明，
全部由它派生。所以发版的关键动作只有一个：**在下个版本号下写清用户能看到的变化。**

## 速查

```bash
# 1. 写 CHANGELOG.md：在 [Unreleased] 下面新增本版本段落
# 2. 三处版本号改成同一个（缺一个 CI 直接失败）
#    src-tauri/tauri.conf.json / src-tauri/Cargo.toml / package.json
# 3. 生成归档快照并自检
npm run notes:archive
npm run notes:check      # 版本号一致 + 本版说明存在 + 有条目
npm run notes            # 预览：这就是即将发出去的 Release 正文
npm run build            # 前端要能过 tsc + vite
# 4. 合进 main 并 push → CI 自动打包、发正式 Release
```

## 1. CHANGELOG 怎么写

在 `[Unreleased]` 之下新增：

```markdown
## [0.1.3] - 2026-09-25

### 新增

- **一句话讲清用户得到什么能力**：必要的时候补一句为什么 / 怎么用。

### 修复

- 描述「什么情况下会出问题 → 现在怎样了」。
```

硬性要求：

| 要求 | 原因 |
| --- | --- |
| 版本号与 `tauri.conf.json` 完全一致（`## [0.1.3]`） | CI 按版本号取段落，取不到就 fail |
| 至少有一条 `- ` 条目 | 空段落等于「没有更新说明」，CI 会拒绝发布 |
| 只写**用户可见**的变化 | 这段文字会原样出现在客户端弹窗里 |
| 不写内部重构/CI 细节，除非它影响用户（如发版方式、体积、安装形态） | 同上 |
| 已发布版本的段落**只增不改**（要纠正就发新版本说明） | 旧版本是历史归档，改它会让已成事实的说明与用户看到的不一致 |
| 日期用发布当天的本地日期 `YYYY-MM-DD` | 归档与 Release 会用到 |

反面例子（曾经真的这么发过）：

```
Windows：.exe / .msi　Linux：.deb / .rpm / .AppImage　macOS：universal .dmg
```

它没说这一版**改了什么**，用户点开更新提示看不到任何信息。

## 2. 版本号三处同步

| 文件 | 字段 | 说明 |
| --- | --- | --- |
| `src-tauri/tauri.conf.json` | `version` | **发版门禁读取它**，也是安装包版本 |
| `src-tauri/Cargo.toml` | `[package] version` | 编译进二进制，用于「当前版本」比较 |
| `package.json` | `version` | 仓库内一致性（私有包，不发布） |

`npm run notes:check` 会校验这三处，不一致会打印每个文件的当前值。

## 3. 归档快照

```bash
npm run notes:archive    # 从 CHANGELOG 重新生成 docs/releases/vX.Y.Z.md
```

- 生成物**不要手改**，改了会在下次生成时被覆盖；
- 归档的意义：不依赖 GitHub Releases 页面就能回溯某版本改了什么，自建更新清单也能直接引用；
- 忘记生成不会阻断发布，但 CI 会打一条 warning 提醒（`docs/releases/vX.Y.Z.md 归档快照还没生成`）。

## 4. CI 在 push main 之后做什么

`publish-tauri`（三平台矩阵）：

1. **版本门禁**：`gh api .../releases` 查到 `v<当前版本>` 已正式发布 → 整个作业跳过（绿勾 + notice），
   所以「没改版本号的普通提交」不会重复发版；
2. **发布说明门禁**：`node scripts/release-notes.mjs --check` —— 版本号不一致 / 本版本没有 CHANGELOG 段落 /
   段落里没有任何条目，**直接失败**，不会产出一个没有说明的 Release；正文同时写进 Job Summary 方便复核；
3. 打包（`tauri-action`）并把同一份正文作为 Release 正文 → `latest.json` 的 `notes`；
4. `publish` 作业：发布前再按 CHANGELOG 刷一遍正文，然后把草稿转正式。

## 5. 发布后确认（1 分钟）

```bash
# Release 正文（应等于 CHANGELOG 段落 + 安装包脚注）
gh release view v0.1.3 --json body --jq .body

# 客户端实际拿到的 notes（更新检查就是读这个文件）
curl -sL https://github.com/Yinxe/dsh-luncher/releases/latest/download/latest.json | head -c 400
```

再打开旧版本客户端点「检查更新」：横幅应出现「查看新特性」，弹窗里就是这份说明。

## 6. 自建更新清单模式

设置里填了「更新清单地址」时，客户端走 `check_manifest` 读 JSON：

```json
{ "version": "0.1.3", "notes": "把 CHANGELOG 段落贴进来即可", "url": "https://…/DSH-Launcher-0.1.3.AppImage" }
```

`notes` 建议直接复制 `npm run notes` 的输出（不要手写第二份，会和 CHANGELOG 漂移）。

## 7. 常见故障

| 现象 | 原因 / 处理 |
| --- | --- |
| CI 失败：`版本号不一致` | 三个版本文件没改齐，按提示逐个改 |
| CI 失败：`CHANGELOG.md 里没有 vX.Y.Z 的段落` | 先写 CHANGELOG（含 `## [X.Y.Z] - 日期`），再 push |
| CI 失败：`段落没有任何条目` | 只写了标题没写内容，至少补一条 `- ` |
| 客户端收不到更新 | Release 不能是草稿/pre-release；`releases/latest` 只认正式版本；确认 `latest.json` 能下载 |
| 发布后发现说明写错 | 直接编辑 GitHub Release 正文 + 修 `CHANGELOG.md`（`latest.json` 里的 notes 已经下发，改不了，除非重发/热修版本） |
