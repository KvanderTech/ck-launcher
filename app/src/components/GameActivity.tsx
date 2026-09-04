import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import type { OperationApi } from "../app/tauri";
import type { GameStartedEvent } from "../app/types";

interface GameActivityProps {
  api: Pick<OperationApi, "readLatestGameLog" | "stopGame">;
  game: GameStartedEvent;
  iconUrl?: string;
  name: string;
}

export function GameActivity({ api, game, iconUrl, name }: GameActivityProps) {
  const [consoleOpen, setConsoleOpen] = useState(false);
  const [log, setLog] = useState("");
  const [logError, setLogError] = useState<string>();
  const [stopping, setStopping] = useState(false);
  const outputRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    if (!consoleOpen) return;
    let active = true;

    async function refreshLog() {
      try {
        const nextLog = await api.readLatestGameLog();
        if (!active) return;
        setLog(nextLog);
        setLogError(undefined);
      } catch {
        if (active) setLogError("Ждём первые строки от Minecraft…");
      }
    }

    void refreshLog();
    const timer = window.setInterval(() => void refreshLog(), 750);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [api, consoleOpen, game.operationId]);

  useEffect(() => {
    if (!consoleOpen || !outputRef.current) return;
    outputRef.current.scrollTop = outputRef.current.scrollHeight;
  }, [consoleOpen, log]);

  async function stopGame() {
    if (stopping) return;
    setStopping(true);
    try {
      await api.stopGame(game.operationId);
    } catch {
      setStopping(false);
      setLogError("Не удалось остановить Minecraft. Попробуйте ещё раз.");
      setConsoleOpen(true);
    }
  }

  return (
    <>
      <section className="game-activity" aria-label={`Запущена сборка ${name}`}>
        <span className="game-activity-dot" aria-hidden="true" />
        {iconUrl ? <img alt="" src={iconUrl} /> : <span className="game-activity-mark">ЦК</span>}
        <strong title={name}>{name}</strong>
        <button
          aria-label="Открыть консоль Minecraft"
          className="game-activity-console"
          onClick={() => setConsoleOpen(true)}
          title="Консоль Minecraft"
          type="button"
        >
          <svg aria-hidden="true" viewBox="0 0 24 24"><path d="m7 8 4 4-4 4M13 16h4" /></svg>
        </button>
        <button
          aria-label="Принудительно остановить Minecraft"
          className="game-activity-stop"
          disabled={stopping}
          onClick={() => void stopGame()}
          title="Остановить Minecraft"
          type="button"
        >
          <svg aria-hidden="true" viewBox="0 0 24 24"><rect height="8" rx="1" width="8" x="8" y="8" /></svg>
        </button>
      </section>

      {consoleOpen && createPortal(
        <div className="game-console-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) setConsoleOpen(false); }}>
          <section aria-label="Консоль Minecraft" aria-modal="true" className="game-console" role="dialog">
            <header>
              <div>
                <span className="game-activity-dot" aria-hidden="true" />
                <div><small>Запущена сборка</small><strong>{name}</strong></div>
              </div>
              <div className="game-console-actions">
                <button className="game-console-stop" disabled={stopping} onClick={() => void stopGame()} type="button">
                  <svg aria-hidden="true" viewBox="0 0 24 24"><rect height="8" rx="1" width="8" x="8" y="8" /></svg>
                  {stopping ? "Останавливаем…" : "Остановить игру"}
                </button>
                <button aria-label="Закрыть консоль" className="game-console-close" onClick={() => setConsoleOpen(false)} type="button">
                  <svg aria-hidden="true" viewBox="0 0 24 24"><path d="m7 7 10 10M17 7 7 17" /></svg>
                </button>
              </div>
            </header>
            <div className="game-console-title"><span>Консоль</span><small>Данные обновляются автоматически</small></div>
            <pre ref={outputRef} className="game-console-output">{log || logError || "Minecraft запускается…"}</pre>
            {logError && log && <p className="game-console-error">{logError}</p>}
          </section>
        </div>,
        document.body,
      )}
    </>
  );
}
