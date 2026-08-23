import { useEffect, useRef, useState } from "react";

import { settingsApi, type SettingsApi } from "../../app/tauri";
import type { MemorySettingsStatus } from "../../app/types";

interface MemorySettingsProps {
  api?: SettingsApi;
  onSaved(memoryMb: number): void;
}

export function MemorySettings({ api = settingsApi, onSaved }: MemorySettingsProps) {
  const [status, setStatus] = useState<MemorySettingsStatus>();
  const [failed, setFailed] = useState(false);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "error">("idle");
  const savedMemory = useRef<number | undefined>(undefined);
  const timer = useRef<number | undefined>(undefined);
  const request = useRef(0);
  const mounted = useRef(true);

  useEffect(() => {
    let active = true;
    mounted.current = true;
    void api.memoryStatus().then(
      (next) => {
        if (active) {
          savedMemory.current = next.memoryMb;
          setStatus(next);
        }
      },
      () => { if (active) setFailed(true); },
    );
    return () => {
      active = false;
      mounted.current = false;
      if (timer.current !== undefined) window.clearTimeout(timer.current);
    };
  }, [api]);

  if (failed) return <p role="alert">Не удалось получить безопасный предел памяти.</p>;
  if (!status) return <p>Определяем безопасный предел памяти…</p>;

  function changeMemory(memoryMb: number) {
    setStatus((current) => current ? { ...current, memoryMb } : current);
    setSaveState("idle");
    if (timer.current !== undefined) window.clearTimeout(timer.current);
    const currentRequest = ++request.current;
    timer.current = window.setTimeout(() => {
      setSaveState("saving");
      void api.updateMemory(memoryMb).then(
        (saved) => {
          if (request.current !== currentRequest) return;
          savedMemory.current = saved.memoryMb;
          if (mounted.current) {
            setStatus((current) => current ? { ...current, memoryMb: saved.memoryMb } : current);
            setSaveState("idle");
          }
          onSaved(saved.memoryMb);
        },
        () => {
          if (request.current !== currentRequest || !mounted.current) return;
          setStatus((current) => current && savedMemory.current !== undefined
            ? { ...current, memoryMb: savedMemory.current }
            : current);
          setSaveState("error");
        },
      );
    }, 250);
  }

  return (
    <section aria-labelledby="memory-settings-title">
      <h2 id="memory-settings-title">Оперативная память</h2>
      <output htmlFor="memory-slider">{status.memoryMb} МБ</output>
      <input
        aria-label="Оперативная память"
        id="memory-slider"
        max={status.maxMemoryMb}
        min={status.minMemoryMb}
        onChange={(event) => changeMemory(Number(event.currentTarget.value))}
        step={status.stepMemoryMb}
        type="range"
        value={status.memoryMb}
      />
      {saveState === "saving" ? <p aria-live="polite">Сохраняем…</p> : null}
      {saveState === "error" ? <p role="alert">Не удалось сохранить память. Значение восстановлено.</p> : null}
    </section>
  );
}
