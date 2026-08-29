import { useEffect, useState } from "react";

import type { AppApi } from "../../app/tauri";
import type { BuildSummary } from "../../app/types";

interface Props {
  api: AppApi;
  onBuildSelected(): Promise<void>;
  onOpenCatalog(): void;
  onPlay(): Promise<void>;
}

export function LibraryPage({ api, onBuildSelected, onOpenCatalog, onPlay }: Props) {
  const [builds, setBuilds] = useState<BuildSummary[]>([]);
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();

  async function reload() {
    setBuilds(await api.listBuilds());
  }

  useEffect(() => { void reload().catch(() => setError("Не удалось открыть библиотеку.")); }, [api]);

  async function select(build: BuildSummary, play = false) {
    setBusy(build.id); setError(undefined);
    try {
      await api.selectBuild(build.id);
      await onBuildSelected();
      await reload();
      if (play) await onPlay();
    } catch (reason) {
      setError(messageFrom(reason));
    } finally {
      setBusy(undefined);
    }
  }

  async function remove(build: BuildSummary) {
    if (!window.confirm(`Переместить сборку «${build.name}» в корзину ЦК Лаунчера?`)) return;
    setBusy(build.id); setError(undefined);
    try {
      await api.deleteBuild(build.id);
      await reload();
      await onBuildSelected();
    } catch (reason) {
      setError(messageFrom(reason));
    } finally {
      setBusy(undefined);
    }
  }

  return (
    <section className="library-page">
      <div className="content-heading">
        <div><span className="eyebrow">УСТАНОВЛЕННОЕ</span><h1>Библиотека</h1><p>Все версии и сборки в одном месте.</p></div>
        <button className="primary-small" onClick={onOpenCatalog} type="button">+ Найти сборку</button>
      </div>
      {error && <div className="catalog-error" role="alert">{error}</div>}
      {builds.length === 0 ? (
        <div className="library-empty"><h2>Библиотека пока пуста</h2><p>Установите модпак из Modrinth или создайте собственную сборку.</p><button onClick={onOpenCatalog} type="button">Открыть каталог</button></div>
      ) : (
        <div className="library-grid">
          {builds.map((build) => <article className={build.isActive ? "library-card active" : "library-card"} key={build.id}>
            {build.iconUrl ? <img alt="" src={build.iconUrl} /> : <span className="library-icon">{build.name.slice(0, 1).toUpperCase()}</span>}
            <div className="library-card-copy"><h2>{build.name}</h2><p>{loaderName(build.loader)} · {baseVersion(build)}</p>{build.isActive && <small>Текущая сборка</small>}</div>
            <div className="library-actions">
              <button disabled={busy === build.id} onClick={() => void select(build, true)} type="button">Играть</button>
              <button disabled={busy === build.id || build.isActive} onClick={() => void select(build)} type="button">Выбрать</button>
              <button className="danger-quiet" disabled={busy === build.id} onClick={() => void remove(build)} type="button">Удалить</button>
            </div>
          </article>)}
        </div>
      )}
    </section>
  );
}

function baseVersion(build: BuildSummary) {
  const parts = build.gameVersion.split("-");
  return build.loader === "fabric" || build.loader === "quilt" ? parts[parts.length - 1] ?? build.gameVersion : build.gameVersion;
}
function loaderName(loader: string) { return loader === "vanilla" ? "Vanilla" : loader.slice(0, 1).toUpperCase() + loader.slice(1); }
function messageFrom(reason: unknown) {
  return reason && typeof reason === "object" && "message" in reason && typeof reason.message === "string"
    ? reason.message
    : "Операция не выполнена.";
}
