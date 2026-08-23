export type OperationId = string;

export type LauncherStage =
  | "idle"
  | "authenticating"
  | "resolving-java"
  | "checking"
  | "downloading"
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

export function progressLabel(progress: ProgressEvent): string {
  const percent = Math.floor((progress.completedBytes / progress.totalBytes) * 100);

  return `Загрузка ${progress.currentFile ?? ""} · ${percent}%`.trim();
}
