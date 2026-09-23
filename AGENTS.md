# DSH Starter — 开发规范（Agent 规则）

面向在本仓库工作的 AI Agent 与协作者。**这里的规则是硬性的**：违反其中任何一条，构建/CI 会失败或用户会直接看到问题。

## 项目速览

- **技术栈**：Tauri 2（Rust 系统层 + 托盘/窗口/子进程）+ React 18 + TypeScript + Vite（界面）+ shadcn/ui + Tailwind v4。
- **Rust 侧**（`src-tauri/src/`）：`lib.rs` 注册插件与命令；`commands.rs` 是前端能调用的全部命令；
  其余按领域分文件（`procs` 进程与实例、`profiles`/`profile_cfg` profile、`plugin`/`pnpm` 插件、
  `installed`/`installer`/`registry` 版本与安装、`credentials`/`modelcfg` 配置与凭据、`tray` 托盘、
  `update_check` 自更新、`netports` 端口归属）。改了命令签名必须同步 `lib.rs` 的 `generate_handler!`。
- **前端**（`src/`）：`App.tsx` 主界面与状态、`api.ts` 所有 `invoke` 封装、`types.ts` 与 Rust 结构体一一对应的类型、
  `components/` 业务组件、`components/ui/` shadcn 组件、`lib/` 主题等基础库。
  「Profiles」页是**左右双栏工作台**：左列实例列表（`App.tsx` 的 `selectedProfile`），右列
  `components/ProfileWorkspace.tsx`（Tabs：`ProfileQuickTab` / `ModelConfigView` / `PluginsTab` /
  `ProfileFilesTab` / package.json 只读）；插件、模型、配置文件**不再是独立页面**，全部按「先定 profile 再配置」组织。
- **一切实时终端输出统一进右侧「通用终端面板」**（`components/TerminalPanel.tsx` +
  `components/terminal/{InstanceTaskView,PluginTaskView,SystemTaskView}.tsx`）：实例日志、插件任务、dsh 版本安装、
  Node 安装四类任务按 `TerminalTaskRef {kind,id}` 寻址，选择状态是 `App.tsx` 的 `terminalTask`（别在面板内部再存一份）；
  任务数据源是挂在 App 层的 `hooks/use-plugin-jobs.ts` 与 `hooks/use-terminal-jobs.ts`（事件流→缓冲聚合，
  80ms flush + alive 守卫防 StrictMode 双订阅），**页面里不要再嵌自己的输出块**——需要状态就用
  `pluginRunningCount`/`pluginJobsTick` 这类 props 或回调。宽屏（≥1280px）为与内容并排的右侧常驻栏，小屏自动切为侧边抽屉（`hooks/use-terminal-host.ts`）。
- **配置归属（0.1.7 硬约定）**：≥0.1.7 的 profile 配置在其 `profiles/<p>/cordis.patch.yml`，<0.1.7 走全局
  `settings.yaml`（迁移后被 dsh 改名 `settings.yaml.imported`，启动旧版前由 `restore_legacy_settings()` 还原）。
  启动器写 patch 一律**行级 marker 注释块整块接管**（`profile_cfg.rs` 的 `MODELCFG_MARKER = "# dsh-starter: modelcfg"`，
  快捷配置同理由 `# dsh-starter: web-quick` 接管），**绝不整文件 parse→serialize**：patch 含 `!!js` 自定义 tag，
  round-trip 必坏数据；写后 `validate_yaml` + 原子写 + `*.starter-bak` 备份。`modelcfg.rs` 的
  `read_for_profile/write_for_profile` 是唯一入口，前端别绕过 `get/set_model_config` 直接改 patch。
- **下载页**（`site/`）：独立的 Vite + React + shadcn/ui + Tailwind 站点（GitHub Pages），**不参与主程序构建**。
  `lib/release.ts` 是唯一的数据层（平台清单、两条源的直链规则、R2/REPO 常量），页面上的版本号与体积都是
  打开时现拉的；`components/site/` 业务区块、`components/ui/` 同为 shadcn 组件。改平台清单或地址规则只改这里
  —— 与 `src-tauri/src/update_check.rs` 的 `R2_BASE`、`scripts/r2-manifest.mjs` 的固定键表必须保持一致。
  **换桶时四处同改**：两处 `R2_BASE`（客户端 + 下载页）与 `release.yml` 的 `R2_BUCKET` / `R2_PUBLIC_BASE`；
  只改一半会让客户端读到旧桶那份「陈旧但有效」的清单，判定「已是最新」而**静默卡死更新**（见 `docs/RELEASING.md`）。
- **文档**：`README.md`（面向用户的能力与配置）、`CHANGELOG.md`（**发布说明唯一来源**）、
  `docs/RELEASING.md`（发版手册）、`docs/releases/`（旧版说明归档，生成物）。

## UI 规则（硬性）

- **所有 UI 统一使用 shadcn/ui 全套组件与风格**：优先复用 `src/components/ui/` 下已有的组件（Button、Card、Badge、Dialog、Sheet、AlertDialog、Alert、Tabs、ToggleGroup、Table、Select、Switch、Input、Textarea、Label、Field、Progress、Sonner Toast 等）。
- **禁止手写/新建 UI 组件**：除非 shadcn 没有合适的组件，否则不得自建组件、不得用原生标签堆砌已有组件的替代品（如手写 `<table>`、手写弹窗、手写遮罩、手写 toast）。
- 新增组件一律用 CLI：`npx shadcn@latest add <component>`；修改 shadcn 生成的源码前需确认为项目定制（如 sonner 的主题源已改为 `@/lib/theme`）。
- 组件底层 primitive 统一来自合并包 `radix-ui`（shadcn@latest 约定），不要安装/引用 `@radix-ui/react-*` 单包。
- 全局 toast 用 sonner：`import { toast } from "sonner"` + `<Toaster />`（`@/components/ui/sonner`），业务侧统一走 `addToast(kind, text)`。
- 确认类交互用 `AlertDialog`，弹窗用 `Dialog`，侧滑抽屉用 `Sheet`，分段选择用 `Tabs`，多选/单选 chips 用 `ToggleGroup`，数据表用 `Table`。
- 项目自有主题系统在 `src/lib/theme.tsx`（`useTheme` 返回 `{ theme, resolved, setTheme }`），不要引入 next-themes。
- 文案一律中文、面向用户；报错要说清「怎么修」，不要只有错误码。

## 代码规则

- **Rust**
  - 命令统一返回 `Result<T, String>`，错误信息是给用户看的中文可操作提示（谁占用了端口、该去哪改、下一步点哪）。
  - 阻塞操作（进程枚举、文件扫描、npm/pnpm、下载）走 `tauri::async_runtime::spawn_blocking`，不要卡住主线程。
  - 子进程语义保持「启动器退出 → 子进程被内核回收」；动 `procs.rs` 之前先看 PDEATHSIG 与独立进程注册表两套机制。
  - 新增命令：写在 `commands.rs` → 在 `lib.rs` 的 `generate_handler!` 注册 → 在 `src/api.ts` 加封装 → 在 `src/types.ts` 补类型。
  - 委托给 `dsh` 的能力一律走官方 CLI（`dsh plugin`、`dsh --profile` 等），不要自己改写 profile 的 `package.json` / `dsh.profile.bundles`。
- **TypeScript / React**
  - 所有 `invoke` 走 `api.ts`，类型放 `types.ts`（字段与 Rust 的 `serde(rename_all = "camelCase")` 对齐）。
  - 轮询 / 事件订阅用 `events.*`，不要在各组件里散落 `invoke`。
  - 用户可重复点击的动作必须有**在途守卫**（禁用 + 转圈），不要只依赖后端兜底 —— 见 `doStartProfile`。
- **提交信息**：`type(scope): 中文描述`（`feat` / `fix` / `docs` / `ci` / `perf` / `refactor` / `test` / `chore`）；一次提交只做一件事；
  用户可见的行为变化必须同时写进 `CHANGELOG.md` 的 `[Unreleased]`。

## 版本与发布（硬性）

1. **`CHANGELOG.md` 是发布说明的唯一来源**：GitHub Release 正文、`latest.json` 的 `notes`、客户端「发现新版本」弹窗都由它派生。
   每个版本必须有 `## [x.y.z] - YYYY-MM-DD` 段落，且至少一条 `- ` 条目，**只写用户可见的变化**。
2. **版本号三处同步**：`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`package.json` 必须一致
   （`npm run notes:check` 校验，CI 不一致直接失败）。
3. **归档**：发布前执行 `npm run notes:archive` 生成 `docs/releases/vX.Y.Z.md` 并一起提交；
   `docs/releases/v*.md` 是生成物，**不要手改**（该目录下只有 `README.md` 是手写的）。
4. **发版动作**：写 CHANGELOG → 改三处版本号 → `npm run notes:archive` → 合进 `main` → 完事。
   CI 自动打三平台包、把 CHANGELOG 段落写成 Release 正文再转正式；版本号没变则整作业跳过，不会重复发版。
5. 详细流程、发布后核对清单与故障排查见 `docs/RELEASING.md`。

## 构建与验证

```bash
npm run build                 # 前端：tsc 类型检查 + vite 构建（提交前必跑）
cd src-tauri && cargo check   # Rust 编译检查（改 Rust 必跑）
cd src-tauri && cargo test    # Rust 单测（现有 100+ 用例，改核心逻辑后跑）
npm run notes:check           # 版本号一致性 + CHANGELOG 本版说明（发版必跑）
npm run app                   # 本地起 Tauri 开发版；build:debug / build:release 出安装包
npm run site:build            # 下载页：tsc + vite 构建（改 site/ 必跑，等价于 cd site && npm run build）
npm run site:dev              # 下载页本地预览（dev server）
```

- 加了 Rust 依赖后 `cargo check` 需要网络（`src-tauri/.cargo/config.toml` 已配国内镜像）。
- 改 UI 至少跑 `npm run build`，确认 `tsc` 无错。
- 行为类改动（单实例、托盘、子进程生命周期）**要用真实二进制验证**，编译通过不算通过；
  例如单实例可用 `busctl --user list | grep SingleInstance` 观察。
- 下载页要**在真实浏览器里过一遍**：直链、源切换、窄屏横向溢出都只有渲染后才看得出来。
  站点依赖 shadcn registry（`npx shadcn@latest add …`）**不支持走代理**，本机有 `*_proxy` 环境变量时要临时清掉再跑。

## 安全红线

- **凭据、token、签名私钥绝不入库、绝不写日志、绝不打进前端包**：`credentials.rs` 只保存引用；CI 签名密钥来自 Secret；
  不要为了调试把 token 打印进日志或 toast。
- 外部输入（插件名、仓库地址、版本号、路径）落到磁盘/命令行前必须校验或转义（见 `util::shell_quote`、`verify.rs`）。
- 不要为了让测试通过而放宽校验（跳过验签、跳过路径检查等一律不允许）。

## 文档同步

| 改动 | 必须同步 |
| --- | --- |
| 用户可见的新能力/行为 | `README.md` + `CHANGELOG.md` 的 `[Unreleased]` |
| 发版方式、CI 行为、目录约定 | `docs/RELEASING.md`、本文件 |
| 新增命令/类型/组件的位置约定 | 本文件「项目速览」「代码规则」 |
| 下载页内容、平台清单、直链形态 | `site/src/lib/release.ts`（唯一数据层）+ `README.md` 的「下载页」小节；改平台或 R2 键还要对齐 `scripts/r2-manifest.mjs` |
