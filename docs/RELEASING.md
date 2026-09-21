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

针对第一段，已经加了缓存：

- `Swatinem/rust-cache@v2`，`workspaces: src-tauri -> target`（Cargo.toml 在子目录里，
  用默认值会找不到 target、等于没开），缓存 registry + `target/`；
- `setup-node` 的 `cache: npm`；
- `.github/workflows/cache-warm.yml`：每周一跑一次，**只恢复、不保存**（`save-if: false`），
  专门刷新 GitHub「7 天不访问即清」的 TTL。为什么不能让它保存：`shared-key` 与发版作业
  同名，一旦让这个不编译的作业写缓存，就会把一份空 target 写进同一个 key —— 而缓存条目
  不可覆盖，发版那次反而只能拿到空缓存（污染）。恢复命中同样算「访问」，够用了。

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
`R2_BASE`），GitHub 源保留为兜底；用户可在设置里二选一。

实测（国内直连、不走代理）拉 81MB 的 AppImage：**GitHub 的 release 下载直接连不上**
（curl 返回 000），自建源 200、约 **3.3MB/s**（首字节 ~0.9s）。走代理时两者都受同一条
隧道限速（~0.8–1.6MB/s），差距不明显 —— 所以自建源的价值首先是**可用性**，
其次才是不受 GitHub 速率限制。

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

   没配也能正常发版：`publish-r2` 作业会自动跳过并打一条 **warning**（不是静默的 notice ——
   客户端默认走 R2，这里跳过就意味着 R2 上的清单会停在旧版本，得照本节末尾手动补传）。

### 每次发布自动发生什么

`publish-r2` 作业（`needs: publish`，**只在这一轮真的发布了新版本时才跑**；版本号没变就推 main
属于空跑，整个作业会跟着跳过，不会白下载白上传 118MB）：

1. `gh release download` 拉本次发布的全部资产；
2. `node scripts/r2-manifest.mjs` 把 `latest.json` 里的下载地址从 GitHub 资产 API
   （`api.github.com/repos/…/releases/assets/<id>`）改写成**固定键**
   `<R2_PUBLIC_BASE>/latest/<平台>.扩展名?v=<版本>`，**签名原样保留**（安装包字节没变，
   客户端照常验签），并校验清单引用的文件都在本地；
3. `aws s3 cp` 按 `upload.tsv`（`<本地文件> <TAB> <R2 键> <TAB> Content-Disposition>`）
   逐行覆盖上传：对象用 `immutable` 长缓存 + `?v=<版本>` 指纹，**同时带上
   `Content-Disposition: attachment; filename="<原始资产名>"`**（见下节）；
   最后用 `no-cache` 覆盖 `latest.json`；
4. 自检：从公网取回清单，确认每个平台地址都落在 `<基址>/latest/` 下，再逐个 HEAD，
   确认 200 且 `Content-Disposition` 里确实带着原始文件名（少一个就红）。

### 桶里的布局（固定键，只有一份 latest）

```
dsh-luncher-release/
├── latest.json                        # 更新清单：每次覆盖（no-cache）
└── latest/                            # 固定键（immutable + ?v=<版本> 指纹）
    ├── windows-x64-setup.exe          # NSIS（windows-x86_64-nsis）→ 下载名 DSH.Launcher_<版本>_x64-setup.exe
    ├── windows-x64.msi                # MSI（windows-x86_64 / -msi）→ 下载名 DSH.Launcher_<版本>_x64_en-US.msi
    ├── darwin-universal.app.tar.gz    # macOS 三架构共用 universal 包
    ├── linux-x86_64.AppImage
    ├── linux-x86_64.deb
    └── linux-x86_64.rpm
```

「键名」和「下载名」是两回事：键为了地址稳定、永远不变；下载名由 `Content-Disposition`
给出，永远带版本号和产品名（见上一节）。

- 地址**不带版本号**：每次发布覆盖同名对象，桶里永远只有一份「当前最新」，
  旧版本自然消失、**不需要任何清理步骤**，占用稳定在约 118 MB；
- 顺带的好处：`<基址>/latest/windows-x64-setup.exe` 可以直接当「永久最新版下载链接」分享；
- 只上传清单真正引用的包（AppImage / deb / rpm / app.tar.gz / msi / setup.exe）；
  `.dmg` 不在更新清单里（它供首次下载），要镜像的话另外传。

### 固定键的代价：下载名 + Content-Disposition

对象键被改写成「平台名」，浏览器/下载器就会拿键的最后一段当保存名 —— 人从直链下载
会得到 `windows-x64-setup.exe`：没有版本号，也认不出是谁的包。所以每个对象上传时
都要带 `Content-Disposition: attachment; filename="<原始资产名>"`，下载保存名才是
`DSH.Launcher_0.1.6_x64-setup.exe`。

- 这个头**更新器不看**：它按文件头魔数判 exe/msi/app.tar.gz，落盘用自己的临时名
  （`DSH Launcher-<版本>-installer.exe`）。所以它对自动更新零影响，纯粹为人服务。
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

```bash
gh release download vX.Y.Z --dir r2-upload --clobber
gh release view vX.Y.Z --json assets > r2-upload/assets.json
node scripts/r2-manifest.mjs --in r2-upload/latest.json --assets-map r2-upload/assets.json \
  --check-dir r2-upload --base "$R2_PUBLIC_BASE" --list-files r2-upload/upload.tsv \
  --out r2-upload/latest.json
# upload.tsv 每行是「本地文件 <TAB> R2 键 <TAB> Content-Disposition」；
# 用本机 OAuth 登录态即可，无需 S3 凭据：
while IFS=$'\t' read -r local key disposition; do
  npx wrangler r2 object put "dsh-luncher-release/latest/$key" --file "r2-upload/$local" \
    --content-type application/octet-stream --content-disposition "$disposition" \
    --cache-control "public, max-age=31536000, immutable" --remote
done < r2-upload/upload.tsv
npx wrangler r2 object put dsh-luncher-release/latest.json --file r2-upload/latest.json \
  --content-type application/json --cache-control "no-cache, max-age=0" --remote
```

### 其它

- 想换自定义域名：改「一次性准备」表格里的两处基址即可。`r2.dev` 是 Cloudflare 的托管
  开发域名、有速率限制，流量大了建议绑自定义域名。
- 早期按版本号命名的对象（`DSH.Launcher_0.1.5_*.exe`）已删除；固定键方案下不会再产生，
  版本信息改由 `Content-Disposition` 和清单里的 `?v=` 承载。

## 8. 常见故障

| 现象 | 原因 / 处理 |
| --- | --- |
| CI 失败：`版本号不一致` | 三个版本文件没改齐，按提示逐个改 |
| CI 失败：`CHANGELOG.md 里没有 vX.Y.Z 的段落` | 先写 CHANGELOG（含 `## [X.Y.Z] - 日期`），再 push |
| CI 失败：`段落没有任何条目` | 只写了标题没写内容，至少补一条 `- ` |
| 客户端收不到更新 | Release 不能是草稿/pre-release；`releases/latest` 只认正式版本；确认 `latest.json` 能下载 |
| 发布后发现说明写错 | 直接编辑 GitHub Release 正文 + 修 `CHANGELOG.md`（`latest.json` 里的 notes 已经下发，改不了，除非重发/热修版本） |
