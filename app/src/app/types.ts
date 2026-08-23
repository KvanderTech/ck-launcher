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
