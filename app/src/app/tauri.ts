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
  BuildSummary,
  BuildFileEntry,
  BuildWorldSummary,
  BuildLogSummary,
  InstalledContent,
  ModrinthProjectType,
  ModrinthProjectDetails,
  ModrinthVersion,
  ModrinthSearchResult,
  OfflineSkin,
  MinecraftCosmetics,
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
  openExternalUrl(url: string): Promise<void>;
  launchOrInstall(profileId: string): Promise<OperationId>;
  cancelOperation(operationId: OperationId): Promise<void>;
  stopGame(operationId: OperationId): Promise<void>;
  readLatestGameLog(): Promise<string>;
  openLatestGameLog(): Promise<void>;
  onProgress(handler: (event: ProgressEvent) => void): Promise<UnlistenFn>;
  onGameStarted(handler: (event: GameStartedEvent) => void): Promise<UnlistenFn>;
  onGameExited(handler: (event: GameExitedEvent) => void): Promise<UnlistenFn>;
  onLauncherError(handler: (event: LauncherErrorEvent) => void): Promise<UnlistenFn>;
}

export interface ContentApi {
  listBuilds(): Promise<BuildSummary[]>;
  repairBuild(buildId: string): Promise<BuildSummary>;
  createBuild(name: string, gameVersion: string, loader: string): Promise<BuildSummary>;
  selectBuild(buildId: string): Promise<BuildSummary>;
  renameBuild(buildId: string, name: string): Promise<BuildSummary>;
  chooseBuildIcon(buildId: string): Promise<BuildSummary | null>;
  deleteBuild(buildId: string): Promise<void>;
  searchModrinth(query: string, projectType: ModrinthProjectType, gameVersion?: string, loader?: string, offset?: number, category?: string, environment?: string, index?: string): Promise<ModrinthSearchResult>;
  modrinthProject(projectId: string): Promise<ModrinthProjectDetails>;
  modrinthProjectVersions(projectId: string): Promise<ModrinthVersion[]>;
  installModrinthProject(projectId: string, buildId: string, versionId?: string): Promise<InstalledContent>;
  installModrinthModpack(projectId: string, versionId?: string): Promise<InstalledContent>;
  importMrpack(sourcePath?: string): Promise<InstalledContent | null>;
  pendingMrpackPath?(): Promise<string | null>;
  onOpenMrpack?(handler: (path: string) => void): Promise<UnlistenFn>;
  listInstalledContent(buildId: string): Promise<InstalledContent[]>;
  removeInstalledContent(buildId: string, projectId: string): Promise<void>;
  setInstalledContentEnabled(buildId: string, projectId: string, enabled: boolean): Promise<InstalledContent>;
  importLocalContent(buildId: string, projectType: ModrinthProjectType): Promise<InstalledContent[]>;
  openBuildFolder(buildId: string): Promise<void>;
  listBuildFiles(buildId: string, relativePath?: string): Promise<BuildFileEntry[]>;
  listBuildWorlds(buildId: string): Promise<BuildWorldSummary[]>;
  listBuildLogs(buildId: string): Promise<BuildLogSummary[]>;
  readBuildLog(buildId: string, relativePath: string): Promise<string>;
  openBuildPath(buildId: string, relativePath?: string): Promise<void>;
  listOfflineSkins(accountId: string): Promise<OfflineSkin[]>;
  addOfflineSkin(accountId: string): Promise<OfflineSkin | null>;
  deleteOfflineSkin(accountId: string, skinId: string): Promise<void>;
  renameOfflineSkin(accountId: string, skinId: string, name: string): Promise<OfflineSkin>;
  setOfflineSkinFavorite(accountId: string, skinId: string, isFavorite: boolean): Promise<OfflineSkin>;
  selectOfflineSkin(accountId: string, skinId: string): Promise<OfflineSkin>;
  minecraftCosmetics(accountId: string): Promise<MinecraftCosmetics>;
  applyMinecraftSkin(accountId: string, skinId: string, variant: "classic" | "slim"): Promise<MinecraftCosmetics>;
  activateMinecraftCape(accountId: string, capeId?: string): Promise<MinecraftCosmetics>;
}

export interface AppApi extends LauncherApi, RuntimeApi, SettingsApi, ProfileApi, OperationApi, ContentApi {}

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
  stopGame: (operationId) => invoke<void>("stop_game", { operationId }),
  readLatestGameLog: () => invoke<string>("read_latest_game_log"),
  openLatestGameLog: () => invoke<void>("open_latest_game_log"),
  openExternalUrl: (url) => invoke<void>("open_external_url", { url }),
  onProgress: (handler) => listenPayload("launcher://progress", handler),
  onGameStarted: (handler) => listenPayload("launcher://game-started", handler),
  onGameExited: (handler) => listenPayload("launcher://game-exited", handler),
  onLauncherError: (handler) => listenPayload("launcher://error", handler),
  listBuilds: () => invoke<BuildSummary[]>("list_builds"),
  repairBuild: (buildId) => invoke<BuildSummary>("repair_build", { buildId }),
  createBuild: (name, gameVersion, loader) => invoke<BuildSummary>("create_build", { name, gameVersion, loader }),
  selectBuild: (buildId) => invoke<BuildSummary>("select_build", { buildId }),
  renameBuild: (buildId, name) => invoke<BuildSummary>("rename_build", { buildId, name }),
  chooseBuildIcon: (buildId) => invoke<BuildSummary | null>("choose_build_icon", { buildId }),
  deleteBuild: (buildId) => invoke<void>("delete_build", { buildId }),
  searchModrinth: (query, projectType, gameVersion, loader, offset = 0, category, environment, index) => invoke<ModrinthSearchResult>("search_modrinth", { query, projectType, gameVersion, loader, offset, category, environment, index }),
  modrinthProject: (projectId) => invoke<ModrinthProjectDetails>("modrinth_project", { projectId }),
  modrinthProjectVersions: (projectId) => invoke<ModrinthVersion[]>("modrinth_project_versions", { projectId }),
  installModrinthProject: (projectId, buildId, versionId) => invoke<InstalledContent>("install_modrinth_project", { projectId, buildId, versionId }),
  installModrinthModpack: (projectId, versionId) => invoke<InstalledContent>("install_modrinth_modpack", { projectId, versionId }),
  importMrpack: (sourcePath) => invoke<InstalledContent | null>("import_mrpack", { sourcePath }),
  pendingMrpackPath: () => invoke<string | null>("pending_mrpack_path"),
  onOpenMrpack: (handler) => listenPayload("launcher://open-mrpack", handler),
  listInstalledContent: (buildId) => invoke<InstalledContent[]>("list_installed_content", { buildId }),
  removeInstalledContent: (buildId, projectId) => invoke<void>("remove_installed_content", { buildId, projectId }),
  setInstalledContentEnabled: (buildId, projectId, enabled) => invoke<InstalledContent>("set_installed_content_enabled", { buildId, projectId, enabled }),
  importLocalContent: (buildId, projectType) => invoke<InstalledContent[]>("import_local_content", { buildId, projectType }),
  openBuildFolder: (buildId) => invoke<void>("open_build_folder", { buildId }),
  listBuildFiles: (buildId, relativePath = "") => invoke<BuildFileEntry[]>("list_build_files", { buildId, relativePath }),
  listBuildWorlds: (buildId) => invoke<BuildWorldSummary[]>("list_build_worlds", { buildId }),
  listBuildLogs: (buildId) => invoke<BuildLogSummary[]>("list_build_logs", { buildId }),
  readBuildLog: (buildId, relativePath) => invoke<string>("read_build_log", { buildId, relativePath }),
  openBuildPath: (buildId, relativePath = "") => invoke<void>("open_build_path", { buildId, relativePath }),
  listOfflineSkins: (accountId) => invoke<OfflineSkin[]>("list_offline_skins", { accountId }),
  addOfflineSkin: (accountId) => invoke<OfflineSkin | null>("add_offline_skin", { accountId }),
  deleteOfflineSkin: (accountId, skinId) => invoke<void>("delete_offline_skin", { accountId, skinId }),
  renameOfflineSkin: (accountId, skinId, name) => invoke<OfflineSkin>("rename_offline_skin", { accountId, skinId, name }),
  setOfflineSkinFavorite: (accountId, skinId, isFavorite) => invoke<OfflineSkin>("set_offline_skin_favorite", { accountId, skinId, isFavorite }),
  selectOfflineSkin: (accountId, skinId) => invoke<OfflineSkin>("select_offline_skin", { accountId, skinId }),
  minecraftCosmetics: (accountId) => invoke<MinecraftCosmetics>("minecraft_cosmetics", { accountId }),
  applyMinecraftSkin: (accountId, skinId, variant) => invoke<MinecraftCosmetics>("apply_minecraft_skin", { accountId, skinId, variant }),
  activateMinecraftCape: (accountId, capeId) => invoke<MinecraftCosmetics>("activate_minecraft_cape", { accountId, capeId }),
};

export interface WindowApi {
  startDragging(): Promise<void>;
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
}

export const windowApi: WindowApi = {
  startDragging: () => getCurrentWindow().startDragging(),
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
};
