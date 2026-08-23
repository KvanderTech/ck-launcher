import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import type {
  AccountSummary,
  GameExitedEvent,
  GameStartedEvent,
  GameVersionSummary,
  JavaMajor,
  JavaRuntimeStatus,
  LauncherErrorEvent,
  LauncherProfile,
  MemorySettingsStatus,
  OperationId,
  ProgressEvent,
} from "./types";

export interface LauncherApi {
  listAccounts(): Promise<AccountSummary[]>;
  beginMicrosoftLogin(): Promise<AccountSummary>;
  cancelMicrosoftLogin(): Promise<void>;
  removeAccount(accountId: string): Promise<void>;
  setActiveAccount(accountId: string): Promise<void>;
}

export const launcherApi: LauncherApi = {
  listAccounts: () => invoke<AccountSummary[]>("list_accounts"),
  beginMicrosoftLogin: () => invoke<AccountSummary>("begin_microsoft_login"),
  cancelMicrosoftLogin: () => invoke<void>("cancel_microsoft_login"),
  removeAccount: (accountId) => invoke<void>("remove_account", { accountId }),
  setActiveAccount: (accountId) =>
    invoke<void>("set_active_account", { accountId }),
};

export interface RuntimeApi {
  runtimeStatuses(): Promise<JavaRuntimeStatus[]>;
  detectRuntime(requirement: JavaMajor): Promise<JavaRuntimeStatus>;
  installRuntime(requirement: JavaMajor): Promise<JavaRuntimeStatus>;
  chooseRuntimePath(requirement: JavaMajor): Promise<JavaRuntimeStatus | null>;
}

export const runtimeApi: RuntimeApi = {
  runtimeStatuses: () => invoke<JavaRuntimeStatus[]>("runtime_statuses"),
  detectRuntime: (requirement) => invoke<JavaRuntimeStatus>("detect_runtime", { requirement }),
  installRuntime: (requirement) => invoke<JavaRuntimeStatus>("install_runtime", { requirement }),
  chooseRuntimePath: (requirement) => invoke<JavaRuntimeStatus | null>("choose_runtime_path", { requirement }),
};

export interface SettingsApi {
  memoryStatus(): Promise<MemorySettingsStatus>;
  updateMemory(memoryMb: number): Promise<LauncherProfile>;
}

export const settingsApi: SettingsApi = {
  memoryStatus: () => invoke<MemorySettingsStatus>("memory_status"),
  updateMemory: (memoryMb) => invoke<LauncherProfile>("update_profile_memory", { memoryMb }),
};

export interface ProfileApi {
  listGameVersions(): Promise<GameVersionSummary[]>;
  requiredJavaForVersion(versionId: string): Promise<JavaMajor>;
  getProfile(): Promise<LauncherProfile>;
  updateProfile(profile: LauncherProfile): Promise<LauncherProfile>;
  chooseGameDirectory(): Promise<LauncherProfile | null>;
}

export interface OperationApi {
  launchOrInstall(profileId: string): Promise<OperationId>;
  cancelOperation(operationId: OperationId): Promise<void>;
  openLatestGameLog(): Promise<void>;
  onProgress(handler: (event: ProgressEvent) => void): Promise<UnlistenFn>;
  onGameStarted(handler: (event: GameStartedEvent) => void): Promise<UnlistenFn>;
  onGameExited(handler: (event: GameExitedEvent) => void): Promise<UnlistenFn>;
  onLauncherError(handler: (event: LauncherErrorEvent) => void): Promise<UnlistenFn>;
}

export interface AppApi extends LauncherApi, RuntimeApi, SettingsApi, ProfileApi, OperationApi {}

type LaunchInvoke = (command: string, args?: Record<string, unknown>) => Promise<OperationId>;

export function invokeLaunchOrInstall(
  profileId: string,
  invokeCommand: LaunchInvoke = invoke,
): Promise<OperationId> {
  return invokeCommand("launch_or_install", { profileId });
}

function listenPayload<T>(eventName: string, handler: (payload: T) => void) {
  return listen<T>(eventName, ({ payload }) => handler(payload));
}

export const appApi: AppApi = {
  ...launcherApi,
  ...runtimeApi,
  ...settingsApi,
  listGameVersions: () => invoke<GameVersionSummary[]>("list_game_versions"),
  requiredJavaForVersion: (versionId) => invoke<JavaMajor>("required_java_for_version", { versionId }),
  getProfile: () => invoke<LauncherProfile>("get_profile"),
  updateProfile: (profile) => invoke<LauncherProfile>("update_profile", { profile }),
  chooseGameDirectory: () => invoke<LauncherProfile | null>("choose_game_directory"),
  launchOrInstall: (profileId) => invokeLaunchOrInstall(profileId),
  cancelOperation: (operationId) => invoke<void>("cancel_operation", { operationId }),
  openLatestGameLog: () => invoke<void>("open_latest_game_log"),
  onProgress: (handler) => listenPayload("launcher://progress", handler),
  onGameStarted: (handler) => listenPayload("launcher://game-started", handler),
  onGameExited: (handler) => listenPayload("launcher://game-exited", handler),
  onLauncherError: (handler) => listenPayload("launcher://error", handler),
};

export interface WindowApi {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
}

export const windowApi: WindowApi = {
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
};
