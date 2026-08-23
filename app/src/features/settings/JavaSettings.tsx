import type { JavaMajor, JavaRuntimeStatus } from "../../app/types";

interface JavaSettingsProps {
  statuses: JavaRuntimeStatus[];
  onInstall(major: JavaMajor): void;
  onDetect(major: JavaMajor): void;
  onChoose(major: JavaMajor): void;
}

const labels = {
  valid: "Готова",
  missing: "Не найдена",
  installing: "Устанавливается…",
  invalid: "Не подходит",
} as const;

const sourceLabels = {
  managed: "Управляемая",
  manual: "Выбрана вручную",
  system: "Системная",
} as const;

export function JavaSettings({ statuses, onInstall, onDetect, onChoose }: JavaSettingsProps) {
  return (
    <section aria-label="Настройки Java" className="java-settings">
      {statuses.map((status) => {
        const busy = status.state === "installing";
        return (
          <article className="java-runtime-card" key={status.requirement}>
            <h3>Java {status.requirement}</h3>
            <span className={`runtime-status runtime-${status.state}`}>{labels[status.state]}</span>
            <p>{status.source ? sourceLabels[status.source] : "Путь не выбран"}{status.version ? ` · ${status.version}` : ""}</p>
            <div className="runtime-actions">
              <button aria-label={`Найти Java ${status.requirement}`} disabled={busy} onClick={() => onDetect(status.requirement)} type="button">Найти</button>
              <button aria-label={`Выбрать Java ${status.requirement}`} disabled={busy} onClick={() => onChoose(status.requirement)} type="button">Выбрать</button>
              <button aria-label={`Установить Java ${status.requirement}`} disabled={busy} onClick={() => onInstall(status.requirement)} type="button">Установить</button>
            </div>
          </article>
        );
      })}
    </section>
  );
}
