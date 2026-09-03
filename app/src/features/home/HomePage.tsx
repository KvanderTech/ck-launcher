import type { LauncherErrorDto } from "../../app/types";
import homeRender from "../../assets/home-render.png";

export type LauncherViewState =
  | "ready"
  | "installing"
  | "launching"
  | "running"
  | "recoverable-error"
  | "fatal-error";

interface HomePageProps {
  error?: LauncherErrorDto;
  logPath?: string;
  warning?: LauncherErrorDto;
  onPlay(): void;
  onOpenLog(): void;
  onRetry(): void;
  onOpenExternal(url: string): void;
  state: LauncherViewState;
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
  logPath,
  warning,
  onPlay,
  onOpenLog,
  onRetry,
  onOpenExternal,
  state,
}: HomePageProps) {
  const busy = state === "installing" || state === "launching" || state === "running";

  return (
    <section className="home-page">
      <div className="home-slogan" aria-label="Лаунчер для комфортной игры"><span>Лаунчер для</span><strong>комфортной игры</strong></div>
      <nav aria-label="Социальные сети ЦК" className="home-socials"><button aria-label="Telegram" onClick={() => onOpenExternal("https://t.me/comfortcentr")} type="button"><svg viewBox="0 0 24 24"><path d="M20.7 4.2 3.8 10.7c-1.2.5-1.2 1.2-.2 1.5l4.3 1.4 1.7 5.1c.2.6.1.8.8.8.5 0 .8-.2 1.1-.5l2.1-2 4.4 3.2c.8.4 1.4.2 1.6-.8l2.8-13.3c.3-1.2-.5-2.3-1.7-1.9Z"/><path d="m8 13.5 10.2-6.4-8.4 8.1-.3 3.4"/></svg></button><button aria-label="Discord" onClick={() => onOpenExternal("https://discord.gg/2CkZsVN8nm")} type="button"><svg viewBox="0 0 24 24"><path d="M8.2 6.2a13 13 0 0 1 7.6 0l.8 1.1c2.6.8 3.7 2.8 4.2 8.2a10.5 10.5 0 0 1-4.2 2.2l-1-1.4c.7-.2 1.4-.6 2-1-3.7 1.7-7.5 1.7-11.2 0 .6.4 1.3.8 2 1l-1 1.4a10.5 10.5 0 0 1-4.2-2.2c.5-5.4 1.6-7.4 4.2-8.2l.8-1.1Z"/><circle cx="9" cy="12.5" r="1.2"/><circle cx="15" cy="12.5" r="1.2"/></svg></button><button aria-label="GitHub" onClick={() => onOpenExternal("https://github.com/KvanderTech/ck-launcher")} type="button"><svg viewBox="0 0 24 24"><path d="M12 2.7a9.3 9.3 0 0 0-2.9 18.1c.5.1.6-.2.6-.5v-1.8c-2.6.6-3.1-1.1-3.1-1.1-.4-1.1-1.1-1.4-1.1-1.4-.8-.6.1-.6.1-.6 1 0 1.5 1 1.5 1 .8 1.5 2.2 1 2.7.8.1-.6.3-1 .6-1.2-2.1-.2-4.3-1-4.3-4.6 0-1 .4-1.8 1-2.5-.1-.2-.4-1.2.1-2.5 0 0 .8-.3 2.6 1a9 9 0 0 1 4.8 0c1.8-1.3 2.6-1 2.6-1 .5 1.3.2 2.3.1 2.5.6.7 1 1.5 1 2.5 0 3.6-2.2 4.4-4.3 4.6.4.3.7.9.7 1.7v2.6c0 .3.1.6.7.5A9.3 9.3 0 0 0 12 2.7Z"/></svg></button></nav>
      <img alt="" aria-hidden="true" className="home-character-render" draggable={false} src={homeRender} />
      <span aria-live="polite" className="sr-only">{stateLabels[state]}</span>

      <div className="home-launch-dock">
        <button className="play-button" disabled={busy} onClick={onPlay} type="button">
          {state === "launching" ? "Запускаем…" : state === "running" ? "Игра запущена" : "Играть"}
        </button>
      </div>

      {error ? (
        <section aria-live="assertive" className={`error-panel ${error.recoverable ? "is-recoverable" : "is-fatal"}`} role="alert">
          <div>
            <span className="eyebrow">Ошибка · {error.code}</span>
            <strong>{error.message}</strong>
            {logPath ? <code>{logPath}</code> : null}
          </div>
          <div>
            {error.recoverable ? (
              <button onClick={onRetry} type="button">Повторить</button>
            ) : null}
            {logPath ? (
              <button onClick={onOpenLog} type="button">Открыть очищенный журнал</button>
            ) : (
            <button
              disabled
              title="Открытие журнала будет доступно после добавления безопасной backend-команды."
              type="button"
            >
              Открыть очищенный журнал
            </button>
            )}
          </div>
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
