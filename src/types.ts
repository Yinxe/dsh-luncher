/** 主内容区的视图标识（侧栏导航项与之对应） */
export type View = "quick" | "versions" | "profiles" | "plugins" | "models" | "config" | "credentials";

export type InstallSource = "managed" | "global" | "path";

export interface RemoteVersion {
  version: string;
  channel: string;
  tags: string[];
  publishedAt: string | null;
  description: string | null;
  unpackedSize: number | null;
}

export interface RegistryInfo {
  tags: Record<string, string>;
  versions: RemoteVersion[];
}

export interface InstalledVersion {
  version: string;
  source: InstallSource;
  location: string;
  binJs: string | null;
  nodePath: string | null;
}

/** profile 的运行 Target：不同 Target 有各自的启动方式，目前仅实现 Web（打开浏览器） */
export type ProfileTarget = "web" | "desktop" | "unknown";

export interface ProfileInfo {
  name: string;
  kind: "dir" | "file";
  path: string;
  /** 由 package.json 的 dsh.profile.bundles 识别：含 @deepseek-ai/dsh-web-app ⇒ web */
  target: ProfileTarget;
  /** dsh 内置保留 profile（不可改名/删除） */
  reserved: boolean;
}

export interface EnvironmentInfo {
  appVersion: string;
  os: string;
  arch: string;
  node: string | null;
  nodePath: string | null;
  npm: string | null;
  npmPath: string | null;
  dshHome: string;
  versionsDir: string;
  registry: string;
  dshNativeHome: string;
  profilesDir: string;
  runtimeInstalled: boolean;
  runtimeDir: string;
  logsDir: string;
  /** dsh 是否已初始化过（$DSH_HOME/profiles 里有没有 profile） */
  dshInitialized: boolean;
}

export interface Settings {
  registry: string;
  updateManifestUrl: string;
  /** 更新下载源：r2 = 自建 Cloudflare R2（默认，快）| github = 官方源 */
  updateSource: "r2" | "github" | string;
  defaultArgs: string;
  /** 默认启动的 profile（空 = 不带 --profile，走 dsh 默认） */
  defaultProfile: string;
  /** 当前使用的 dsh 版本：所有 profile 都基于该版本运行 */
  activeVersion: string;
  terminal: string;
  autoCheckUpdate: boolean;
  /** 发现启动器新版本后直接静默下载安装并重启（仅内置 updater 模式） */
  autoInstallUpdate: boolean;
  autoCheckVersions: boolean;
  nodePath: string;
  /** Node 来源：auto（系统优先，缺失回退内置）| system | runtime */
  nodeSource: string;
  /** 内置 Node 运行时下载镜像站 */
  nodeMirror: string;
  closeToTray: boolean;
  /** Profile 启动方式：child=子进程（随启动器退出）| detached=独立进程（后台常驻） */
  launchMode: string;
  /** 可选 GitHub Token：只用于把 api.github.com 额度从 60/小时 提到 5000/小时 */
  githubToken: string;
  /** GitHub 加速：把 github 链接拼到测速选出的前缀代理上（只对 github 域名生效） */
  githubAccel: boolean;
  /** 固定使用哪个代理前缀；留空 = 自动（最快的一个） */
  githubProxy: string;
  /** 额外候选前缀（逗号 / 换行分隔），与内置清单一起参与测速 */
  githubProxyExtra: string;
}

/** 一个可用的前缀代理及其测速耗时 */
export interface ProxyNode {
  /** 形如 https://gh-proxy.com/ */
  prefix: string;
  /** 文件下载测速（GET 小文件）往返毫秒 */
  ms: number;
  /** git 测速（真克隆一个极小仓库）毫秒；null = 该前缀只放行文件下载，不能 git */
  gitMs: number | null;
}

/** GitHub 加速状态（候选前缀 → 测速 → 缓存） */
export interface GhAccel {
  /** 秒级时间戳 */
  updatedAt: number;
  /** 按快慢排序，第一个 = 自动模式使用 */
  nodes: ProxyNode[];
  /** builtin | builtin+extra | cache */
  source: string;
  /** 是否来自磁盘缓存 */
  cached: boolean;
}

/** clone 探测结果：真实克隆到本地后扫描出的插件包 */
export interface CloneProbe {
  url: string;
  /** 克隆落点（~/.dsh-starter/git-plugins/<owner>-<repo>） */
  root: string;
  dirName: string;
  gitRef: string | null;
  /** 本次探测的加速摘要（域名=IP (ms)），未启用为 null */
  accel: string | null;
  candidates: PluginCandidate[];
}

/** 一条免额度/受限通道的自检结果 */
export interface ChannelProbe {
  /** github-refs | jsdelivr | raw | github-api */
  name: string;
  ok: boolean;
  /** 往返毫秒 */
  ms: number;
  detail: string | null;
}

/** GitHub API 额度状态（探测/更新检测走免额度通道，这里只反映元数据额度） */
export interface GitHubRateLimit {
  limit: number | null;
  remaining: number | null;
  /** 额度重置的 unix 秒 */
  reset: number | null;
  authenticated: boolean;
  exhausted: boolean;
}

export interface InstallLogEvent {
  version: string;
  line: string;
  stream: string;
}

export interface InstallFinishedEvent {
  version: string;
  success: boolean;
  message: string;
}

export interface StarterUpdateStatus {
  available: boolean;
  current: string;
  latest: string | null;
  notes: string | null;
  url: string | null;
  /** builtin = 内置 updater，可在应用内直接安装；其余模式只能跳转下载页 */
  mode: "manifest" | "builtin" | "unconfigured" | "unsupported" | "error";
  /** 安装时需要管理员授权（deb / rpm：pkexec + dpkg / rpm -U，会弹系统密码框） */
  needsElevation: boolean;
  message: string | null;
}

export interface StarterUpdateProgress {
  received: number;
  total: number;
}

export interface LaunchResult {
  ok: boolean;
  message: string;
}

export interface ProcInfo {
  id: number;
  version: string;
  profile: string;
  startedAt: number;
  running: boolean;
}

export interface ProcLogEvent {
  id: number;
  version: string;
  profile: string;
  line: string;
  stream: string;
}

export interface ProcExitEvent {
  id: number;
  version: string;
  profile: string;
  code: number | null;
  stopped: boolean;
}

/** 前端维护的单个 dsh 实例视图状态（内嵌子进程走实时管道，独立/外部实例没有管道） */
export interface ProcEntry {
  id: number;
  version: string;
  profile: string;
  startedAt: number;
  lines: string[];
  exited: boolean;
  code: number | null;
  /** 从日志中识别出的 dsh web UI 地址 */
  webUrl: string | null;
  /** true = 非内嵌实例（独立进程 / 终端外部启动）：日志不在管道里 */
  external?: boolean;
  /** 独立进程的日志文件路径（外部实例为 null） */
  logFile?: string | null;
  /** web 监听端口 */
  port?: number | null;
}

/** 回收站条目（删除的 profile 只是被移入这里，可还原或彻底删除） */
export interface DeletedProfile {
  /** 回收站里的目录/文件名（还原、彻底删除用它定位） */
  dirName: string;
  /** 解析出的原 profile 名 */
  name: string;
  path: string;
  /** 删除时间（毫秒时间戳） */
  deletedAt: number;
  isDir: boolean;
}

/** 独立进程的日志读取结果（read_instance_log） */
export interface InstanceLog {
  path: string;
  content: string;
  truncated: boolean;
}

export interface RuntimeProgressEvent {
  received: number;
  total: number;
}

export interface RuntimeFinishedEvent {
  ok: boolean;
  message: string;
}

export interface ProfileInstance {
  profile: string;
  running: boolean;
  pid: number | null;
  /** embedded=启动器子进程 | external=终端/外部启动 | detached=启动器派生的独立进程 | port=按端口探测发现 */
  source: string | null;
  version: string | null;
  /** web 实例的监听端口（按端口发现时提供；无名实例靠它区分） */
  port: number | null;
  /** 独立进程的日志文件路径（内嵌/外部实例为 null） */
  logFile: string | null;
  /** 启动时间戳（毫秒）；外部实例未知 */
  startedAt: number | null;
  /** 从实例日志解析出的 dsh 访问地址（独立进程由后端解析；外部实例没有日志可解） */
  webUrl: string | null;
}

export interface BundleInfo {
  name: string;
  version: string | null;
  source: string;
  enabled: boolean;
  /** 该包通过 patch 层声明的真实插件 id（包名 ≠ 插件 id） */
  pluginIds: string[];
  /** 宿主自带（in-box）：不可卸载、不可停用 */
  official: boolean;
}

export interface PatchItemInfo {
  id: string | null;
  name: string | null;
  disabled: boolean;
}

export interface PatchEntryInfo {
  index: number;
  kind: string;
  id: string | null;
  disabled: boolean;
  items: PatchItemInfo[];
}

/** dependencies 里的一个直接依赖（isBundle=false 即「装了但不是插件」的包，可手动卸载） */
export interface PackageDepInfo {
  name: string;
  version: string | null;
  source: string;
  isBundle: boolean;
}

export interface ProfileDetail {
  profile: string;
  exists: boolean;
  bundles: BundleInfo[];
  packages: PackageDepInfo[];
  /** package.json 原始内容（与 Rust package_raw 对齐） */
  packageRaw: string;
  patchRaw: string;
  patchEntries: PatchEntryInfo[];
}

/**
 * 一个 dsh 版本的发布说明（来自官方 monorepo 的 GitHub Release，tag 前缀 `dsh-v`）。
 * 正文是中英双段 Markdown，小标题用 `<h3 id="cn-…">` 这类锚点标记语言分段。
 */
export interface DshRelease {
  version: string;
  tag: string;
  title: string;
  publishedAt: string | null;
  prerelease: boolean;
  body: string;
  htmlUrl: string;
}

/** npm registry 搜索结果（安装对话框：先搜索 → 看描述 → 再安装） */
export interface PackageSearchItem {
  name: string;
  version: string;
  description: string | null;
  publishedAt: string | null;
  link: string | null;
}

/** 探测到的一个可安装插件包（仓库根 / monorepo 子包 / 本地目录子包共用） */
export interface PluginCandidate {
  /** 相对插件根目录的路径；"" = 根目录 */
  path: string;
  /** package.json 的 name（读不到时为 null） */
  name: string | null;
  version: string | null;
  description: string | null;
  /** lib/ 目录存在且含构建产物（.js/.cjs/.mjs） */
  libOk: boolean;
  /** package.json 声明了 dsh.bundle.patch —— dsh 会把它并入 profile 层 */
  hasBundle: boolean;
  /** 插件根目录内有 cordis.patch.yml */
  hasPatch: boolean;
  /** 命中 pnpm-workspace / workspaces 成员 glob */
  workspaceMember: boolean;
  /** 可以直接作为插件安装（声明了 dsh.bundle） */
  ready: boolean;
  /** 交给 dsh plugin add 的安装规格 */
  installSpec: string;
}

/** 插件更新检测结果（npm 比对版本号；GitHub 比对提交；本地克隆比对 git HEAD；纯本地/直链无渠道） */
export interface PluginUpdateInfo {
  name: string;
  /** npm | git | git-clone | tarball | link | file | workspace */
  source: string;
  spec: string;
  hasUpdate: boolean;
  checked: boolean;
  installedVersion: string | null;
  latestVersion: string | null;
  repo: string | null;
  installedCommit: string | null;
  remoteCommit: string | null;
  /** 可交给 dsh plugin add 的升级规格（npm=包名@latest；git=去 sha 的 github 规格；clone=link:路径） */
  updateSpec: string | null;
  /** 升级方式：add（重新走 dsh plugin add）/ git-pull（先 pull 再重新 link）/ null（无渠道） */
  updateKind: "add" | "git-pull" | null;
  /** 本地 link 目标路径 */
  localPath: string | null;
  /** git 工作树根绝对路径（git-clone 源） */
  cloneDir: string | null;
  /** 该工作树是否由启动器克隆（~/.dsh-starter/git-plugins 下） */
  managedClone: boolean | null;
  /** 插件目录相对 git 根的路径（重新 link 用） */
  subPath: string | null;
  /** git 工作树有未提交改动 */
  dirty: boolean | null;
  /** 无渠道/被拒绝的原因：no-channel（link 或直链本就没有渠道）/ private-repo（私有仓库，暂不支持） */
  blocked: "no-channel" | "private-repo" | null;
  /** link 目标目录里是否有构建产物（lib/*.js）；false = 升级时需要重新构建 */
  libOk: boolean | null;
  note: string | null;
}

/** 插件管理任务（内置终端里的一条记录） */
export interface PluginJob {
  id: number;
  profile: string;
  /** install | uninstall | upgrade | pull | clone */
  kind: string;
  label: string;
  command: string;
  startedAt: number;
  finishedAt: number | null;
  running: boolean;
  ok: boolean | null;
  exitCode: number | null;
  cancelled: boolean;
  /** 失败时的对症建议（首行适合直接放进 toast） */
  hint: string | null;
  /** 本次任务实际执行的 dsh plugin 参数（与 Rust argv 对齐，「允许构建脚本并重试」原样重跑） */
  argv: string[];
  /** 被 pnpm 拦下的构建脚本所属包（非空时显示「允许构建脚本并重试」） */
  pendingBuilds: string[];
  lines: PluginLogLine[];
  /** 因缓冲上限被丢弃的行数 */
  dropped: number;
}

export interface PluginLogLine {
  /** stdout | stderr | info */
  stream: string;
  text: string;
  at: number;
}

/** 一条流式输出（内置终端实时追加） */
export interface PluginLogEvent {
  jobId: number;
  profile: string;
  stream: string;
  line: string;
}

/** 任务状态变化（开始 / 结束） */
export interface PluginJobEvent {
  jobId: number;
  profile: string;
  kind: string;
  label: string;
  running: boolean;
  ok: boolean | null;
  exitCode: number | null;
  cancelled: boolean;
  /** 失败时的对症建议 */
  hint: string | null;
  /** 待放行的构建脚本包名（按钮显示条件） */
  pendingBuilds: string[];
  startedAt: number;
  finishedAt: number | null;
}

/** 本地克隆仓库（clone + link 安装的落点） */
export interface ClonedPlugin {
  dirName: string;
  path: string;
  url: string | null;
  branch: string | null;
  commit: string | null;
  subject: string | null;
  dirty: boolean;
  candidates: PluginCandidate[];
}

/** clone + link 安装输入 */
export interface CloneInstallInput {
  url: string;
  gitRef: string | null;
  subPath: string | null;
  build: boolean;
}

/** web 快捷配置当前值（解析自 cordis.patch.yml；条目不存在 = *Present=false，键缺失 = null） */
export interface WebQuickConfig {
  webserverPresent: boolean;
  host: string | null;
  port: number | null;
  webRuntimePresent: boolean;
  openBrowser: boolean | null;
  printUrl: boolean | null;
  surfaceContext: boolean | null;
  connectionPresent: boolean;
  cookieMaxAgeDays: number | null;
}

/** web 快捷配置保存载荷：三个 patch 条目由启动器整块生成（键成套写全，trustedHosts 联动 !!js 信任链；
 *  printUrl 不开放——启动器依赖启动日志识别访问地址，恒为 true） */
export interface WebQuickConfigInput {
  host: string;
  port: number;
  openBrowser: boolean;
  surfaceContext: boolean;
  cookieMaxAgeDays: number;
}

/** 凭据文件 ~/.dsh/.credentials.yaml 中 refs 的一条命名凭据 */
export interface CredentialRef {
  name: string;
  value: string;
  /**
   * 注释：YAML 里键正上方那一行 `# 注释` 的内容。
   * 写入时同样写成键上方的一行注释；null 表示这条凭据没有注释。
   */
  note: string | null;
}

/** records 里的一条内部凭据记录（dsh 自管理，只读展示；secret 值不回传前端） */
export interface CredentialRecord {
  key: string;
  kind: string | null;
  secretLength: number | null;
  payloadKeys: string[];
}

/** 凭据文件整体读取结果（文件不存在时 exists=false 而非报错） */
export interface CredentialFile {
  path: string;
  exists: boolean;
  version: number | null;
  refs: CredentialRef[];
  records: CredentialRecord[];
}

/** 模型配置里的一个可用模型（llm-pi-ai.providers.<id>.models[] 条目） */
export interface ModelEntryInfo {
  id: string;
  name: string | null;
  contextWindow: number | null;
  maxTokens: number | null;
  /** 输入模态（如 text / image） */
  input: string[];
  /** 推理档位映射：逻辑档位名 → API 参数值（值可为 null），原样透传 */
  reasoningEfforts: Record<string, string | null> | null;
  /** 未识别字段原样透传（保存时原位恢复，避免丢数据） */
  extra: Record<string, unknown> | null;
}

/** 模型配置里的一个模型服务提供方（llm-pi-ai.providers 的一个键值对） */
export interface ProviderEntryInfo {
  id: string;
  displayName: string | null;
  /** openai-completions | openai-responses | anthropic-messages（自定义值原样透传） */
  api: string | null;
  baseURL: string | null;
  /** API Key 的环境变量名（值可在环境变量或凭据 refs 中维护） */
  apiKeyEnv: string | null;
  headers: Record<string, unknown> | null;
  compat: Record<string, unknown> | null;
  models: ModelEntryInfo[];
  extra: Record<string, unknown> | null;
}

/** agent-default-model：Agent 默认使用的模型与推理档位 */
export interface DefaultModelInfo {
  provider: string;
  model: string;
  /** 取所选模型 reasoningEfforts 的键名（如 low / high / xhigh） */
  reasoningEffort: string | null;
  extra: Record<string, unknown> | null;
}

/** 模型配置整体读取结果（解析自 ~/.dsh/settings.yaml 的两个分节） */
export interface ModelConfigInfo {
  path: string;
  exists: boolean;
  providers: ProviderEntryInfo[];
  defaultModel: DefaultModelInfo | null;
  /** 文件存在但解析失败时置位：禁止结构化保存 */
  parseError: string | null;
}

/** 模型配置保存载荷（后端只重写 llm-pi-ai / agent-default-model 两节） */
export interface ModelConfigInput {
  providers: ProviderEntryInput[];
  defaultModel: DefaultModelInput | null;
}

export interface ProviderEntryInput {
  id: string;
  displayName: string | null;
  api: string | null;
  baseURL: string | null;
  apiKeyEnv: string | null;
  headers: Record<string, unknown> | null;
  compat: Record<string, unknown> | null;
  models: ModelEntryInput[];
  extra: Record<string, unknown> | null;
}

export interface ModelEntryInput {
  id: string;
  name: string | null;
  contextWindow: number | null;
  maxTokens: number | null;
  input: string[] | null;
  reasoningEfforts: Record<string, string | null> | null;
  extra: Record<string, unknown> | null;
}

export interface DefaultModelInput {
  provider: string;
  model: string;
  reasoningEffort: string | null;
  extra: Record<string, unknown> | null;
}

/** 远端 /models 接口解析出的可用模型条目 */
export interface RemoteModelInfo {
  id: string;
  name: string | null;
}

export interface Toast {
  id: number;
  kind: "ok" | "err" | "info";
  text: string;
}
