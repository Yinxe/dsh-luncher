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

export interface ProfileInfo {
  name: string;
  kind: "dir" | "file";
  path: string;
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
  /** 内置 Node 运行时下载镜像站 */
  nodeMirror: string;
  closeToTray: boolean;
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
  source: "embedded" | "external" | null;
  version: string | null;
}

export interface Toast {
  id: number;
  kind: "ok" | "err" | "info";
  text: string;
}
