# 发版手册

适用对象：维护 DSH Starter 的人（包括 AI Agent）。**CHANGELOG.md 是发布说明的唯一来源** ——
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

### 为什么打包要 10 分钟（时间都花在哪）

三平台**并行**跑，所以墙钟时间 = 最慢的那个平台。以 0.1.7 那次为例，从日志时间戳拆出来：

| 平台 | 装 Node/Rust/`npm ci` | 编译**全部依赖** | 编自身 crate + thin-LTO 链接 | 打包 + 上传 | 作业总计 |
| --- | --- | --- | --- | --- | --- |
| Linux | 约 45s（apt 占 25s） | 174s | 137s | 约 95s（AppImage 79s） | 451s |
| Windows | 约 40s | 324s | **266s** | 约 25s | 707s |
| macOS（universal） | 约 45s | 516s（两个架构各编一遍） | 175s | 约 75s | 774s |

也就是说「10 分钟」其实是两段：**冷编译全部依赖** + **最终 thin-LTO 链接**。
本地之所以快，是因为 `src-tauri/target` 已经躺着编好的依赖（本机实测：target 全热
0.43 秒；只改了版本号、重编自身 + 链接 1 分 38 秒；而 CI 每次都是空 target）。

针对**可以缓存的两段**（依赖编译、打包工具下载），已经加了缓存：

- `Swatinem/rust-cache@v2`，`workspaces: src-tauri -> target`（Cargo.toml 在子目录里，
  用默认值会找不到 target、等于没开），缓存 registry + `target/`；
- `setup-node` 的 `cache: npm`；
- `actions/cache@v4` 单独缓存 **AppImage 打包工具**（`~/.cache/tauri`，约 36MB）：
  tauri-bundler 是「文件不存在才下载」（`linuxdeploy.rs` 的 `prepare_tools`），所以恢复这份
  目录就能省掉日志里那 5 次 `Downloading AppRun-x86_64 / linuxdeploy-*`。
  **不要**图省事改成 `tauri.conf.json` 的 `bundle.useLocalToolsDir: true`（工具落到 `target/.tauri`）：
  rust-cache 保存前会 `cleanTargetDir`，把 target 下非 profile 目录里的散落文件全删掉 ——
  工具会在存缓存那一刻被清空，配置看起来生效、实际每次照旧重下，而且不报错。
  也不能塞进 rust-cache 那份条目（`cache-directories`）：条目不可覆盖，恢复命中原 key 会直接
  跳过保存，新路径要等下一次依赖/rustc 变更才带得上。key 固定为 `tauri-tools-linux-v1`
  （这 5 个文件没有版本概念、文件名写死在 bundler 里，缺哪个下哪个），要强制重下就把
  `tauri-tools-linux-v1` 改成 `-v2`（release.yml 与 cache-warm.yml 两处同步改）；
- `.github/workflows/cache-warm.yml`：每周一跑一次，**只恢复、不保存**（`save-if: false`），
  专门刷新 GitHub「7 天不访问即清」的 TTL —— Rust 构建缓存与上面的 AppImage 工具缓存
  **各是一份条目，各碰一次**（工具那份用 `actions/cache/restore`，只读）。为什么不能让它保存：
  `shared-key` 与发版作业同名，一旦让这个不编译的作业写缓存，就会把一份空 target 写进同一个
  key —— 而缓存条目不可覆盖，发版那次反而只能拿到空缓存（污染）。恢复命中同样算「访问」，够用了。

三点要知道（前两条是读 `Swatinem/rust-cache` 源码确认的）：

- **版本号变动不会让缓存失效**：它算 key 时会把 manifest 里的 `version` 统一改写成
  `0.0.0`，缓存前缀只含 `shared-key` + OS/架构 + rustc 版本 + 环境变量；即便真改了依赖，
  它也把前缀当 `restore-keys` 用 —— 仍能恢复到上一份 `target`，剩下的交给 cargo 自己的
  指纹做增量编译。所以**不存在「依赖一变就又冷编译 10 分钟」**这回事；
- **缓存条目不可覆盖**：命中同一个 key 时 `actions/cache` 不会再写。这是保活作业必须
  `save-if: false` 的原因 —— 一个不编译的作业若把空 `target` 写进 `tauri-release`，
  发版那次反而只能拿到空缓存，而且要等 7 天淘汰才恢复；
- 容量与第二段：GitHub 单仓库缓存上限 10GB（本机 `target/release` 4GB，压缩后每平台
  约 1～1.5GB，够用），超限按 LRU 淘汰。而**最终 thin-LTO 链接（137～266s）缓存动不了**：
  可选的下一步是 `[profile.release] lto = "thin"` → `false`，代价是二进制略大、运行性能
  略降 —— 这是取舍，要改先实测，别凭感觉换。

另外有个坑：**发新版期间别再往 main 推提交**。版本门禁查的是「已经*正式发布*的版本」，
而打包中的 Release 还停在草稿状态 —— 此时推 main 会通过门禁、再起一整轮三端构建
（白烧 10 分钟，两个 run 还会抢同一个草稿）。等这轮跑完再推。

## 5. 发布后确认（1 分钟）

```bash
# Release 正文（应等于 CHANGELOG 段落 + 安装包脚注）
gh release view v0.1.3 --json body --jq .body

# 客户端实际拿到的 notes（更新检查就是读这个文件）
curl -sL https://github.com/Yinxe/dsh-starter/releases/latest/download/latest.json | head -c 400
```

再打开旧版本客户端点「检查更新」：横幅应出现「查看新特性」，弹窗里就是这份说明。

## 6. 自建更新清单模式

设置里填了「更新清单地址」时，客户端走 `check_manifest` 读 JSON：

```json
{ "version": "0.1.3", "notes": "把 CHANGELOG 段落贴进来即可", "url": "https://…/DSH-Starter-0.1.3.AppImage" }
```

`notes` 建议直接复制 `npm run notes` 的输出（不要手写第二份，会和 CHANGELOG 漂移）。

## 7. 自建更新源（Cloudflare R2）

客户端**默认**从自建 R2 源下载更新（基址写死在 `src-tauri/src/update_check.rs` 的
`R2_BASE`），GitHub 源保留为兜底；用户可在设置里二选一。

实测（国内直连、不走代理）拉 81MB 的 AppImage：**GitHub 的 release 下载直接连不上**
（curl 返回 000），自建源 200、约 **3.3MB/s**（首字节 ~0.9s）。走代理时两者都受同一条
隧道限速（~0.8–1.6MB/s），差距不明显 —— 所以自建源的价值首先是**可用性**，
其次才是不受 GitHub 速率限制。

### 一次性准备（已完成的部分标 ✅）

1. ✅ **建桶并开公共访问**（两个桶都在：旧桶 `dsh-luncher-release` 只留历史、不再更新，
   当前用的是 `dsh-starter-release`，见后面「换桶记录」）：

   ```bash
   npx wrangler r2 bucket create dsh-starter-release --location apac
   npx wrangler r2 bucket dev-url enable dsh-starter-release
   ```

   公共基址形如 `https://pub-<hash>.r2.dev`，**必须同时写进下面几处且保持一致**：

   | 位置 | 用途 |
   | --- | --- |
   | `src-tauri/src/update_check.rs` 的 `R2_BASE` | 客户端用它拼清单地址 |
   | `site/src/lib/release.ts` 的 `R2_BASE` | 下载页拼自建源直链 |
   | `.github/workflows/release.yml` 的 `R2_PUBLIC_BASE` | CI 用它改写清单里的下载地址 |
   | `.github/workflows/release.yml` 的 `R2_BUCKET` | CI 往哪个桶上传（必须与基址同一个桶） |

2. ⬜ **建 R2 API Token**（只能网页建 —— 用 OAuth 登录的 wrangler 没有建 token 的权限）：
   Cloudflare Dashboard → R2 → API → Manage API Tokens → Create API Token →
   权限 `Object Read & Write`，Scope 选桶。
   ⚠️ **Scope 是按桶列的**：只覆盖旧桶的 token 写不了新桶。0.2.1 换到 `dsh-starter-release`
   之后实测 CI 用的这份 token 仍能写入（即它的 Scope 已覆盖新桶 / 是账号级），所以**不必换
   secret**；但将来若重建 token，记得把在用的桶都列进 Scope，否则 `publish-r2` 会 403 ——
   失败是响的，只是 R2 上的清单会停在旧版本。
   记下 **Access Key ID** 与 **Secret Access Key**。

3. ⬜ **填三个 secrets**（仓库 Settings → Secrets and variables → Actions）：

   | Secret | 值 |
   | --- | --- |
   | `R2_ACCOUNT_ID` | Cloudflare 账号 ID（`wrangler whoami` 可见） |
   | `R2_ACCESS_KEY_ID` | 上一步的 Access Key ID |
   | `R2_SECRET_ACCESS_KEY` | 上一步的 Secret Access Key |

   没配也能正常发版：`publish-r2` 作业会自动跳过并打一条 **warning**（不是静默的 notice ——
   客户端默认走 R2，这里跳过就意味着 R2 上的清单会停在旧版本，得照本节末尾手动补传）。

### 换桶记录（0.2.1 起客户端走新桶）

产品 0.2.0 改名成 DSH Starter，桶也跟着换 —— **R2 不支持改桶名**，所以是新建 + 迁移：

| 项 | 值 |
| --- | --- |
| 旧桶（只留历史，不再更新） | `dsh-luncher-release` |
| 当前桶 | `dsh-starter-release` |
| 公开基址 | `https://pub-576ca711d9cf4cfe96b58195dfe6ce81.r2.dev` |

迁移时是照第 7 节的手动补传流程，把 0.2.0 的六个安装包与改写过的清单先铺进新桶，再换的基址 ——
所以下载页切到新桶那一刻，自建源直链不会是死链。四处基址的同改随 **0.2.1** 发布
（0.2.0 的二进制里仍是旧基址，所以它读不到新桶，需要手动装一次或把更新源切到 GitHub）。

✅ **凭据已验证可用**：0.2.1 的 `publish-r2` 成功把清单与安装包写进了 `dsh-starter-release`
（新桶 `latest.json` 已是 0.2.1），所以 **GitHub 上的 R2 secret 不需要更换**。

⚠️ **为什么单独写一段**：更新器按顺序取「第一个能解析的清单」，R2 排在 GitHub 兜底之前。
换桶时只改一半（例如上传改到新桶、客户端仍读旧桶）会让旧桶那份**陈旧却依然有效**的清单
被先取到，客户端据此判定「已是最新」，**连 GitHub 兜底都走不到 —— 静默卡死更新，且不报任何错**。
所以这四处永远一起改：`update_check.rs` 的 `R2_BASE`（客户端）、`release.ts` 的 `R2_BASE`（下载页）、
`release.yml` 的 `R2_BUCKET` 与 `R2_PUBLIC_BASE`（CI）。

### 每次发布自动发生什么

`publish-r2` 作业（`needs: publish`，**只在这一轮真的发布了新版本时才跑**；版本号没变就推 main
属于空跑，整个作业会跟着跳过，不会白下载白上传 118MB）。0.3.1 起重写为脚本，全部逻辑在
`scripts/` 里（为什么不用 `gh release download`：见下）：

1. `node scripts/sync-r2.mjs --dir r2-upload --version <X.Y.Z> --release-id <id>`：
   按 publish 作业输出的 **releaseId** 走 `/releases/<id>/assets` 专用端点轮询并下载
   **本次发布的全部资产**。⚠️ 绝不走 `gh release download` / `releases/tags/*`：草稿转正式后
   这两个接口会长时间返回**滞后的空副本**（0.3.1 连挂两轮的死因，实测 1 小时+ 仍报 0 资产，
   而 id 端点始终是 14 个）；
2. sync-r2 内部调 `node scripts/r2-manifest.mjs` 把 `latest.json` 里的下载地址从 GitHub 资产 API
   （`api.github.com/repos/…/releases/assets/<id>`）改写成**固定键**
   `<R2_PUBLIC_BASE>/latest/<平台>.扩展名?v=<版本>`，**签名原样保留**（安装包字节没变，
   客户端照常验签），并校验清单引用的文件都在本地；
3. sync-r2 汇总出 `upload.tsv`（`<本地文件> <TAB> <对象键（完整路径）> <TAB> Content-Disposition`，
   disposition 为 `-` 表示不加该头），workflow 用 `aws s3 cp` 逐行上传：
   固定键（`latest/…`）与全资产镜像（`releases/v<版本>/<原文件名>`）都用 `immutable` 长缓存，
   固定键同时带 `Content-Disposition: attachment; filename="<原始资产名>"`（见下节）；
   最后用 `no-cache` 覆盖桶根的 `latest.json`；
4. 自检 `node scripts/verify-r2.mjs`：从公网取回清单，确认**版本号就是本次发布**、每个平台地址
   都落在 `<基址>/latest/` 下，再对 `upload.tsv` 里每个对象逐个 HEAD，确认 200；固定键还要确认
   `Content-Disposition` 里确实带着原始文件名（少一个就红）。

### 桶里的布局（固定键，只有一份 latest）

```
dsh-starter-release/
├── latest.json                        # 更新清单：每次覆盖（no-cache）
├── latest/                            # 固定键（immutable + ?v=<版本> 指纹）
│   ├── windows-x64-setup.exe          # NSIS（windows-x86_64-nsis）→ 下载名 DSH.Starter_<版本>_x64-setup.exe
│   ├── windows-x64.msi                # MSI（windows-x86_64 / -msi）→ 下载名 DSH.Starter_<版本>_x64_en-US.msi
│   ├── darwin-universal.app.tar.gz    # macOS 三架构共用 universal 包
│   ├── linux-x86_64.AppImage
│   ├── linux-x86_64.deb
│   └── linux-x86_64.rpm
└── releases/                          # 本次发布**全部** GitHub 资产的 1:1 镜像（自 0.3.1 起）
    └── v<版本>/<原始资产名>            # 含 .sig 与 .dmg：GitHub 不可达时这里也有完整一套
```

「键名」和「下载名」是两回事：键为了地址稳定、永远不变；下载名由 `Content-Disposition`
给出，永远带版本号和产品名（见上一节）。

- 地址**不带版本号**：每次发布覆盖同名对象，`latest/` 下永远只有一份「当前最新」，
  旧版本自然消失、**不需要任何清理步骤**；`releases/v<版本>/` 每版一份，占桶约 118 MB × 版本数
  （R2 存储按量计费，但流量免费，先不用管回收）。
- 顺带的好处：`<基址>/latest/windows-x64-setup.exe` 可以直接当「永久最新版下载链接」分享；
- **更新只认 `latest.json` 里出现的固定键**；`releases/v<版本>/` 是全量镜像，不参与清单，
  `.dmg` 与 `.sig` 在这里都有，GitHub 不可达时人工也能取到完整一套。

### 固定键的代价：下载名 + Content-Disposition

对象键被改写成「平台名」，浏览器/下载器就会拿键的最后一段当保存名 —— 人从直链下载
会得到 `windows-x64-setup.exe`：没有版本号，也认不出是谁的包。所以每个对象上传时
都要带 `Content-Disposition: attachment; filename="<原始资产名>"`，下载保存名才是
`DSH.Starter_0.1.6_x64-setup.exe`。

- 这个头**更新器不看**：它按文件头魔数判 exe/msi/app.tar.gz，落盘用自己的临时名
  （`DSH Starter-<版本>-installer.exe`）。所以它对自动更新零影响，纯粹为人服务。
- `scripts/r2-manifest.mjs --list-files` 的 TSV 第三列就是算好的这个头，CI 直接透传；
  手写命令时别忘了它，否则文件名又会「丢」。
- 更新一次**已发布版本**的对象元数据（字节不变）时，路径没变、CDN 边缘缓存里的那份
  旧响应也就没变，必须换个查询串才会重新回源：

  ```bash
  node scripts/r2-manifest.mjs --in latest.json --assets-map assets.json \
    --base "$R2_PUBLIC_BASE" --fingerprint "0.1.6-r2" --out latest.json
  ```

  `--fingerprint` 只在这种「原地改元数据」的场合用；正常发版**不要**传，默认就是版本号。
  下一轮真正的发版会自然把指纹换成新版本号，不需要再收尾。0.1.6 现在用的就是
  `?v=0.1.6-r2`（补 `Content-Disposition` 那次留下的）。

### 必须记住的两条

- **`latest.json` 要随每次发布更新**：更新器按顺序取第一个能解析的清单，R2 上的清单
  陈旧会让客户端停在旧版本（届时用户可在设置里切到 GitHub 源自救）。
- **对象可以长缓存，但只在带 `?v=<版本>` 指纹的前提下**：固定路径的内容每次发布都变，
  指纹让 CDN 每个版本看到一个「新对象」，所以 `immutable` 是安全的、下载也快；
  **`latest.json` 绝不能长缓存**（它必须每次回源），CI 固定写成 `no-cache, max-age=0`。
  改动这两处缓存策略前先想清楚这一点 —— 这是这套方案唯一容易踩的坑。

### 手动补传（等价于 CI 那几步）

首选其实是**手动触发 workflow 勾 `force_r2`**（不重新打包，认领已发布的 v<当前版本> 只做
R2 全量补传）。要在本机跑的话：

```bash
export GITHUB_REPOSITORY=Yinxe/dsh-starter
export R2_PUBLIC_BASE=https://pub-576ca711d9cf4cfe96b58195dfe6ce81.r2.dev
# 按 tag 现查 releaseId 并下载**全部**资产 + 改写清单 + 算好 upload.tsv
# （鉴权走 `gh auth token`；别用 gh release download —— 它读滞后的 tags 接口，见上文）
node scripts/sync-r2.mjs --dir r2-upload --version X.Y.Z
# upload.tsv: 本地文件 <TAB> 对象键（完整路径，latest/… 与 releases/v…/…） <TAB> Content-Disposition
while IFS=$'\t' read -r local key cd; do
  args=(--content-type application/octet-stream --cache-control "public, max-age=31536000, immutable")
  [ "$cd" != "-" ] && args+=(--content-disposition "$cd")
  npx wrangler r2 object put "dsh-starter-release/$key" --file "r2-upload/$local" "${args[@]}" --remote
done < r2-upload/upload.tsv
npx wrangler r2 object put dsh-starter-release/latest.json --file r2-upload/latest.json \
  --content-type application/json --cache-control "no-cache, max-age=0" --remote
node scripts/verify-r2.mjs --dir r2-upload --version X.Y.Z   # 自检，等价 CI 最后一步
```

### 其它

- 想换自定义域名：改「一次性准备」表格里的两处基址即可。`r2.dev` 是 Cloudflare 的托管
  开发域名、有速率限制，流量大了建议绑自定义域名。
- 早期按版本号命名的对象（`DSH.Starter_0.1.5_*.exe`）已删除；固定键方案下不会再产生，
  版本信息改由 `Content-Disposition` 和清单里的 `?v=` 承载。

## 8. 下载页（GitHub Pages）

站点源码在 `site/`，由 `.github/workflows/pages.yml` 部署到
<https://yinxe.github.io/dsh-starter/>（push 到 `main` 且改动 `site/**` 时才跑）。

**它与发版是解耦的**：页面在浏览器里现拉自建源清单与 GitHub Release API 得到版本号、体积与直链，
所以发新版**不需要重新构建页面**，pages 作业也不必等 release 作业。

两条与发布流程相关的边界：

- **桶没配 CORS**，浏览器读不到 `latest.json` 的内容（只探得到「可达」）。这不影响下载，也不影响
  启动器（它走 Rust HTTP 客户端），页面因此改用 GitHub Release 的正文与体积。若给桶配上 CORS
  （允许 `https://yinxe.github.io` 的 GET/HEAD），把 `site/src/lib/release.ts` 的
  `R2_CORS_ENABLED` 改成 `true`，页面就会改读自建源清单并核对两条源是否同步。**没配 CORS 时不要
  打开这个常量** —— 那样每次加载页面都会在控制台留下一条 CORS 报错。
- **`.dmg` 不在更新清单里**：更新器只认 `latest.json` 里的固定键（`STABLE_KEYS`），DMG 供首次
  手动下载。0.3.1 起全量镜像会把 DMG 也传到 `<基址>/releases/v<版本>/`（按原始文件名直取），
  但它仍不进清单、不占固定键；下载页在自建源模式下依旧把 DMG 那行指向 GitHub，
  要改就得给 `scripts/r2-manifest.mjs` 补一个固定的 dmg 键（当前未做）。

首次部署需要在仓库 **Settings → Pages** 把 Source 选成 *GitHub Actions*；工作流里的
`configure-pages@v6` 带了 `enablement: true`，通常会自动打开。

## 9. 常见故障

| 现象 | 原因 / 处理 |
| --- | --- |
| CI 失败：`版本号不一致` | 三个版本文件没改齐，按提示逐个改 |
| CI 失败：`CHANGELOG.md 里没有 vX.Y.Z 的段落` | 先写 CHANGELOG（含 `## [X.Y.Z] - 日期`），再 push |
| CI 失败：`段落没有任何条目` | 只写了标题没写内容，至少补一条 `- ` |
| 客户端收不到更新 | Release 不能是草稿/pre-release；`releases/latest` 只认正式版本；确认 `latest.json` 能下载 |
| `gh release download` 报 no assets，但 Release 页面明明有资产 | `releases/tags/*` 在草稿转正式后返回**滞后的读副本**（实测 1h+ 仍报 0 资产），`gh release download/view` 全走它。别等它自愈：用 `scripts/sync-r2.mjs`（按 releaseId 走 `/releases/<id>/assets`），或勾 `force_r2` 手动触发 workflow |
| 发布后发现说明写错 | 直接编辑 GitHub Release 正文 + 修 `CHANGELOG.md`（`latest.json` 里的 notes 已经下发，改不了，除非重发/热修版本） |
| 下载页版本号一直是旧的 | 页面不缓存清单，先硬刷新；仍不对就查 `latest.json` 与 GitHub Release 是否有一边没更新（Vite 产物本身有 hash，不是页面缓存问题） |
| 下载页显示「自建源不可达」 | R2 桶被删/改名/桶名权限变了；自建源的固定键直链同时也会失效，需要按第 7 节重新上传 |
