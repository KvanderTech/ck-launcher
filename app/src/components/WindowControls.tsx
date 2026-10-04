import { KvanthIcon } from "./KvanthIcon";
import { windowApi, type WindowApi } from "../app/tauri";

interface WindowControlsProps {
  api?: WindowApi;
}

export function WindowControls({ api = windowApi }: WindowControlsProps) {
  return (
    <div aria-label="Управление окном" className="window-controls" role="group">
      <button aria-label="Свернуть" onClick={() => void api.minimize()} type="button">
        <KvanthIcon name="minimize" size={18} />
      </button>
      <button aria-label="Развернуть" onClick={() => void api.toggleMaximize()} type="button">
        <KvanthIcon name="maximize" size={18} />
      </button>
      <button className="window-close" aria-label="Закрыть" onClick={() => void api.close()} type="button">
        <KvanthIcon name="close" size={18} />
      </button>
    </div>
  );
}
