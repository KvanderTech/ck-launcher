import { useEffect, useMemo, useState } from "react";
import type { AppApi } from "../../app/tauri";
import type { BuildSummary, GameVersionSummary, InstalledContent, ModrinthProject, ModrinthProjectType } from "../../app/types";

const tabs: Array<{ id: ModrinthProjectType; label: string }> = [
  { id: "modpack", label: "Сборки" },
  { id: "mod", label: "Моды" },
  { id: "resourcepack", label: "Ресурспаки" },
  { id: "shader", label: "Шейдеры" },
];

interface Props {
  api: AppApi;
  versions: GameVersionSummary[];
  onBuildSelected(): Promise<void> | void;
}

export function ContentPage({ api, versions, onBuildSelected }: Props) {
  const [type, setType] = useState<ModrinthProjectType>("modpack");
  const [query, setQuery] = useState("");
  const [gameVersion, setGameVersion] = useState(versions[0]?.id ?? "1.21.1");
  const [loader, setLoader] = useState("fabric");
  const [projects, setProjects] = useState<ModrinthProject[]>([]);
  const [builds, setBuilds] = useState<BuildSummary[]>([]);
  const [installed, setInstalled] = useState<InstalledContent[]>([]);
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("Моя сборка");

  const activeBuild = useMemo(() => builds.find((item) => item.isActive) ?? builds[0], [builds]);

  async function reloadBuilds() {
    const next = await api.listBuilds();
    setBuilds(next);
    const active = next.find((item) => item.isActive) ?? next[0];
    setInstalled(active ? await api.listInstalledContent(active.id) : []);
  }

  async function search() {
    setBusy("search"); setError(undefined);
    try {
      const result = await api.searchModrinth(query, type, gameVersion, type === "resourcepack" || type === "shader" ? undefined : loader, 0);
      setProjects(result.hits);
    } catch { setError("Не удалось загрузить каталог Modrinth. Проверьте интернет и повторите."); }
    finally { setBusy(undefined); }
  }

  useEffect(() => { void reloadBuilds().catch(() => setError("Не удалось загрузить список сборок.")); }, []);
  useEffect(() => { void search(); }, [type, gameVersion, loader]);

  async function create() {
    setBusy("create"); setError(undefined);
    try {
      await api.createBuild(newName, gameVersion, loader);
      await reloadBuilds(); await onBuildSelected(); setCreating(false);
    } catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  }

  async function select(build: BuildSummary) {
    setBusy(build.id); setError(undefined);
    try { await api.selectBuild(build.id); await reloadBuilds(); await onBuildSelected(); }
    catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  }

  async function install(project: ModrinthProject) {
    if (!activeBuild) { setCreating(true); setError("Сначала создайте сборку, в которую будет установлен контент."); return; }
    setBusy(project.project_id); setError(undefined);
    try { await api.installModrinthProject(project.project_id, activeBuild.id); await reloadBuilds(); await onBuildSelected(); }
    catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  }

  async function remove(item: InstalledContent) {
    if (!activeBuild) return;
    setBusy(item.projectId);
    try { await api.removeInstalledContent(activeBuild.id, item.projectId); await reloadBuilds(); }
    catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  }

  return (
    <section className="content-page">
      <div className="content-heading">
        <div><span className="eyebrow">MODRINTH · ЕДИНЫЙ КАТАЛОГ</span><h1>Контент и сборки</h1><p>Ищите, устанавливайте и запускайте контент из одного места.</p></div>
        <button className="primary-small" onClick={() => setCreating((value) => !value)} type="button">+ Создать сборку</button>
      </div>

      <div className="build-strip">
        {builds.length === 0 ? <span className="empty-inline">Сборок пока нет</span> : builds.map((build) => (
          <button className={build.isActive ? "build-chip active" : "build-chip"} key={build.id} onClick={() => void select(build)} type="button">
            {build.iconUrl ? <img alt="" src={build.iconUrl} /> : <span>{build.name.slice(0, 1).toUpperCase()}</span>}
            <b>{build.name}</b><small>{build.loader} · {baseVersion(build)}</small>
          </button>
        ))}
      </div>

      {creating && <div className="create-build-card">
        <label>Название<input value={newName} onChange={(event) => setNewName(event.target.value)} /></label>
        <label>Версия<select value={gameVersion} onChange={(event) => setGameVersion(event.target.value)}>{versions.map((version) => <option key={version.id}>{version.id}</option>)}</select></label>
        <label>Загрузчик<select value={loader} onChange={(event) => setLoader(event.target.value)}><option value="fabric">Fabric</option><option value="vanilla">Vanilla</option></select></label>
        <button disabled={busy === "create"} onClick={() => void create()} type="button">{busy === "create" ? "Создаём…" : "Создать"}</button>
      </div>}

      <div className="catalog-tabs">{tabs.map((tab) => <button className={type === tab.id ? "active" : ""} key={tab.id} onClick={() => setType(tab.id)} type="button">{tab.label}</button>)}</div>
      <form className="catalog-search" onSubmit={(event) => { event.preventDefault(); void search(); }}>
        <input aria-label="Поиск Modrinth" placeholder={`Поиск: ${tabs.find((tab) => tab.id === type)?.label.toLowerCase()}…`} value={query} onChange={(event) => setQuery(event.target.value)} />
        <select aria-label="Версия Minecraft" value={gameVersion} onChange={(event) => setGameVersion(event.target.value)}>{versions.slice(0, 40).map((version) => <option key={version.id}>{version.id}</option>)}</select>
        {type !== "resourcepack" && type !== "shader" && <select aria-label="Загрузчик" value={loader} onChange={(event) => setLoader(event.target.value)}><option value="fabric">Fabric</option><option value="vanilla">Vanilla</option></select>}
        <button type="submit">Найти</button>
      </form>
      {error && <div className="catalog-error" role="alert">{error}</div>}

      <div className="content-layout">
        <div className="project-list" aria-busy={busy === "search"}>
          {busy === "search" && projects.length === 0 ? <div className="catalog-empty">Загружаем Modrinth…</div> : projects.map((project) => {
            const already = installed.some((item) => item.projectId === project.project_id);
            return <article className="project-card" key={project.project_id}>
              {project.icon_url ? <img alt="" src={project.icon_url} /> : <span className="project-icon">{project.title.slice(0, 1)}</span>}
              <div><h2>{project.title} <small>от {project.author}</small></h2><p>{project.description}</p><div className="project-tags">{project.categories.slice(0, 4).map((category) => <span key={category}>{category}</span>)}<span>↓ {compact(project.downloads)}</span></div></div>
              <button disabled={already || busy === project.project_id} onClick={() => void install(project)} type="button">{already ? "Установлено" : busy === project.project_id ? "Установка…" : "+ Установить"}</button>
            </article>;
          })}
          {!busy && projects.length === 0 && <div className="catalog-empty">По вашему запросу ничего не найдено.</div>}
        </div>
        <aside className="installed-panel"><h2>В сборке</h2><strong>{activeBuild?.name ?? "Сборка не выбрана"}</strong><div>{installed.map((item) => <div className="installed-row" key={item.id}>{item.iconUrl ? <img alt="" src={item.iconUrl} /> : <span /> }<b>{item.title}</b><button aria-label={`Удалить ${item.title}`} onClick={() => void remove(item)} type="button">×</button></div>)}{installed.length === 0 && <p>Установленного контента пока нет.</p>}</div></aside>
      </div>
    </section>
  );
}

function baseVersion(build: BuildSummary) { const parts = build.gameVersion.split("-"); return build.loader === "fabric" ? parts[parts.length - 1] : build.gameVersion; }
function compact(value: number) { return new Intl.NumberFormat("ru", { notation: "compact", maximumFractionDigits: 1 }).format(value); }
function errorMessage(reason: unknown) { if (reason && typeof reason === "object" && "message" in reason && typeof reason.message === "string") return reason.message; return "Операция не выполнена."; }
