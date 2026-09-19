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
}

export interface Settings {
  registry: string;
  updateManifestUrl: string;
  defaultArgs: string;
  terminal: string;
  autoCheckUpdate: boolean;
  autoCheckVersions: boolean;
  nodePath: string;
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

export interface Toast {
  id: number;
  kind: "ok" | "err" | "info";
  text: string;
}
