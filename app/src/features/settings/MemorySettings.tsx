import { useEffect, useState } from "react";

import { settingsApi, type SettingsApi } from "../../app/tauri";
import type { MemorySettingsStatus } from "../../app/types";

interface MemorySettingsProps {
  api?: SettingsApi;
  memoryMb: number;
  onChange(memoryMb: number): void;
  saveState: "idle" | "saving" | "error";
}

export function MemorySettings({
  api = settingsApi,
  memoryMb,
  onChange,
  saveState,
}: MemorySettingsProps) {
  const [status, setStatus] = useState<MemorySettingsStatus>();
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    void api.memoryStatus().then(
      (next) => { if (active) setStatus(next); },
      () => { if (active) setFailed(true); },
    );
    return () => { active = false; };
  }, [api]);

  if (failed) return <p role="alert">Не удалось получить безопасный предел памяти.</p>;
  if (!status) return <p>Определяем безопасный предел памяти…</p>;

  return (
    <section aria-labelledby="memory-settings-title">
      <h2 id="memory-settings-title">Оперативная память</h2>
      <output htmlFor="memory-slider">{memoryMb} МБ</output>
      <input
        aria-label="Оперативная память"
        id="memory-slider"
        max={status.maxMemoryMb}
        min={status.minMemoryMb}
        onChange={(event) => onChange(Number(event.currentTarget.value))}
        step={status.stepMemoryMb}
        type="range"
        value={memoryMb}
      />
      {saveState === "saving" ? <p aria-live="polite">Сохраняем…</p> : null}
      {saveState === "error" ? <p role="alert">Не удалось сохранить память. Значение восстановлено.</p> : null}
    </section>
  );
}
