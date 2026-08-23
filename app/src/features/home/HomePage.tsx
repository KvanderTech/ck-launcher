import type {
  GameVersionSummary,
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
  error?: LauncherErrorDto;
  onCancel(): void;
  onPlay(): void;
  onRetry(): void;
  onVersionChange(versionId: string): void;
  profile: LauncherProfile;
  progress?: ProgressEvent;
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
  error,
  onCancel,
  onPlay,
  onRetry,
  onVersionChange,
  profile,
  progress,
  runtimes,
  state,
  versions,
}: HomePageProps) {
  const busy = state === "installing" || state === "launching" || state === "running";
  const validRuntime = runtimes.find((runtime) => runtime.state === "valid");

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
            <span><small>Java</small><strong>{validRuntime ? `Java ${validRuntime.requirement} готова` : "Нужно настроить"}</strong></span>
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
        <ProgressPanel onCancel={state === "installing" ? onCancel : undefined} progress={progress} />
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
    </section>
  );
}
