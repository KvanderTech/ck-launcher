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

export function JavaSettings({ statuses, onInstall, onDetect, onChoose }: JavaSettingsProps) {
  return (
    <section aria-label="Настройки Java" className="java-settings">
      {statuses.map((status) => {
        const busy = status.state === "installing";
        return (
          <article className="java-runtime-card" key={status.requirement}>
            <h3>Java {status.requirement}</h3>
            <p>{labels[status.state]}</p>
            {status.path ? <p title={status.path}>{status.path}</p> : null}
            <div>
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
