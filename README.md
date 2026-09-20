# DSH Launcher

跨平台的 [@deepseek-ai/dsh](https://www.npmjs.com/package/@deepseek-ai/dsh)（DeepSeek Harness CLI）启动器。

技术栈：**Tauri 2**（Rust 系统层 + 托盘/窗口）+ **React 18 + TypeScript**（界面）+ **Node.js / npm**（构建工具链，dsh 本身也是 Node 程序）。

![DSH Launcher「版本与安装」界面（深色主题）](images/preview.png)

## 功能

- **官方版本列表**：直接从 npm registry 拉取 `@deepseek-ai/dsh` 已发布的全部版本（含 `latest` / `next` / `alpha` dist-tags、发布日期、体积），支持切换 registry 镜像。
- **识别已安装版本**：三个来源自动合并识别——
  - 本启动器管理的 `~/.dsh-launcher/versions/`；
  - npm 全局安装（`npm root -g`）；
  - PATH 中的 `dsh` 可执行文件（顺着符号链接解析版本）。
- **安装 / 卸载 / 重装**：未安装的版本由启动器用 npm 装进自己的数据目录（互不污染全局环境），`--loglevel info` 把每个包的真实 fetch/resolve/extract 过程实时显示在安装卡里，可取消；卸载即删除对应目录。
- **内嵌启动（默认）**：「启动」把 dsh 作为启动器的**子进程**运行——日志实时显示在底部进程面板（多实例 tab、运行时长、停止按钮），**退出启动器即结束所有 dsh 进程**；也可点「终端」在独立系统终端中启动（交互式 TUI 场景，不受启动器生命周期管理）。**每个 profile 同时只能运行一个实例**（内嵌与终端启动都受约束），Profile 选择器默认选中 `web`（不存在则取第一个），不提供“默认 profile”空选项。
- **Node 运行时预装**：宿主机没有 Node/npm 时，一键下载 Node LTS 安装到 `~/.dsh-launcher/runtime/`（用户级、无需 root、不污染系统），下载默认走 npmmirror 镜像站（设置中可换 aliyun 等），npm registry 在设置中配置。
- **选择启动**：任意已安装版本一键在**新的系统终端窗口**中启动对应版本的 `dsh`（用绝对 node + bin.js 启动，不依赖 PATH）；支持 **profile**：顶栏下拉框列出 `$DSH_HOME/profiles`（缺省 `~/.dsh/profiles`，兼容单数 `profile/`）下的所有 profile（自动跳过 `node_modules`），选中即以 `dsh --profile <名字>` 启动并可保存为默认，留空则不带 `--profile` 走 dsh 默认。可设置默认附加参数（如 `--preset qqbot`）。
- **插件管理**：按 profile 管理 dsh bundle 插件——列出已装插件与启用状态并可一键启停（启停按包内真实插件 ID 写入 cordis.patch 层），也可直接编辑 profile 侧的配置文件并写回。
  - **全部走官方 `dsh plugin` 命令**：安装 = `dsh plugin --profile <p> add <spec>`，卸载 = `remove`，升级同样回到 `add`（git 克隆源则先 `git pull` 再重新 `link:` 安装），启动器从不直接改写 `package.json` / `dsh.profile.bundles`。
  - **内置终端**：插件页顶部常驻可折叠终端，安装 / 卸载 / 升级 / 克隆的 stdout+stderr **逐行实时**输出（每行按流着色、自动滚底、可复制 / 导出到 `~/.dsh-launcher/logs/`）；每个任务一个标签页，运行中可一键取消（取消会杀掉整棵进程组，pnpm/node 派生的孙进程不会残留），窗口重挂载后历史仍在。同一 profile 同时只允许一个插件任务，避免并发写坏 `node_modules`。
  - **四种安装方式**：① npm 包（registry 搜索 / 精确规格）② GitHub 仓库 ③ 链接安装（本地 `link:` 路径 / `.tgz` 直链）④ clone 仓库 + 本地 link（克隆到 `~/.dsh-launcher/git-plugins/<owner>-<repo>`，可选自动 `pnpm install` + `pnpm run build`）。
  - **仓库探测（monorepo 友好，且不吃 GitHub 额度）**：先用 `git ls-remote` 拿目标 ref 的 HEAD sha，再用 jsDelivr（`data.jsdelivr.com` 取整棵文件树 + `cdn.jsdelivr.net` 取各 `package.json`）**按该 sha** 探测——两者都不消耗 GitHub API 额度，也不会因为匿名 60 次/小时 的限制而失效（GitHub 的 git 协议对私有与不存在的仓库都要求凭据，因此这两种情况直接前置拒绝，不会在终端里弹登录）。自动识别 `pnpm-workspace.yaml` / `package.json workspaces`，把 **workspace 子包插件**逐个列出（名称 / 版本 / 描述 / 是否声明 `dsh.bundle` / `lib/` 构建产物是否就绪），可多选一次装多个；候选 `package.json` 并发抓取（4 路）并对 CDN 限流做退避重试。`api.github.com` 只用于元数据增强（stars / license / 描述）：额度用尽时标记「元数据受限」并照常给出候选，绝不阻塞安装；探测通道不可用时才逐级降级为 REST 文件树 → 单目录 contents 探测。本地目录（含克隆出来的仓库）用同一套规则探测。
  - **安装 / 卸载的前后置校验**（细节对齐同生态的 dshmarket）：装完立刻核对新增的包——缺 `dsh` 清单、或没有可加载入口（源码检出、构建被 `allowBuilds` 拦住）会在**下次启动**把整个 profile 拖挂，因此当场卸掉并说明原因；新增包与被装插件的 loader **entry id 冲突**（cordis 不允许同 id 两个 insert，装错会让 dsh 起不来）也会被检出并卸掉。卸载前先查用户自己的 `cordis.patch.yml` 是否仍引用该包（引用则拒绝并指出要删哪几行，启动器不改写你的补丁文件）、是否带原生模块（`.node` 在进程退出前不会释放，需重启才能重装）；卸载后以**磁盘事实**对账：包没了却还在 `package.json` 里留着依赖/bundle 行的，删掉残留行（原件备份为 `package.json.launcher-bak`），包还在的就保留行并提示重试。升级后还会比对实际安装版本——pnpm 的 `minimumReleaseAge` 会**静默保留旧版本并退出 0**，版本没变会明确告警而不是报成功。pnpm 失败按 dshmarket 的清单一次性恢复：新版本等待期放行（`--config.minimum-release-age=0`，短横线拼写，camelCase 在 pnpm ≥12.3 会被静默忽略）、网络抖动重跑、下载超时加长、宿主 peer 关闭自动安装、node_modules 布局/store 不匹配先重建——全部仍经由 `dsh plugin`。
  - **更新检测（同样零 API 额度）**：npm 包比对 registry `latest`；`github:` 规格与 clone+link 源都走 `git ls-remote` 比对提交（同一仓库的多个插件共用一次查询，结果缓存 60s；实测真实 profile 的 16 个依赖 ≈13s、GitHub API 调用 **0 次**），clone+link 源支持一键 `git pull` 升级；远端不可匿名访问（私有仓库）时明确标注 `私有仓库 · 不支持` 并提示在本地仓库手动 `git pull`；纯本地 link 与 `.tgz` 直链标注「无更新渠道」，只能手动重装。
- **模型配置**：参考 dsh 官方 provider 配置布局，结构化编辑全局配置的模型两节——`llm-pi-ai.providers`（API 密钥 / 显示名称 / API 地址 / API 协议 / 模型列表）与 `agent-default-model`（默认模型三级联动：Provider → 模型 → 思考等级，不选则保持默认）。API 密钥可下拉选择已有凭据或手动输入（手动输入的密钥保存时自动回存到「凭据管理」）；支持「获取可用模型」——从服务方 `GET {baseURL}/models` 拉取列表勾选添加（openai / anthropic 协议，密钥按 手动值 > 凭据 refs > 环境变量 解析）；模型的思考等级（reasoningEfforts）可补充映射，不填则不写入。保存只重写这两节，`settings.yaml` 其余内容与节外注释逐字节保留，写前自动备份，未识别字段原样透传。
- **通道自检与自适应选路**：设置页可一键并发探测四条通道（`github.com` refs / jsDelivr / raw / `api.github.com`）的可达性与往返延迟，直接看清当时哪条通、多快。选路不写死顺序——每条通道记录最近成功延迟（EWMA）与连续失败次数：延迟相近时优先**免额度**的 jsDelivr；`api` 快一倍以上（本机实测 0.6s vs 8s）就先走 api；任一通道连续失败 2 次会被**临时跳过 5 分钟**，到期自动半开，避免每次都白等一个超时。探测顺序也据此调整：先取元数据（0.5s 级，顺带拿到默认分支名）→ 再按规则取文件树，`github.com` 的 refs 解析退为兜底，不再挡在关键路径最前面；REST 文件树被 `truncated` 截断时如实标记（大仓库实测只剩 5 个候选，jsDelivr 给 10 个），不把"候选变少"伪装成完整结果。
- **GitHub Token（可选）**：设置里可填一个 token（或直接用 `GITHUB_TOKEN` / `GH_TOKEN` 环境变量），把 `api.github.com` 额度从匿名 60 次/小时 提到 5000 次/小时。除了 stars / license 这类元数据增强，启动器的探测与更新检测本来就走免额度通道，不填也完全可用；设置页会显示当前额度与重置时间。
- **任务栏 / 托盘图标**：常驻托盘，左键切换窗口；右键菜单为「打开主界面」+ 各 profile 的运行状态（● 运行中 / ○ 未运行，纯展示不可点击）+ 退出；点关闭默认最小化到托盘（可在设置中改为直接退出）。
- **启动器自更新**：
  - 支持自建**更新清单**（设置里填一个返回 `{ "version": "x.y.z", "notes": "...", "url": "https://..." }` 的 JSON 地址），启动时自动检查，发现新版本弹横幅跳转下载；
  - 同时内置 **Tauri updater**（签名自动更新）：发布时在 `src-tauri/tauri.conf.json` 的 `plugins.updater` 里配好 `pubkey` 和 `endpoints`，即可在应用内一键"下载并安装 + 重启"。

## 数据目录

```
~/.dsh-launcher/       # 启动器自身数据
├── settings.json      # 启动器设置
├── runtime/           # 内置 Node 运行时（一键预装）
│   └── node-v22.14.0/
├── logs/              # 插件任务日志导出（内置终端「导出」按钮）
├── git-plugins/       # clone+link 安装的本地仓库（git pull 更新）
│   └── owner-repo/
└── versions/
    └── 0.1.5-rc.2/    # 每个版本一个 npm prefix
        └── node_modules/@deepseek-ai/dsh/

~/.dsh/                # dsh 自身数据（$DSH_HOME，可被环境变量覆盖）
└── profiles/          # dsh 的可启动 profile（子目录；兼容旧版单数 profile/）
    ├── web/
    ├── headless/
    └── ci.yaml        # → dsh --profile ci
```

## 本地开发

前置：Node.js ≥ 18、npm、Rust 工具链（rustup），Linux 需要 WebKit/GTK 系统依赖：

```sh
# Debian/Ubuntu
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev

# Arch
sudo pacman -S webkit2gtk-4.1 base-devel libxdo libayatana-appindicator librsvg

# Fedora
sudo dnf install webkit2gtk4.1-devel libappindicator-gtk3-devel librsvg2-devel
```

运行 / 打包（统一走 Tauri CLI）：

```sh
npm install
npm run app            # 开发模式（tauri dev，vite 热更新）
npm run build:debug    # 调试版二进制（内嵌前端，不产安装包）
npm run build:release  # 正式打包：产出当前平台支持的全部安装包
npm run release        # 同上（别名）
```

`bundle.targets` 为 `"all"`，即打包**当前平台支持的全部格式**：

| 平台 | 产物 |
| --- | --- |
| Linux | `.deb` / `.rpm` / `.AppImage` |
| Windows | NSIS `.exe` 安装包 / `.msi` |
| macOS | `.app` / `.dmg`（配 `--target universal-apple-darwin` 出 Intel + Apple Silicon 通用包） |

产物落在 `src-tauri/target/release/bundle/`。只想出其中几种就覆盖 `--bundles`，例如
`npm run build:release -- --bundles deb,appimage`；只要裸二进制（不打包）加 `--no-bundle`。

前端单独构建检查：`npm run build`（tsc + vite）。

> Linux 编译与运行都依赖上面安装的 WebKit/GTK 系统库；cargo 依赖下载已配置国内镜像（`src-tauri/.cargo/config.toml`，只影响本项目）。
> Tauri 2 的 rpm 打包是**纯 Rust** 实现（内置 `rpm` crate），不需要系统安装 `rpm` / `rpmbuild`，Debian/Ubuntu 上可直接产出 `.rpm`。

## 发布新版本（启动器自身）

1. 改 `src-tauri/tauri.conf.json` 的 `version` 与 `src-tauri/Cargo.toml` 的 `version`；
2. 在 `src-tauri/tauri.conf.json` 的 `plugins.updater` 里配好 `pubkey` / `endpoints`，并把 `bundle.createUpdaterArtifacts` 改回 `true`，然后设置 `TAURI_SIGNING_PRIVATE_KEY` 环境变量；
3. `npm run build:release` —— 产出安装包，`createUpdaterArtifacts` 会同时生成 updater 签名产物；
4. 把安装包和 `latest.json`（Tauri updater 格式）发布到你的下载源，并同步更新自建清单 JSON 或 `plugins.updater.endpoints`。

> **三端全格式交给 CI**：`.github/workflows/release.yml` 基于 Tauri [官方模板](https://v2.tauri.app/distribute/pipelines/github) + `tauri-apps/tauri-action@v1`——打标签、建 Draft Release、上传产物全部由该 action 完成，workflow 里没有任何自定义版本脚本。
> 触发方式：手动（`workflow_dispatch`）或 push 到 `release` 分支。版本号**可选**：填了走官方 `tauri build --config` 覆盖（不修改任何文件），留空则用 `tauri.conf.json` 里的版本。
> matrix 覆盖 ubuntu-22.04（deb/rpm/AppImage）、windows-latest（.exe/.msi）、macos-latest（universal .dmg）。
> 跨平台产物无法在单机上交叉编译，Windows / macOS 安装包必须由对应 runner 产出。

## 已知边界

- dsh 是交互式 CLI，启动器会在**系统终端**里拉起它（自动探测 gnome-terminal / konsole / alacritty / kitty / Terminal.app / Windows Terminal 等），无终端可用时界面会展示等效命令供手动粘贴。
- 全局 / PATH 安装的 dsh 只做识别与启动，不会代为卸载（避免误删用户环境）。
- Linux 托盘需要 `libayatana-appindicator3`（运行时依赖，打包的 deb 已声明系统会自带或按提示安装）。
