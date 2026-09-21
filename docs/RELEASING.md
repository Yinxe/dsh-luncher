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

## 7. 自建更新源（Cloudflare R2）

客户端**默认**从自建 R2 源下载更新（基址写死在 `src-tauri/src/update_check.rs` 的
`R2_BASE`），GitHub 源保留为兜底；用户可在设置里二选一。实测同一条网络下 81MB 的
AppImage：R2 约 3.3MB/s、GitHub 约 1.7MB/s（快 1.9 倍，下载时间 ~47s → ~24s）。

### 一次性准备（已完成的部分标 ✅）

1. ✅ **建桶并开公共访问**：

   ```bash
   npx wrangler r2 bucket create dsh-luncher-release --location apac
   npx wrangler r2 bucket dev-url enable dsh-luncher-release
   ```

   公共基址形如 `https://pub-<hash>.r2.dev`，**必须同时写进两处且保持一致**：

   | 位置 | 用途 |
   | --- | --- |
   | `src-tauri/src/update_check.rs` 的 `R2_BASE` | 客户端用它拼清单地址 |
   | `.github/workflows/release.yml` 的 `R2_PUBLIC_BASE` | CI 用它改写清单里的下载地址 |

2. ⬜ **建 R2 API Token**（只能网页建 —— 用 OAuth 登录的 wrangler 没有建 token 的权限）：
   Cloudflare Dashboard → R2 → API → Manage API Tokens → Create API Token →
   权限 `Object Read & Write`，Scope 选 `dsh-luncher-release`。
   记下 **Access Key ID** 与 **Secret Access Key**。

3. ⬜ **填三个 secrets**（仓库 Settings → Secrets and variables → Actions）：

   | Secret | 值 |
   | --- | --- |
   | `R2_ACCOUNT_ID` | Cloudflare 账号 ID（`wrangler whoami` 可见） |
   | `R2_ACCESS_KEY_ID` | 上一步的 Access Key ID |
   | `R2_SECRET_ACCESS_KEY` | 上一步的 Secret Access Key |

   没配也能正常发版：`publish-r2` 作业会自动跳过并打一条 notice。

### 每次发布自动发生什么

`publish-r2` 作业（`needs: publish`，只在发布成功后跑）：

1. `gh release download` 拉本次发布的全部资产；
2. `node scripts/r2-manifest.mjs` 把 `latest.json` 里的下载地址从 GitHub 资产 API
   （`api.github.com/repos/…/releases/assets/<id>`）改写成 `<R2_PUBLIC_BASE>/<文件名>`，
   **签名原样保留**（安装包字节没变，客户端照常验签），并校验清单引用的文件都在本地；
3. `aws s3 cp` 上传清单引用的 6 个安装包（`max-age=31536000, immutable`）与
   `latest.json`（`no-cache, max-age=0`）；
4. 清理旧版本产物，再自检：从公网取回清单，确认每个平台地址都指向自建源。

### 旧版本会被清掉吗

会，但不是立刻：

- `latest.json` 每次都被**覆盖**（同一个 key）；
- 安装包按文件名新增，所以旧版本会先留着；
- 清理步骤随后删除「不在本次清单里、且超过 `KEEP_DAYS`（默认 7 天）」的对象 ——
  当前清单引用的文件与 `latest.json` **永远不删**。

留 7 天宽限期是为了避开这个竞态：客户端刚拿到旧清单、还没下完，旧文件就被删 → 404。
桶里因此稳定在「当前版本 + 最近一次发布」约 120–240 MB；想留更多版本就把
`KEEP_DAYS` 调大（或删掉这一步，代价是每次发布多堆约 118 MB）。
GitHub Release 始终是完整归档，R2 只服务「更新」这一条链路。

手动补传（例如补历史版本）等价于：

```bash
gh release download vX.Y.Z --dir r2-upload --clobber
gh release view vX.Y.Z --json assets > r2-upload/assets.json
node scripts/r2-manifest.mjs --in r2-upload/latest.json --assets-map r2-upload/assets.json \
  --check-dir r2-upload --base "$R2_PUBLIC_BASE" --list-files r2-upload/files.txt --out r2-upload/latest.json
# 再用 wrangler 逐个上传（无需 S3 凭据，用本机 OAuth 登录态即可）：
npx wrangler r2 object put dsh-luncher-release/<文件名> --file r2-upload/<文件名> --remote
```

### 必须记住的两条

- **`latest.json` 要随每次发布更新**：更新器按顺序取第一个能解析的清单，R2 上的清单
  陈旧会让客户端停在旧版本（届时用户可在设置里切到 GitHub 源自救）。
- **`latest.json` 的缓存必须保持 `no-cache`**，否则新版本提示会被 CDN 缓存挡住。

### 其它

- 只上传清单真正引用的包（AppImage / deb / rpm / app.tar.gz / msi / setup.exe）；
  `.dmg` 不在更新清单里（它供首次下载），要镜像的话另外传。
- 每个版本约 118 MB（AppImage 81MB 占大头）。不清理的话一年几个 GB、R2 存储
  约 $0.015/GB/月，钱不多但没必要堆着。
- 想换自定义域名：改上面表格里的两处基址即可。`r2.dev` 是 Cloudflare 的托管开发域名，
  有速率限制，流量大了建议绑自定义域名。

## 8. 常见故障

| 现象 | 原因 / 处理 |
| --- | --- |
| CI 失败：`版本号不一致` | 三个版本文件没改齐，按提示逐个改 |
| CI 失败：`CHANGELOG.md 里没有 vX.Y.Z 的段落` | 先写 CHANGELOG（含 `## [X.Y.Z] - 日期`），再 push |
| CI 失败：`段落没有任何条目` | 只写了标题没写内容，至少补一条 `- ` |
| 客户端收不到更新 | Release 不能是草稿/pre-release；`releases/latest` 只认正式版本；确认 `latest.json` 能下载 |
| 发布后发现说明写错 | 直接编辑 GitHub Release 正文 + 修 `CHANGELOG.md`（`latest.json` 里的 notes 已经下发，改不了，除非重发/热修版本） |
