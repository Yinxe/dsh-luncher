import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  EnvironmentInfo,
  InstallFinishedEvent,
  InstallLogEvent,
  InstalledVersion,
  LauncherUpdateStatus,
  LaunchResult,
  ProcExitEvent,
  ProcInfo,
  ProcLogEvent,
  ProfileInfo,
  ProfileInstance,
  PluginEntryInfo,
  ProfileDetail,
  PluginJobEvent,
  RegistryInfo,
  RuntimeFinishedEvent,
  RuntimeProgressEvent,
  Settings,
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
  startEmbedded: (version: string | null, profile?: string | null, args?: string | null) =>
    invoke<ProcInfo>("start_embedded", { version, profile, args }),
  stopProcess: (id: number) => invoke<boolean>("stop_process", { id }),
  listProcesses: () => invoke<ProcInfo[]>("list_processes"),
  listProfileInstances: () => invoke<ProfileInstance[]>("list_profile_instances"),
  stopProfileInstance: (profile: string) =>
    invoke<boolean>("stop_profile_instance", { profile }),
  exportProcLog: (profile: string, pid: number, content: string) =>
    invoke<string>("export_proc_log", { profile, pid, content }),
  getProfileDetail: (profile: string) =>
    invoke<ProfileDetail>("get_profile_detail", { profile }),
  listProfilePlugins: (profile: string) =>
    invoke<PluginEntryInfo[]>("list_profile_plugins", { profile }),
  setProfilePlugin: (profile: string, id: string, disabled: boolean) =>
    invoke<void>("set_profile_plugin", { profile, id, disabled }),
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
  readGlobalConfig: () => invoke<string>("read_global_config"),
  writeGlobalConfig: (content: string) => invoke<void>("write_global_config", { content }),
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
