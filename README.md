# DSH Launcher

跨平台的 [@deepseek-ai/dsh](https://www.npmjs.com/package/@deepseek-ai/dsh)（DeepSeek Harness CLI）启动器。

技术栈：**Tauri 2**（Rust 系统层 + 托盘/窗口）+ **React 18 + TypeScript**（界面）+ **Node.js / npm**（构建工具链，dsh 本身也是 Node 程序）。

## 功能

- **官方版本列表**：直接从 npm registry 拉取 `@deepseek-ai/dsh` 已发布的全部版本（含 `latest` / `next` / `alpha` dist-tags、发布日期、体积），支持切换 registry 镜像。
- **识别已安装版本**：三个来源自动合并识别——
  - 本启动器管理的 `~/.dsh-launcher/versions/`；
  - npm 全局安装（`npm root -g`）；
  - PATH 中的 `dsh` 可执行文件（顺着符号链接解析版本）。
- **安装 / 卸载 / 重装**：未安装的版本由启动器用 npm 装进自己的数据目录（互不污染全局环境），`--loglevel info` 把每个包的真实 fetch/resolve/extract 过程实时显示在安装卡里，可取消；卸载即删除对应目录。
- **内嵌启动（默认）**：「启动」把 dsh 作为启动器的**子进程**运行——日志实时显示在底部进程面板（多实例 tab、运行时长、停止按钮），**退出启动器即结束所有 dsh 进程**；也可点「终端」在独立系统终端中启动（交互式 TUI 场景，不受启动器生命周期管理）。
- **Node 运行时预装**：宿主机没有 Node/npm 时，一键下载 Node LTS 安装到 `~/.dsh-launcher/runtime/`（用户级、无需 root、不污染系统），下载默认走 npmmirror 镜像站（设置中可换 aliyun 等），npm registry 在设置中配置。
- **选择启动**：任意已安装版本一键在**新的系统终端窗口**中启动对应版本的 `dsh`（用绝对 node + bin.js 启动，不依赖 PATH）；支持 **profile**：顶栏下拉框列出 `$DSH_HOME/profiles`（缺省 `~/.dsh/profiles`，兼容单数 `profile/`）下的所有 profile（自动跳过 `node_modules`），选中即以 `dsh --profile <名字>` 启动并可保存为默认，留空则不带 `--profile` 走 dsh 默认；托盘菜单也提供"启动最新 dsh"。可设置默认附加参数（如 `--preset qqbot`）。
- **任务栏 / 托盘图标**：常驻托盘，左键切换窗口，右键菜单（显示窗口 / 启动最新 dsh / 退出）；点关闭默认最小化到托盘（可在设置中改为直接退出）。
- **启动器自更新**：
  - 支持自建**更新清单**（设置里填一个返回 `{ "version": "x.y.z", "notes": "...", "url": "https://..." }` 的 JSON 地址），启动时自动检查，发现新版本弹横幅跳转下载；
  - 同时内置 **Tauri updater**（签名自动更新）：发布时在 `src-tauri/tauri.conf.json` 的 `plugins.updater` 里配好 `pubkey` 和 `endpoints`，即可在应用内一键"下载并安装 + 重启"。

## 数据目录

```
~/.dsh-launcher/       # 启动器自身数据
├── settings.json      # 启动器设置
├── runtime/           # 内置 Node 运行时（一键预装）
│   └── node-v22.14.0/
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

运行 / 打包：

```sh
npm install
npm run app            # 开发模式（vite 热更新）
npm run build:debug    # 无 root 快速编译（调试版，需配合 dev 服务器看 UI）
npm run build:release  # 自包含正式版（内嵌前端，--features custom-protocol）
npm run start          # 启动桌面应用（release 二进制不存在时自动先构建）
npm run tauri build    # 产出 deb / AppImage / dmg / nsis 安装包（按当前平台）
```

前端单独构建检查：`npm run build`（tsc + vite）。

### 没有 sudo？本仓库自带两套无 root 方案（Debian 13 验证通过）

1. **cargo 国内镜像**：`src-tauri/.cargo/config.toml` 已配置 rsproxy 镜像（只影响本项目，删掉该文件即恢复官方源）。
2. **WebKit/GTK 开发库 sysroot**：`.sysroot/` 内已用 `apt-get download` + `dpkg -x` 解压好 webkit2gtk-4.1 相关库（无需 root）。没有系统依赖时这样编译：

```sh
cd src-tauri
export PKG_CONFIG_PATH="$PWD/.sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:$PWD/.sysroot/usr/share/pkgconfig"
RUSTFLAGS="-L native=$PWD/.sysroot/usr/lib/x86_64-linux-gnu" cargo build
LD_LIBRARY_PATH="$PWD/.sysroot/usr/lib/x86_64-linux-gnu" ./target/debug/dsh-launcher
```

（正式打包 deb/AppImage 时仍建议 `sudo apt install` 安装系统依赖后用 `npm run tauri build`。）

## 发布新版本（启动器自身）

1. 改 `src-tauri/tauri.conf.json` 的 `version` 与 `src-tauri/Cargo.toml` 的 `version`；
2. `npm run tauri build` —— `createUpdaterArtifacts` 会同时生成 updater 签名产物；
3. 把安装包和 `latest.json`（Tauri updater 格式）发布到你的下载源，并同步更新自建清单 JSON 或 `plugins.updater.endpoints`。

## 已知边界

- dsh 是交互式 CLI，启动器会在**系统终端**里拉起它（自动探测 gnome-terminal / konsole / alacritty / kitty / Terminal.app / Windows Terminal 等），无终端可用时界面会展示等效命令供手动粘贴。
- 全局 / PATH 安装的 dsh 只做识别与启动，不会代为卸载（避免误删用户环境）。
- Linux 托盘需要 `libayatana-appindicator3`（运行时依赖，打包的 deb 已声明系统会自带或按提示安装）。
