import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { AppApi } from "../../app/tauri";
import type { BuildSummary, GameVersionSummary, InstalledContent, ModrinthProject, ModrinthProjectDetails, ModrinthProjectType, ModrinthVersion } from "../../app/types";
import { playSound } from "../../components/SoundEffects";

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
  installTask?: ContentInstallTask;
  onInstallTaskChange(task?: ContentInstallTask): void;
  forBuild?: boolean;
  startCreating?: boolean;
}

export interface ContentInstallTask { project: ModrinthProject; stage: string; step: number; error?: string; }

export function ContentPage({ api, versions, onBuildSelected, installTask, onInstallTaskChange, forBuild = false, startCreating = false }: Props) {
  const [type, setType] = useState<ModrinthProjectType>(forBuild ? "mod" : "modpack");
  const [query, setQuery] = useState("");
  const [gameVersion, setGameVersion] = useState(versions[0]?.id ?? "1.21.1");
  const [loader, setLoader] = useState("fabric");
  const [category, setCategory] = useState("");
  const [environment, setEnvironment] = useState("");
  const [sort, setSort] = useState("relevance");
  const [hideInstalled, setHideInstalled] = useState(false);
  const [projects, setProjects] = useState<ModrinthProject[]>([]);
  const [page, setPage] = useState(0);
  const [totalHits, setTotalHits] = useState(0);
  const [builds, setBuilds] = useState<BuildSummary[]>([]);
  const [installed, setInstalled] = useState<InstalledContent[]>([]);
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();
  const [creating, setCreating] = useState(startCreating);
  const [newName, setNewName] = useState("Моя сборка");
  const searchRequest = useRef(0);
  const [selectedProject, setSelectedProject] = useState<ModrinthProject>();
  const [projectDetails, setProjectDetails] = useState<ModrinthProjectDetails>();
  const [projectVersions, setProjectVersions] = useState<ModrinthVersion[]>([]);
  const [detailTab, setDetailTab] = useState<"description" | "versions">("description");
  const [versionGameFilter, setVersionGameFilter] = useState("");
  const [versionLoaderFilter, setVersionLoaderFilter] = useState("");

  const activeBuild = useMemo(() => builds.find((item) => item.isActive) ?? builds[0], [builds]);
  const versionGameOptions = useMemo(() => [...new Set(projectVersions.flatMap((version) => version.game_versions))].sort(compareMinecraftVersions), [projectVersions]);
  const versionLoaderOptions = useMemo(() => [...new Set(projectVersions.flatMap((version) => version.loaders))].sort(), [projectVersions]);
  const filteredProjectVersions = useMemo(() => projectVersions.filter((version) => (!versionGameFilter || version.game_versions.includes(versionGameFilter)) && (!versionLoaderFilter || version.loaders.includes(versionLoaderFilter))), [projectVersions, versionGameFilter, versionLoaderFilter]);
  const defaultGameVersion = versions[0]?.id ?? "1.21.1";
  const pageCount = Math.max(1, Math.ceil(totalHits / 20));

  function resetFilters() {
    setHideInstalled(false); setSort("relevance"); setCategory(""); setEnvironment("");
    setGameVersion(""); setLoader("");
  }

  async function reloadBuilds() {
    const next = await api.listBuilds();
    setBuilds(next);
    const active = next.find((item) => item.isActive) ?? next[0];
    if (active) {
      setLoader(active.loader);
      setGameVersion(baseVersion(active));
    }
    setInstalled(active ? await api.listInstalledContent(active.id) : []);
  }

  async function search(targetPage = page) {
    const request = ++searchRequest.current;
    setBusy("search"); setError(undefined);
    try {
      const result = await api.searchModrinth(query, type, gameVersion || undefined, type === "resourcepack" || type === "shader" ? undefined : loader || undefined, targetPage * 20, category, environment, sort);
      if (request === searchRequest.current) { setProjects(result.hits); setTotalHits(result.total_hits); setPage(targetPage); }
    } catch { if (request === searchRequest.current) setError("Не удалось загрузить каталог Modrinth. Проверьте интернет и повторите."); }
    finally { if (request === searchRequest.current) setBusy(undefined); }
  }

  useEffect(() => { void reloadBuilds().catch(() => setError("Не удалось загрузить список сборок.")); }, []);
  useEffect(() => { void search(0); }, [type, gameVersion, loader, category, environment, sort]);

  async function create() {
    setBusy("create"); setError(undefined);
    try {
      await api.createBuild(newName, gameVersion || defaultGameVersion, loader || "fabric");
      await reloadBuilds(); await onBuildSelected(); setCreating(false);
    } catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  }

  async function select(build: BuildSummary) {
    setBusy(build.id); setError(undefined);
    try { await api.selectBuild(build.id); playSound("build-switch"); await reloadBuilds(); await onBuildSelected(); }
    catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  }

  async function openProject(project: ModrinthProject) {
    setSelectedProject(project); setProjectDetails(undefined); setProjectVersions([]); setDetailTab("description"); setVersionGameFilter(""); setVersionLoaderFilter(""); setError(undefined);
    try {
      const [details, releases] = await Promise.all([api.modrinthProject(project.project_id), api.modrinthProjectVersions(project.project_id)]);
      setProjectDetails(details); setProjectVersions(releases);
    } catch (reason) { setError(errorMessage(reason)); }
  }

  async function install(project: ModrinthProject, versionId?: string) {
    if (installTask) return;
    setBusy(project.project_id); setError(undefined);
    onInstallTaskChange({ project, stage: "Загрузка контента", step: 1 });
    let succeeded = false;
    let installStep = 1;
    const phaseTimer = window.setInterval(() => {
      installStep = Math.min(3, installStep + 1);
      onInstallTaskChange({ project, stage: installStep === 2 ? "Подготовка Minecraft" : "Настройка загрузчика", step: installStep });
    }, 6500);
    try {
      if (project.project_type === "modpack") {
        await api.installModrinthModpack(project.project_id, versionId);
      } else {
        let targetBuild = activeBuild;
        if (!targetBuild) {
          const requiredLoader = project.project_type === "mod"
            ? (loader === "vanilla" ? "fabric" : loader)
            : "vanilla";
          targetBuild = await api.createBuild(
            `${project.title} — ${gameVersion}`,
            gameVersion,
            requiredLoader,
          );
        }
        await api.installModrinthProject(project.project_id, targetBuild.id, versionId);
      }
      await reloadBuilds(); await onBuildSelected();
      if (project.project_type === "modpack") playSound("install-complete");
      succeeded = true;
    }
    catch (reason) { const message = errorMessage(reason); setError(message); onInstallTaskChange({ project, stage: "Установка не завершена", step: 0, error: message }); }
    finally { window.clearInterval(phaseTimer); setBusy(undefined); if (succeeded) onInstallTaskChange(undefined); }
  }

  async function importMrpack() {
    if (installTask) return;
    const placeholder: ModrinthProject = { project_id: "local-mrpack", project_type: "modpack", title: "Сборка из файла", description: "Импорт локального файла .mrpack", author: "Локальный файл", categories: [], versions: [], downloads: 0, follows: 0, date_modified: "" };
    setBusy("mrpack"); setError(undefined);
    onInstallTaskChange({ project: placeholder, stage: "Выбор и установка файла", step: 1 });
    try {
      const result = await api.importMrpack();
      if (!result) { onInstallTaskChange(undefined); return; }
      playSound("install-complete");
      onInstallTaskChange({ project: { ...placeholder, title: result.title }, stage: "Сборка установлена", step: 3 });
      await reloadBuilds(); await onBuildSelected();
      window.setTimeout(() => onInstallTaskChange(undefined), 1200);
    } catch (reason) {
      const message = errorMessage(reason);
      setError(message);
      onInstallTaskChange({ project: placeholder, stage: "Установка не завершена", step: 0, error: message });
    } finally { setBusy(undefined); }
  }

  if (selectedProject) {
    const already = installed.some((item) => item.projectId === selectedProject.project_id);
    return <section className="content-page project-detail-page">
      <button className="project-back" onClick={() => setSelectedProject(undefined)} type="button">← Назад в каталог</button>
      <header className="project-detail-hero">
        {selectedProject.icon_url ? <img alt="" src={selectedProject.icon_url} /> : <span>{selectedProject.title[0]}</span>}
        <div><span className="eyebrow">MODRINTH · {typeLabel(selectedProject.project_type)}</span><h1>{selectedProject.title}</h1><p>{projectDetails?.description ?? selectedProject.description}</p><div className="project-tags">{selectedProject.categories.slice(0, 5).map((category) => <span key={category}>{category}</span>)}<span>↓ {compact(projectDetails?.downloads ?? selectedProject.downloads)}</span></div></div>
        <button disabled={already || Boolean(installTask)} onClick={() => void install(selectedProject)} type="button">{already ? "Установлено" : "+ Установить"}</button>
      </header>
      <nav className="project-detail-tabs"><button className={detailTab === "description" ? "active" : ""} onClick={() => setDetailTab("description")} type="button">Описание</button><button className={detailTab === "versions" ? "active" : ""} onClick={() => setDetailTab("versions")} type="button">Версии <span>{projectVersions.length}</span></button></nav>
      {error && <div className="catalog-error" role="alert">{error}</div>}
      {detailTab === "description" ? <article className="project-description"><MarkdownBody source={projectDetails?.body || selectedProject.description} /></article> : <><div className="version-filters"><label>Версия Minecraft<select value={versionGameFilter} onChange={(event) => setVersionGameFilter(event.target.value)}><option value="">Все версии</option>{versionGameOptions.map((version) => <option key={version} value={version}>{version}</option>)}</select></label><label>Загрузчик<select value={versionLoaderFilter} onChange={(event) => setVersionLoaderFilter(event.target.value)}><option value="">Все загрузчики</option>{versionLoaderOptions.map((value) => <option key={value} value={value}>{loaderLabel(value)}</option>)}</select></label><button disabled={!versionGameFilter && !versionLoaderFilter} onClick={() => { setVersionGameFilter(""); setVersionLoaderFilter(""); }} type="button">Сбросить</button><span>Найдено: {filteredProjectVersions.length}</span></div><div className="project-version-list">{filteredProjectVersions.map((version) => <article key={version.id}><span className={`release-kind ${version.version_type}`}>{version.version_type === "release" ? "R" : version.version_type === "beta" ? "B" : "A"}</span><div><h2>{version.name || version.version_number}</h2><p>{version.game_versions.join(", ")} · {version.loaders.map(loaderLabel).join(", ") || "Minecraft"}</p></div><time>{formatProjectDate(version.date_published)}</time><span>↓ {compact(version.downloads)}</span><button disabled={already || Boolean(installTask)} onClick={() => void install(selectedProject, version.id)} type="button">Установить</button></article>)}{filteredProjectVersions.length === 0 && <div className="version-filter-empty">Для выбранной версии и загрузчика релизов нет.</div>}</div></>}
    </section>;
  }

  const visibleTabs = forBuild ? tabs.filter((tab) => tab.id !== "modpack") : tabs;
  return (
    <section className="content-page">
      <div className="content-heading">
        <div><span className="eyebrow">MODRINTH · ЕДИНЫЙ КАТАЛОГ</span><h1>Контент и сборки</h1><p>Ищите, устанавливайте и запускайте контент из одного места.</p></div>
        <div className="content-heading-actions"><button className="secondary-small" disabled={Boolean(installTask) || busy === "mrpack"} onClick={() => void importMrpack()} type="button">Установить .mrpack</button><button className="primary-small" onClick={() => setCreating((value) => !value)} type="button">+ Создать сборку</button></div>
      </div>

      {builds.length > 0 && <div className="build-strip">
        {builds.map((build) => (
          <button className={build.isActive ? "build-chip active" : "build-chip"} data-sound="none" key={build.id} onClick={() => void select(build)} type="button">
            {build.iconUrl ? <img alt="" src={build.iconUrl} /> : <span>{build.name.slice(0, 1).toUpperCase()}</span>}
            <b>{build.name}</b><small>{build.loader} · {baseVersion(build)}</small>
          </button>
        ))}
      </div>}

      {creating && <div className="create-build-card">
        <label>Название<input value={newName} onChange={(event) => setNewName(event.target.value)} /></label>
        <label>Версия<select value={gameVersion || defaultGameVersion} onChange={(event) => setGameVersion(event.target.value)}>{versions.map((version) => <option key={version.id}>{version.id}</option>)}</select></label>
        <label>Загрузчик<select value={loader || "fabric"} onChange={(event) => setLoader(event.target.value)}><option value="fabric">Fabric</option><option value="quilt">Quilt</option><option value="vanilla">Vanilla</option></select></label>
        <button disabled={busy === "create"} onClick={() => void create()} type="button">{busy === "create" ? "Создаём…" : "Создать"}</button>
      </div>}

      <div className="catalog-tabs">{visibleTabs.map((tab) => <button className={type === tab.id ? "active" : ""} key={tab.id} onClick={() => setType(tab.id)} type="button">{tab.label}</button>)}</div>
      <form className="catalog-search is-simple" onSubmit={(event) => { event.preventDefault(); void search(0); }}>
        <input aria-label="Поиск Modrinth" placeholder={`Поиск: ${tabs.find((tab) => tab.id === type)?.label.toLowerCase()}…`} value={query} onChange={(event) => setQuery(event.target.value)} />
        <button type="submit">Найти</button>
      </form>
      {error && <div className="catalog-error" role="alert">{error}</div>}

      <div className="content-layout">
        <div className="project-list" aria-busy={busy === "search"}>
          {busy === "search" && projects.length === 0 ? <div className="catalog-empty">Загружаем Modrinth…</div> : projects.filter((project) => !hideInstalled || !installed.some((item) => item.projectId === project.project_id)).map((project) => {
            const already = installed.some((item) => item.projectId === project.project_id);
            return <article className="project-card" key={project.project_id} onClick={() => void openProject(project)} tabIndex={0} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") void openProject(project); }}>
              {project.icon_url ? <img alt="" src={project.icon_url} /> : <span className="project-icon">{project.title.slice(0, 1)}</span>}
              <div><h2>{project.title} <small>от {project.author}</small></h2><p>{project.description}</p><div className="project-tags">{project.categories.slice(0, 4).map((category) => <span key={category}>{category}</span>)}<span>↓ {compact(project.downloads)}</span></div></div>
              <button disabled={already || Boolean(installTask) || busy === project.project_id} onClick={(event) => { event.stopPropagation(); void install(project); }} type="button">{already ? "Установлено" : installTask?.project.project_id === project.project_id ? "Установка…" : "+ Установить"}</button>
            </article>;
          })}
          {!busy && projects.length === 0 && <div className="catalog-empty">По вашему запросу ничего не найдено.</div>}
        </div>
        <aside className="catalog-filter-panel"><div className="filter-panel-heading"><strong>Фильтры</strong><button onClick={resetFilters} type="button">Сбросить</button></div><label className="filter-switch"><input checked={hideInstalled} onChange={(event) => setHideInstalled(event.target.checked)} type="checkbox" /><span>Скрыть установленное</span></label><hr /><label>Сортировка<select value={sort} onChange={(event) => setSort(event.target.value)}><option value="relevance">По релевантности</option><option value="downloads">По загрузкам</option><option value="follows">По подписчикам</option><option value="newest">Сначала новые</option><option value="updated">Недавно обновлённые</option></select></label><label>Версия Minecraft<select value={gameVersion} onChange={(event) => setGameVersion(event.target.value)}><option value="">Все версии</option>{versions.slice(0, 60).map((version) => <option key={version.id} value={version.id}>{version.id}</option>)}</select></label><label>Категория<select value={category} onChange={(event) => setCategory(event.target.value)}><option value="">Все категории</option>{categoryOptions(type).map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select></label><label>Среда<select value={environment} onChange={(event) => setEnvironment(event.target.value)}><option value="">Любая</option><option value="client">Клиент</option><option value="server">Сервер</option></select></label><hr /><div className="filter-loader-list"><strong>Загрузчик</strong>{[["", "Все"], ["fabric", "Fabric"], ["quilt", "Quilt"], ["forge", "Forge"], ["neoforge", "NeoForge"], ["vanilla", "Vanilla"]].map(([value, label]) => <button className={loader === value ? "active" : ""} disabled={type === "resourcepack" || type === "shader"} key={value || "all"} onClick={() => setLoader(value)} type="button">{label}</button>)}</div></aside>
      </div>
      {totalHits > 20 && <nav aria-label="Страницы каталога" className="catalog-pagination"><button aria-label="Предыдущая страница" disabled={page === 0} onClick={() => void search(page - 1)} type="button">‹</button>{paginationItems(page, pageCount).map((item, index) => item === "…" ? <span key={`gap-${index}`}>…</span> : <button aria-current={item === page ? "page" : undefined} className={item === page ? "active" : ""} key={item} onClick={() => void search(item)} type="button">{item + 1}</button>)}<button aria-label="Следующая страница" disabled={page + 1 >= pageCount} onClick={() => void search(page + 1)} type="button">›</button></nav>}
    </section>
  );
}

function baseVersion(build: BuildSummary) { const prefix = build.loaderVersion ? `${build.loader}-loader-${build.loaderVersion}-` : ""; return prefix && build.gameVersion.startsWith(prefix) ? build.gameVersion.slice(prefix.length) : build.gameVersion; }
function compact(value: number) { return new Intl.NumberFormat("ru", { notation: "compact", maximumFractionDigits: 1 }).format(value); }
function typeLabel(type: ModrinthProjectType) { return ({ modpack: "СБОРКА", mod: "МОД", resourcepack: "РЕСУРСПАК", shader: "ШЕЙДЕР" } as const)[type]; }
function formatProjectDate(value: string) { const date = new Date(value); return Number.isNaN(date.valueOf()) ? "—" : new Intl.DateTimeFormat("ru", { day: "2-digit", month: "short", year: "numeric" }).format(date); }
function loaderLabel(value: string) { return value === "neoforge" ? "NeoForge" : value === "minecraft" ? "Vanilla" : value ? value[0].toUpperCase() + value.slice(1) : "Minecraft"; }
function compareMinecraftVersions(a: string, b: string) { return b.localeCompare(a, undefined, { numeric: true, sensitivity: "base" }); }
function paginationItems(page: number, count: number): Array<number | "…"> { const values = new Set([0, Math.max(0, page - 1), page, Math.min(count - 1, page + 1), count - 1]); const sorted = [...values].sort((a, b) => a - b); const result: Array<number | "…"> = []; sorted.forEach((value, index) => { if (index && value - sorted[index - 1] > 1) result.push("…"); result.push(value); }); return result; }
function categoryOptions(type: ModrinthProjectType): Array<[string, string]> { const common: Array<[string, string]> = [["adventure", "Приключения"], ["challenging", "Испытания"], ["combat", "Сражения"], ["lightweight", "Лёгкие"], ["magic", "Магия"], ["multiplayer", "Мультиплеер"], ["optimization", "Оптимизация"], ["technology", "Технологии"]]; if (type === "shader") return [["cartoon", "Мультяшные"], ["fantasy", "Фэнтези"], ["realistic", "Реалистичные"], ["vanilla-like", "В стиле Vanilla"]]; if (type === "resourcepack") return [["audio", "Звуки"], ["models", "Модели"], ["gui", "Интерфейс"], ["realistic", "Реалистичные"], ["vanilla-like", "В стиле Vanilla"]]; return common; }
function MarkdownBody({ source }: { source: string }) { return <div className="markdown-body">{source.split(/\r?\n/).map((line, index) => markdownLine(line, index))}</div>; }
function markdownLine(line: string, key: number): ReactNode {
  const linkedImage = line.match(/^\[!\[([^\]]*)\]\((https?:\/\/[^)]+)\)\]\((https?:\/\/[^)]+)\)$/);
  if (linkedImage) return <a className="markdown-image-link" href={linkedImage[3]} key={key} rel="noreferrer" target="_blank"><img alt={linkedImage[1]} loading="lazy" src={linkedImage[2]} /></a>;
  const image = line.match(/^!\[([^\]]*)\]\((https?:\/\/[^)]+)\)$/);
  if (image) return <img alt={image[1]} className="markdown-image" key={key} loading="lazy" src={image[2]} />;
  const heading = line.match(/^(#{1,4})\s+(.+)$/);
  if (heading) { const body = renderInline(heading[2], key); return heading[1].length <= 2 ? <h2 key={key}>{body}</h2> : <h3 key={key}>{body}</h3>; }
  if (/^[-*]\s+/.test(line)) return <div className="markdown-list-item" key={key}>• <span>{renderInline(line.replace(/^[-*]\s+/, ""), key)}</span></div>;
  if (!line.trim()) return <div className="markdown-gap" key={key} />;
  return <p key={key}>{renderInline(line, key)}</p>;
}
function renderInline(value: string, key: number): ReactNode[] {
  const result: ReactNode[] = []; const pattern = /(\[([^\]]+)\]\((https?:\/\/[^)]+)\)|\*\*([^*]+)\*\*|\*([^*]+)\*)/g; let cursor = 0; let match: RegExpExecArray | null;
  while ((match = pattern.exec(value))) { if (match.index > cursor) result.push(value.slice(cursor, match.index)); if (match[2] && match[3]) result.push(<a href={match[3]} key={`${key}-${match.index}`} rel="noreferrer" target="_blank">{match[2]}</a>); else if (match[4]) result.push(<strong key={`${key}-${match.index}`}>{match[4]}</strong>); else result.push(<em key={`${key}-${match.index}`}>{match[5]}</em>); cursor = match.index + match[0].length; }
  if (cursor < value.length) result.push(value.slice(cursor)); return result;
}
function errorMessage(reason: unknown) { if (reason && typeof reason === "object" && "message" in reason && typeof reason.message === "string") return reason.message; return "Операция не выполнена."; }
