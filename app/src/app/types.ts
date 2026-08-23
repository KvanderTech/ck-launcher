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

export function progressLabel(progress: ProgressEvent): string {
  const percent = Math.floor((progress.completedBytes / progress.totalBytes) * 100);

  return `Загрузка ${progress.currentFile ?? ""} · ${percent}%`.trim();
}
