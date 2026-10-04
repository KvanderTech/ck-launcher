import { KvanthIcon } from "../../components/KvanthIcon";
import type { LauncherErrorDto } from "../../app/types";
import { isWindows11Edition } from "../../app/edition";
import { createPortal } from "react-dom";
import homeRender from "../../assets/home-render.png";
import kvanthWordmark from "../../assets/kvanth-wordmark.png";

export type LauncherViewState =
  | "ready"
  | "installing"
  | "launching"
  | "running"
  | "recoverable-error"
  | "fatal-error";

interface HomePageProps {
  buildName?: string;
  error?: LauncherErrorDto;
  logPath?: string;
  warning?: LauncherErrorDto;
  onPlay(): void;
  onOpenLog(): void;
  onRetry(): void;
  onDismissError(): void;
  onOpenExternal(url: string): void;
  state: LauncherViewState;
  versionLabel?: string;
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
  onDismissError,
  onOpenExternal,
  state,
}: HomePageProps) {
  const busy = state === "installing" || state === "launching" || state === "running";

  return (
    <section className="home-page">
      <div className="home-copy-block">
        {isWindows11Edition
          ? <img alt="Kvanth Launcher" className="home-wordmark" draggable={false} src={kvanthWordmark} />
          : <div className="home-slogan" aria-label="Лаунчер для комфортной игры"><span>Лаунчер для</span><strong>комфортной игры</strong></div>}
        <nav aria-label="Социальные сети ЦК" className="home-socials"><button aria-label="Telegram" onClick={() => onOpenExternal("https://t.me/kvanth_launcher")} type="button"><KvanthIcon name="telegram" size={24} /></button><button aria-label="Discord" onClick={() => onOpenExternal("https://discord.gg/2CkZsVN8nm")} type="button"><KvanthIcon name="discord" size={24} /></button><button aria-label="GitHub" onClick={() => onOpenExternal("https://github.com/KvanderTech/ck-launcher")} type="button"><KvanthIcon name="github" size={24} /></button></nav>
      </div>
      <img alt="" aria-hidden="true" className="home-character-render" draggable={false} src={homeRender} />
      <span aria-live="polite" className="sr-only">{stateLabels[state]}</span>

      <div className="home-launch-dock">
        <button className="play-button" disabled={!isWindows11Edition && busy} onClick={onPlay} type="button">
          {isWindows11Edition ? <KvanthIcon name="play" size={20} /> : null}
          <span>{isWindows11Edition ? "Играть" : state === "launching" ? "Запускаем…" : state === "running" ? "Игра запущена" : "Играть"}</span>
        </button>
      </div>

      {error ? (isWindows11Edition ? createPortal(<div className="home-error-overlay" onMouseDown={(event) => { if (event.target === event.currentTarget) onDismissError(); }}><section aria-live="assertive" className={`error-panel ${error.recoverable ? "is-recoverable" : "is-fatal"}`} role="alert"><div><span className="eyebrow">Ошибка · {error.code}</span><strong>{error.message}</strong>{logPath ? <code>{logPath}</code> : null}</div><div>{error.recoverable && <button onClick={onRetry} type="button"><KvanthIcon name="refresh" size={18} /> Повторить</button>}{logPath && <button onClick={onOpenLog} type="button"><KvanthIcon name="console" size={18} /> Открыть журнал</button>}<button aria-label="Закрыть ошибку" onClick={onDismissError} type="button"><KvanthIcon name="close" size={18} /></button></div></section></div>, document.body) : (
        <section aria-live="assertive" className={`error-panel ${error.recoverable ? "is-recoverable" : "is-fatal"}`} role="alert">
          <div>
            <span className="eyebrow">Ошибка · {error.code}</span>
            <strong>{error.message}</strong>
            {logPath ? <code>{logPath}</code> : null}
          </div>
          <div>
            {error.recoverable ? (
              <button onClick={onRetry} type="button"><KvanthIcon name="refresh" size={18} /> Повторить</button>
            ) : null}
            {logPath ? (
              <button onClick={onOpenLog} type="button"><KvanthIcon name="console" size={18} /> Открыть очищенный журнал</button>
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
        </section>)
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
