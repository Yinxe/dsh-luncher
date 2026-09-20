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
  ChannelProbe,
  GitHubRepoInfo,
  GitHubRateLimit,
  PluginCandidate,
  PluginJob,
  PluginJobEvent,
  PluginLogEvent,
  ClonedPlugin,
  CloneInstallInput,
  RegistryInfo,
  RuntimeFinishedEvent,
  RuntimeProgressEvent,
  Settings,
  WebQuickConfig,
  WebQuickConfigInput,
  CredentialFile,
  CredentialRef,
  DeletedProfile,
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
  // ── 插件管理：全部走官方 dsh plugin 命令，输出实时回流到内置终端 ──
  /** 安装/升级插件（dsh plugin add，可一次多个规格），返回任务 id */
  pluginInstall: (profile: string, specs: string[], mode: "install" | "upgrade" = "install") =>
    invoke<number>("plugin_install", { profile, specs, mode }),
  /** 卸载插件（dsh plugin remove），可选顺带删除本地克隆目录 */
  pluginUninstall: (profile: string, name: string, purgeCloneDir?: string | null) =>
    invoke<number>("plugin_uninstall", {
      profile,
      name,
      purgeCloneDir: purgeCloneDir ?? null,
    }),
  /** 升级本地克隆插件：git pull →（可选）构建 → dsh plugin add link:… */
  pluginPullUpdate: (
    profile: string,
    name: string,
    cloneRoot: string,
    subPath: string | null,
    build: boolean,
  ) =>
    invoke<number>("plugin_pull_update", {
      profile,
      name,
      cloneRoot,
      subPath,
      build,
    }),
  /** clone 仓库 + 本地 link 安装 */
  pluginCloneInstall: (profile: string, input: CloneInstallInput) =>
    invoke<number>("plugin_clone_install", { profile, input }),
  /** 当前会话的全部插件任务（含日志尾部，用于重挂载恢复） */
  listPluginJobs: () => invoke<PluginJob[]>("list_plugin_jobs"),
  cancelPluginJob: (jobId: number) => invoke<boolean>("cancel_plugin_job", { jobId }),
  clearPluginJobs: () => invoke<number>("clear_plugin_jobs"),
  /** 导出某个任务的完整日志，返回文件路径 */
  exportPluginJobLog: (jobId: number) =>
    invoke<string>("export_plugin_job_log", { jobId }),
  /** ~/.dsh-launcher/git-plugins 下的克隆仓库清单 */
  listClonedPlugins: () => invoke<ClonedPlugin[]>("list_cloned_plugins"),
  /** 探测本地目录里的插件包（monorepo 子包一并列出） */
  probeLocalPlugins: (path: string) => invoke<PluginCandidate[]>("probe_local_plugins", { path }),
  deleteClonedPlugin: (dirName: string) =>
    invoke<void>("delete_cloned_plugin", { dirName }),
  /** 取（并创建）git-plugins 目录路径，供在文件管理器中打开 */
  gitPluginsDir: () => invoke<string>("reveal_git_plugins_dir"),
  readProfileFile: (profile: string, file: string) =>
    invoke<string>("read_profile_file", { profile, file }),
  writeProfileFile: (profile: string, file: string, content: string) =>
    invoke<void>("write_profile_file", { profile, file, content }),
  getWebQuickConfig: (profile: string) =>
    invoke<WebQuickConfig>("get_web_quick_config", { profile }),
  setWebQuickConfig: (profile: string, config: WebQuickConfigInput) =>
    invoke<void>("set_web_quick_config", { profile, config }),
  /** 复制 profile；返回自动错开后的 web 端口（null = 该 profile 没有 webserver 配置） */
  copyProfile: (source: string, newName: string) =>
    invoke<number | null>("copy_profile", { source, newName }),
  /** 重命名 profile（dsh 内置保留 profile 会被后端拒绝）；返回新名字 */
  renameProfile: (name: string, newName: string) =>
    invoke<string>("rename_profile", { name, newName }),
  /** 删除 profile（移入 ~/.dsh-launcher/deleted-profiles/ 可找回）；返回回收站路径 */
  deleteProfile: (name: string) =>
    invoke<string>("delete_profile", { name }),
  /** 回收站：删除的 profile 可列出 / 还原 / 彻底删除 */
  listDeletedProfiles: () => invoke<DeletedProfile[]>("list_deleted_profiles"),
  restoreDeletedProfile: (dirName: string) =>
    invoke<string>("restore_deleted_profile", { dirName }),
  purgeDeletedProfile: (dirName: string) =>
    invoke<void>("purge_deleted_profile", { dirName }),
  /** 生成恢复模式 profile：dsh 用 `--from-default-profile web` 从随附模板新建，
   *  再写入端口等快捷配置（不复制当前的 web profile） */
  createRecoveryProfile: () =>
    invoke<{ name: string; port: number }>("create_recovery_profile"),
  searchPackages: (query: string) =>
    invoke<PackageSearchItem[]>("search_registry_packages", { query }),
  checkPluginUpdates: (profile: string) =>
    invoke<PluginUpdateInfo[]>("check_plugin_updates", { profile }),
  fetchGithubRepo: (repo: string) =>
    invoke<GitHubRepoInfo>("fetch_github_repo", { repo }),
  /** 当前 GitHub API 额度（元数据增强用；探测与更新检测走免额度通道） */
  getGithubRateLimit: () => invoke<GitHubRateLimit>("get_github_rate_limit"),
  /** 通道自检：并发探测 refs / jsDelivr / raw / api 的可达性与延迟 */
  checkChannels: () => invoke<ChannelProbe[]>("check_channels"),
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
  onPluginLog: (cb: (e: PluginLogEvent) => void): Promise<UnlistenFn> =>
    listen<PluginLogEvent>("plugin-log", (e) => cb(e.payload)),
  onPluginJob: (cb: (e: PluginJobEvent) => void): Promise<UnlistenFn> =>
    listen<PluginJobEvent>("plugin-job", (e) => cb(e.payload)),
  onToast: (cb: (text: string) => void): Promise<UnlistenFn> =>
    listen<string>("toast", (e) => cb(e.payload)),
};
