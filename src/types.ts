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
}

export interface Settings {
  registry: string;
  updateManifestUrl: string;
  defaultArgs: string;
  /** 默认启动的 profile（空 = 不带 --profile，走 dsh 默认） */
  defaultProfile: string;
  /** 当前使用的 dsh 版本：所有 profile 都基于该版本运行 */
  activeVersion: string;
  terminal: string;
  autoCheckUpdate: boolean;
  autoCheckVersions: boolean;
  nodePath: string;
  /** Node 来源：auto（系统优先，缺失回退内置）| system | runtime */
  nodeSource: string;
  /** 内置 Node 运行时下载镜像站 */
  nodeMirror: string;
  closeToTray: boolean;
  /** Profile 启动方式：child=子进程（随启动器退出）| detached=独立进程（后台常驻） */
  launchMode: string;
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

export interface LauncherUpdateStatus {
  available: boolean;
  current: string;
  latest: string | null;
  notes: string | null;
  url: string | null;
  mode: "manifest" | "builtin" | "unconfigured" | "error";
  message: string | null;
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

/** 前端维护的单个内嵌 dsh 进程视图状态 */
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
  /** embedded=启动器子进程 | external=终端/外部启动 | detached=启动器派生的独立进程 */
  source: string | null;
  version: string | null;
}

export interface BundleInfo {
  name: string;
  version: string | null;
  source: string;
  enabled: boolean;
  /** 该包通过 patch 层声明的真实插件 id（包名 ≠ 插件 id） */
  pluginIds: string[];
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
  patchRaw: string;
  patchEntries: PatchEntryInfo[];
}

/** npm registry 搜索结果（安装对话框：先搜索 → 看描述 → 再安装） */
export interface PackageSearchItem {
  name: string;
  version: string;
  description: string | null;
  publishedAt: string | null;
  link: string | null;
}

/** GitHub 插件来源预览（安装前校验：插件根目录必须含 lib/） */
export interface GitHubRepoInfo {
  fullName: string;
  description: string | null;
  stars: number;
  pushedAt: string | null;
  htmlUrl: string;
  license: string | null;
  gitRef: string | null;
  /** 插件在仓库内的路径（插件根目录） */
  pluginPath: string | null;
  /** lib/ 目录校验：true=已确认存在 false=确认缺失 null=未校验（打包产物直装） */
  libOk: boolean | null;
  /** 最终交给 dsh plugin add 的安装规格（github:owner/repo#ref&path:xx 或打包产物 URL） */
  installSpec: string;
}

/** 插件更新检测结果（npm 比对版本号；GitHub 比对提交哈希；本地源跳过） */
export interface PluginUpdateInfo {
  name: string;
  source: string;
  spec: string;
  hasUpdate: boolean;
  checked: boolean;
  installedVersion: string | null;
  latestVersion: string | null;
  repo: string | null;
  installedCommit: string | null;
  remoteCommit: string | null;
  /** 可直接交给 dsh plugin add 的升级规格（npm=包名@latest；git=去 sha 的 github 规格） */
  updateSpec: string | null;
  note: string | null;
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

export interface PluginJobEvent {
  profile: string;
  line: string;
  done: boolean;
  ok: boolean;
}

/** 凭据文件 ~/.dsh/.credentials.yaml 中 refs 的一条命名凭据 */
export interface CredentialRef {
  name: string;
  value: string;
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

export interface Toast {
  id: number;
  kind: "ok" | "err" | "info";
  text: string;
}
