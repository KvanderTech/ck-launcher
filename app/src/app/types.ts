export type OperationId = string;

export type LauncherStage =
  | "idle"
  | "authenticating"
  | "resolving-metadata"
  | "resolving-java"
  | "checking"
  | "downloading"
  | "installing"
  | "launching"
  | "running"
  | "failed";

export interface ProgressEvent {
  operationId: OperationId;
  stage: LauncherStage;
  completedBytes: number;
  totalBytes: number;
  currentFile?: string;
}

export interface LauncherErrorDto {
  code: string;
  message: string;
  details?: string;
  recoverable: boolean;
}

export interface AccountSummary {
  id: string;
  minecraftName: string;
  minecraftUuid: string;
  headUrl?: string;
  isActive: boolean;
}

export type JavaMajor = 8 | 17 | 21 | 25;
export type JavaRuntimeState = "valid" | "missing" | "installing" | "invalid";
export type JavaRuntimeSource = "managed" | "manual" | "system";

export interface JavaRuntimeStatus {
  requirement: JavaMajor;
  state: JavaRuntimeState;
  path?: string;
  source?: JavaRuntimeSource;
  version?: string;
}

export interface MemorySettingsStatus {
  memoryMb: number;
  minMemoryMb: number;
  maxMemoryMb: number;
  stepMemoryMb: number;
}

export interface GameVersionSummary {
  id: string;
  type: string;
  releaseDate: string;
}

export interface LauncherProfile {
  id: string;
  name: string;
  versionId: string | null;
  memoryMb: number;
  gameDir: string;
  javaOverride: string | null;
}

export interface BuildSummary {
  id: string;
  name: string;
  gameVersion: string;
  loader: "vanilla" | "fabric" | string;
  loaderVersion?: string;
  gameDir: string;
  iconUrl?: string;
  isActive: boolean;
}

export interface BuildFileEntry {
  name: string;
  relativePath: string;
  kind: "file" | "directory";
  size: number;
  modifiedAt: number;
}

export interface BuildWorldSummary {
  name: string;
  relativePath: string;
  size: number;
  modifiedAt: number;
}

export interface BuildLogSummary {
  name: string;
  relativePath: string;
  size: number;
  modifiedAt: number;
}

export type ModrinthProjectType = "modpack" | "mod" | "resourcepack" | "shader";

export interface ModrinthProject {
  project_id: string;
  project_type: ModrinthProjectType;
  title: string;
  description: string;
  author: string;
  categories: string[];
  versions: string[];
  downloads: number;
  follows: number;
  icon_url?: string;
  date_modified: string;
}

export interface ModrinthSearchResult {
  hits: ModrinthProject[];
  offset: number;
  limit: number;
  total_hits: number;
}

export interface ModrinthProjectDetails {
  id: string; title: string; project_type: ModrinthProjectType; icon_url?: string;
  description: string; body: string; downloads: number; followers: number; categories: string[];
}

export interface ModrinthVersion {
  id: string; name: string; version_number: string; version_type: string;
  date_published: string; downloads: number; loaders: string[]; game_versions: string[];
}

export interface InstalledContent {
  id: string;
  buildId: string;
  projectId: string;
  versionId: string;
  projectType: ModrinthProjectType;
  title: string;
  filename: string;
  iconUrl?: string;
  enabled: boolean;
}

export interface OfflineSkin {
  id: string;
  accountId: string;
  name: string;
  dataUrl: string;
  isActive: boolean;
}

export interface MinecraftSkin {
  id: string;
  state: string;
  url: string;
  variant: "CLASSIC" | "SLIM" | string;
}

export interface MinecraftCape {
  id: string;
  state: string;
  url: string;
  alias: string;
}

export interface MinecraftCosmetics {
  id: string;
  name: string;
  skins: MinecraftSkin[];
  capes: MinecraftCape[];
}

export interface GameStartedEvent {
  kind?: "started";
  operationId: OperationId;
  profileId: string;
  pid: number;
}

export interface GameExitedEvent {
  kind?: "exited";
  operationId: OperationId;
  profileId: string;
  exitCode: number;
}

interface ErrorEventBase {
  kind?: "error";
  operationId: OperationId;
  profileId: string;
  error: LauncherErrorDto;
  logPath?: string;
}

export interface ProcessErrorEvent extends ErrorEventBase {
  terminal: boolean;
}

export interface WorkflowErrorEvent extends ErrorEventBase {
  stage: Exclude<LauncherStage, "idle" | "running" | "failed">;
  terminal?: never;
}

export type LauncherErrorEvent = ProcessErrorEvent | WorkflowErrorEvent;

export function progressLabel(progress: ProgressEvent): string {
  const percent = progress.totalBytes > 0
    ? Math.floor((progress.completedBytes / progress.totalBytes) * 100)
    : 0;

  return `Загрузка ${progress.currentFile ?? ""} · ${percent}%`.trim();
}
