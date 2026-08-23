import { windowApi, type WindowApi } from "../app/tauri";

interface WindowControlsProps {
  api?: WindowApi;
}

export function WindowControls({ api = windowApi }: WindowControlsProps) {
  return (
    <div aria-label="Управление окном" className="window-controls" role="group">
      <button aria-label="Свернуть" onClick={() => void api.minimize()} type="button">
        <svg aria-hidden="true" viewBox="0 0 16 16"><path d="M3 8.5h10" /></svg>
      </button>
      <button aria-label="Развернуть" onClick={() => void api.toggleMaximize()} type="button">
        <svg aria-hidden="true" viewBox="0 0 16 16"><rect height="8" rx="1" width="8" x="4" y="4" /></svg>
      </button>
      <button className="window-close" aria-label="Закрыть" onClick={() => void api.close()} type="button">
        <svg aria-hidden="true" viewBox="0 0 16 16"><path d="m4 4 8 8m0-8-8 8" /></svg>
      </button>
    </div>
  );
}
