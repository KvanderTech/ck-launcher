import type {
  GameVersionSummary,
  JavaMajor,
  JavaRuntimeStatus,
  LauncherErrorDto,
  LauncherProfile,
  ProgressEvent,
} from "../../app/types";
import { ProgressPanel } from "../../components/ProgressPanel";

export type LauncherViewState =
  | "ready"
  | "installing"
  | "launching"
  | "running"
  | "recoverable-error"
  | "fatal-error";

interface HomePageProps {
  cancelling: boolean;
  error?: LauncherErrorDto;
  warning?: LauncherErrorDto;
  onCancel(): void;
  onPlay(): void;
  onRetry(): void;
  onVersionChange(versionId: string): void;
  profile: LauncherProfile;
  progress?: ProgressEvent;
  requiredJava?: JavaMajor;
  runtimes: JavaRuntimeStatus[];
  state: LauncherViewState;
  versions: GameVersionSummary[];
}

const stateLabels: Record<LauncherViewState, string> = {
  ready: "Готово к запуску",
  installing: "Устанавливаем выбранную версию",
  launching: "Подготавливаем запуск",
  running: "Minecraft запущен",
  "recoverable-error": "Можно повторить операцию",
  "fatal-error": "Требуется внимание",
};

export function HomePage({
  cancelling,
  error,
  warning,
  onCancel,
  onPlay,
  onRetry,
  onVersionChange,
  profile,
  progress,
  requiredJava,
  runtimes,
  state,
  versions,
}: HomePageProps) {
  const busy = state === "installing" || state === "launching" || state === "running";
  const requiredRuntime = runtimes.find((runtime) => runtime.requirement === requiredJava);
  const runtimeLabel = requiredJava === undefined
    ? "Определяем Java…"
    : requiredRuntime?.state === "valid"
      ? `Java ${requiredJava} готова`
      : requiredRuntime?.state === "installing"
        ? `Java ${requiredJava} устанавливается`
        : requiredRuntime?.state === "invalid"
          ? `Java ${requiredJava} не подходит`
          : `Java ${requiredJava} не найдена`;

  return (
    <section className="home-page">
      <div className="hero-copy">
        <span className="eyebrow">Vanilla Minecraft</span>
        <h1>С возвращением в ваш мир.</h1>
        <p>Выберите официальный релиз, проверьте готовность профиля и запускайте игру в одном спокойном пространстве.</p>
      </div>

      <div className="launch-grid">
        <div className="profile-card">
          <label htmlFor="game-version">Версия Minecraft</label>
          <select
            id="game-version"
            onChange={(event) => onVersionChange(event.currentTarget.value)}
            value={profile.versionId ?? ""}
          >
            <option disabled value="">Выберите стабильный релиз</option>
            {versions.map((version) => <option key={version.id} value={version.id}>{version.id}</option>)}
          </select>
          <div className="profile-facts">
            <span><small>Профиль</small><strong>{profile.name}</strong></span>
            <span><small>Память</small><strong>{profile.memoryMb} МБ</strong></span>
            <span><small>Java</small><strong>{runtimeLabel}</strong></span>
          </div>
        </div>
        <div className="play-card">
          <span className={`state-indicator state-${state}`} aria-hidden="true" />
          <span aria-live="polite" className="launch-state">{stateLabels[state]}</span>
          <button
            className="play-button"
            disabled={busy || !profile.versionId}
            onClick={onPlay}
            type="button"
          >
            {state === "launching" ? "Запускаем…" : state === "running" ? "Игра запущена" : "Играть"}
          </button>
        </div>
      </div>

      {progress && (state === "installing" || state === "launching") ? (
        <ProgressPanel cancelling={cancelling} onCancel={state === "installing" ? onCancel : undefined} progress={progress} />
      ) : null}

      {error ? (
        <section aria-live="assertive" className={`error-panel ${error.recoverable ? "is-recoverable" : "is-fatal"}`} role="alert">
          <div>
            <span className="eyebrow">Ошибка · {error.code}</span>
            <strong>{error.message}</strong>
          </div>
          {error.recoverable ? (
            <button onClick={onRetry} type="button">Повторить</button>
          ) : (
            <button
              disabled
              title="Открытие журнала будет доступно после добавления безопасной backend-команды."
              type="button"
            >
              Открыть очищенный журнал
            </button>
          )}
        </section>
      ) : null}

      {warning ? (
        <section aria-live="polite" className="error-panel is-recoverable" role="status">
          <div>
            <span className="eyebrow">Предупреждение · {warning.code}</span>
            <strong>{warning.message}</strong>
          </div>
        </section>
      ) : null}
    </section>
  );
}
