import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  EnvironmentInfo,
  InstallFinishedEvent,
  InstallLogEvent,
  InstanceLog,
  InstalledVersion,
  LauncherUpdateStatus,
  LaunchResult,
  ProcExitEvent,
  ProcInfo,
  ProcLogEvent,
  ProfileInfo,
  ProfileInstance,
  ProfileDetail,
  PackageSearchItem,
  PluginUpdateInfo,
  GitHubRepoInfo,
  PluginJobEvent,
  RegistryInfo,
  RuntimeFinishedEvent,
  RuntimeProgressEvent,
  Settings,
  WebQuickConfig,
  WebQuickConfigInput,
  CredentialFile,
  CredentialRef,
  ModelConfigInfo,
  ModelConfigInput,
  RemoteModelInfo,
} from "./types";

export const api = {
  getEnvironment: () => invoke<EnvironmentInfo>("get_environment"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) =>
    invoke<void>("save_settings", { settings }),
  listRemote: () => invoke<RegistryInfo>("list_remote_versions"),
  listInstalled: () => invoke<InstalledVersion[]>("list_installed"),
  install: (version: string, force = false) =>
    invoke<boolean>("install_version", { version, force }),
  cancelInstall: () => invoke<void>("cancel_install"),
  installRunning: () => invoke<string | null>("get_install_status"),
  uninstall: (version: string) => invoke<void>("uninstall_version", { version }),
  launch: (version: string | null, args?: string | null, profile?: string | null) =>
    invoke<LaunchResult>("launch_version", { version, args, profile }),
  startEmbedded: (
    version: string | null,
    profile?: string | null,
    args?: string | null,
    detached?: boolean,
  ) =>
    invoke<ProcInfo>("start_embedded", { version, profile, args, detached }),
  stopProcess: (id: number) => invoke<boolean>("stop_process", { id }),
  listProcesses: () => invoke<ProcInfo[]>("list_processes"),
  listProfileInstances: () => invoke<ProfileInstance[]>("list_profile_instances"),
  stopProfileInstance: (profile: string) =>
    invoke<boolean>("stop_profile_instance", { profile }),
  exportProcLog: (profile: string, pid: number, content: string) =>
    invoke<string>("export_proc_log", { profile, pid, content }),
  /** 独立进程实例的日志尾部；返回 null = 该实例没有文件日志（内嵌/外部启动） */
  readInstanceLog: (pid: number, maxBytes?: number) =>
    invoke<InstanceLog | null>("read_instance_log", { pid, maxBytes: maxBytes ?? null }),
  getProfileDetail: (profile: string) =>
    invoke<ProfileDetail>("get_profile_detail", { profile }),
  getPatchReload: (profile: string) => invoke<string>("get_patch_reload", { profile }),
  setBundleEnabled: (profile: string, name: string, enabled: boolean) =>
    invoke<void>("set_bundle_enabled", { profile, name, enabled }),
  uninstallBundle: (profile: string, name: string) =>
    invoke<boolean>("uninstall_bundle", { profile, name }),
  installBundle: (profile: string, name: string) =>
    invoke<boolean>("install_bundle", { profile, name }),
  readProfileFile: (profile: string, file: string) =>
    invoke<string>("read_profile_file", { profile, file }),
  writeProfileFile: (profile: string, file: string, content: string) =>
    invoke<void>("write_profile_file", { profile, file, content }),
  getWebQuickConfig: (profile: string) =>
    invoke<WebQuickConfig>("get_web_quick_config", { profile }),
  setWebQuickConfig: (profile: string, config: WebQuickConfigInput) =>
    invoke<void>("set_web_quick_config", { profile, config }),
  copyProfile: (source: string, newName: string) =>
    invoke<void>("copy_profile", { source, newName }),
  /** 重命名 profile（dsh 内置保留 profile 会被后端拒绝）；返回新名字 */
  renameProfile: (name: string, newName: string) =>
    invoke<string>("rename_profile", { name, newName }),
  /** 删除 profile（移入 ~/.dsh-launcher/deleted-profiles/ 可找回）；返回回收站路径 */
  deleteProfile: (name: string) =>
    invoke<string>("delete_profile", { name }),
  /** 生成恢复模式 profile：基于官方 web 复制，只留官方插件，换成邻近空闲端口 */
  createRecoveryProfile: () =>
    invoke<{ name: string; port: number }>("create_recovery_profile"),
  searchPackages: (query: string) =>
    invoke<PackageSearchItem[]>("search_registry_packages", { query }),
  checkPluginUpdates: (profile: string) =>
    invoke<PluginUpdateInfo[]>("check_plugin_updates", { profile }),
  fetchGithubRepo: (repo: string) =>
    invoke<GitHubRepoInfo>("fetch_github_repo", { repo }),
  readGlobalConfig: () => invoke<string>("read_global_config"),
  writeGlobalConfig: (content: string) => invoke<void>("write_global_config", { content }),
  getModelConfig: () => invoke<ModelConfigInfo>("get_model_config"),
  setModelConfig: (config: ModelConfigInput) =>
    invoke<void>("set_model_config", { config }),
  fetchProviderModels: (baseUrl: string, api: string, apiKeyEnv: string, apiKey?: string) =>
    invoke<RemoteModelInfo[]>("fetch_provider_models", {
      baseUrl,
      api,
      apiKeyEnv,
      apiKey: apiKey ?? null,
    }),
  getCredentials: () => invoke<CredentialFile>("get_credentials"),
  writeCredentialRefs: (refs: CredentialRef[]) =>
    invoke<void>("write_credential_refs", { refs }),
  listProfiles: () => invoke<ProfileInfo[]>("list_profiles"),
  reveal: (path: string) => invoke<void>("reveal_folder", { path }),
  openUrl: (url: string) => invoke<void>("open_external", { url }),
  installRuntime: () => invoke<string>("install_runtime"),
  checkLauncherUpdate: () => invoke<LauncherUpdateStatus>("check_launcher_update"),
};

export const events = {
  onInstallLog: (cb: (e: InstallLogEvent) => void): Promise<UnlistenFn> =>
    listen<InstallLogEvent>("install-log", (e) => cb(e.payload)),
  onInstallFinished: (cb: (e: InstallFinishedEvent) => void): Promise<UnlistenFn> =>
    listen<InstallFinishedEvent>("install-finished", (e) => cb(e.payload)),
  onLauncherUpdate: (cb: (s: LauncherUpdateStatus) => void): Promise<UnlistenFn> =>
    listen<LauncherUpdateStatus>("launcher-update", (e) => cb(e.payload)),
  onProcLog: (cb: (e: ProcLogEvent) => void): Promise<UnlistenFn> =>
    listen<ProcLogEvent>("proc-log", (e) => cb(e.payload)),
  onProcExit: (cb: (e: ProcExitEvent) => void): Promise<UnlistenFn> =>
    listen<ProcExitEvent>("proc-exit", (e) => cb(e.payload)),
  onRuntimeLog: (cb: (line: string) => void): Promise<UnlistenFn> =>
    listen<string>("runtime-log", (e) => cb(e.payload)),
  onRuntimeProgress: (cb: (e: RuntimeProgressEvent) => void): Promise<UnlistenFn> =>
    listen<RuntimeProgressEvent>("runtime-progress", (e) => cb(e.payload)),
  onRuntimeFinished: (cb: (e: RuntimeFinishedEvent) => void): Promise<UnlistenFn> =>
    listen<RuntimeFinishedEvent>("runtime-finished", (e) => cb(e.payload)),
  onPluginLog: (cb: (e: PluginJobEvent) => void): Promise<UnlistenFn> =>
    listen<PluginJobEvent>("plugin-log", (e) => cb(e.payload)),
  onToast: (cb: (text: string) => void): Promise<UnlistenFn> =>
    listen<string>("toast", (e) => cb(e.payload)),
};
