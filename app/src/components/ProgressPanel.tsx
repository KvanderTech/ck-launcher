import type { ProgressEvent } from "../app/types";

interface ProgressPanelProps {
  progress: ProgressEvent;
  cancelling?: boolean;
  onCancel?: () => void;
}

const stageLabels: Record<ProgressEvent["stage"], string> = {
  idle: "Ожидание",
  authenticating: "Проверяем аккаунт",
  "resolving-java": "Подбираем Java",
  checking: "Проверяем файлы",
  downloading: "Загружаем файлы",
  launching: "Запускаем игру",
  running: "Игра запущена",
  failed: "Операция остановлена",
};

function formatBytes(value: number) {
  if (value < 1024) return `${value} Б`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} КБ`;
  return `${(value / (1024 * 1024)).toFixed(1)} МБ`;
}

export function ProgressPanel({ progress, cancelling = false, onCancel }: ProgressPanelProps) {
  const percent = progress.totalBytes > 0
    ? Math.min(100, Math.floor((progress.completedBytes / progress.totalBytes) * 100))
    : 0;
  const canCancel = progress.stage === "downloading" && Boolean(onCancel);

  return (
    <section aria-live="polite" className="progress-panel">
      <div className="progress-copy">
        <div>
          <span className="eyebrow">{stageLabels[progress.stage]}</span>
          <strong>{progress.currentFile ?? "Подготавливаем операцию…"}</strong>
        </div>
        <output aria-label="Прогресс операции">{percent}%</output>
      </div>
      <div
        aria-label={`${percent}%`}
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={percent}
        className="progress-track"
        role="progressbar"
      >
        <span style={{ width: `${percent}%` }} />
      </div>
      <div className="progress-meta">
        <span>{formatBytes(progress.completedBytes)} из {formatBytes(progress.totalBytes)}</span>
        {canCancel ? (
          <button disabled={cancelling} onClick={onCancel} type="button">
            {cancelling ? "Отменяем…" : "Отменить"}
          </button>
        ) : null}
      </div>
    </section>
  );
}
