import { KvanthIcon } from "../../components/KvanthIcon";
import { AppSelect } from "../../components/AppSelect";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { AppApi } from "../../app/tauri";
import type { BuildSummary, GameVersionSummary, InstalledContent, LoaderVersionSummary, ModrinthProject, ModrinthProjectDetails, ModrinthProjectType, ModrinthVersion } from "../../app/types";
import { playSound } from "../../components/SoundEffects";
import { createPortal } from "react-dom";

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
  targetBuildId?: string;
  onBackToBuild?(): void;
  startCreating?: boolean;
}

export interface ContentInstallTask { project: ModrinthProject; stage: string; step: number; error?: string; }
type CatalogSource = "modrinth" | "curseforge";
interface InstallDestination { build: BuildSummary; versionId: string; installed: boolean; }
interface PendingInstall { project: ModrinthProject; source: CatalogSource; rows: InstallDestination[]; loading: boolean; error?: string; }

export function ContentPage({ api, versions, onBuildSelected, installTask, onInstallTaskChange, forBuild = false, targetBuildId, onBackToBuild, startCreating = false }: Props) {
  const [type, setType] = useState<ModrinthProjectType>(forBuild ? "mod" : "modpack");
  const [source, setSource] = useState<CatalogSource>("modrinth");
  const [query, setQuery] = useState("");
  const [gameVersion, setGameVersion] = useState("");
  const [createGameVersion, setCreateGameVersion] = useState(versions.find((version) => version.type === "release")?.id ?? "1.21.1");
  const [showExperimentalVersions, setShowExperimentalVersions] = useState(false);
  const [loaderVersions, setLoaderVersions] = useState<LoaderVersionSummary[]>([]);
  const [loaderVersionMode, setLoaderVersionMode] = useState<"stable" | "latest" | "other">("stable");
  const [chosenLoaderVersion, setChosenLoaderVersion] = useState("");
  const [loaderVersionStatus, setLoaderVersionStatus] = useState<"idle" | "loading" | "ready" | "error">("idle");
  const [loader, setLoader] = useState("");
  const [createLoader, setCreateLoader] = useState<"vanilla" | "fabric" | "quilt" | "forge">("fabric");
  const [category, setCategory] = useState("");
  const [environment, setEnvironment] = useState("");
  const [sort, setSort] = useState("relevance");
  const [hideInstalled, setHideInstalled] = useState(false);
  const [projects, setProjects] = useState<ModrinthProject[]>([]);
  const [page, setPage] = useState(0);
  const [totalHits, setTotalHits] = useState(0);
  const [builds, setBuilds] = useState<BuildSummary[]>([]);
  const [installed, setInstalled] = useState<InstalledContent[]>([]);
  const [catalogReady, setCatalogReady] = useState(!targetBuildId);
  const [busy, setBusy] = useState<string>();
  const [error, setError] = useState<string>();
  const [creating, setCreating] = useState(startCreating);
  const [newName, setNewName] = useState("Моя сборка");
  const [iconDataUrl, setIconDataUrl] = useState<string>();
  const iconInput = useRef<HTMLInputElement>(null);
  const searchRequest = useRef(0);
  const [selectedProject, setSelectedProject] = useState<ModrinthProject>();
  const [selectedCurseForge, setSelectedCurseForge] = useState<ModrinthProject>();
  const [pendingInstall, setPendingInstall] = useState<PendingInstall>();
  const [installSearch, setInstallSearch] = useState("");
  const installRequest = useRef(0);
  const [curseForgeVersions, setCurseForgeVersions] = useState<ModrinthVersion[]>([]);
  const [projectDetails, setProjectDetails] = useState<ModrinthProjectDetails>();
  const [projectVersions, setProjectVersions] = useState<ModrinthVersion[]>([]);
  const [detailTab, setDetailTab] = useState<"description" | "versions">("description");
  const [versionGameFilter, setVersionGameFilter] = useState("");
  const [versionLoaderFilter, setVersionLoaderFilter] = useState("");

  const activeBuild = useMemo(() => targetBuildId ? builds.find((item) => item.id === targetBuildId) : builds.find((item) => item.isActive) ?? builds[0], [builds, targetBuildId]);
  const versionGameOptions = useMemo(() => [...new Set(projectVersions.flatMap((version) => version.game_versions))].sort(compareMinecraftVersions), [projectVersions]);
  const versionLoaderOptions = useMemo(() => [...new Set(projectVersions.flatMap((version) => version.loaders))].sort(), [projectVersions]);
  const filteredProjectVersions = useMemo(() => projectVersions.filter((version) => (!targetBuildId || (activeBuild && supportsBuild(version, activeBuild, type))) && (!versionGameFilter || version.game_versions.includes(versionGameFilter)) && (!versionLoaderFilter || version.loaders.includes(versionLoaderFilter))), [projectVersions, versionGameFilter, versionLoaderFilter, targetBuildId, activeBuild, type]);
  const defaultGameVersion = versions.find((version) => version.type === "release")?.id ?? "1.21.1";
  const createGameVersions = versions.filter((version) => showExperimentalVersions || version.type === "release" || version.id === createGameVersion);
  const selectedLoaderVersion = createLoader === "vanilla" ? undefined : loaderVersionMode === "other" ? chosenLoaderVersion : loaderVersionMode === "latest" ? loaderVersions[0]?.id : (loaderVersions.find((version) => version.stable) ?? loaderVersions[0])?.id;
  const pageCount = Math.max(1, Math.ceil(totalHits / 20));

  function resetFilters() {
    setHideInstalled(false); setSort("relevance"); setCategory(""); setEnvironment("");
    if (!targetBuildId) { setGameVersion(""); setLoader(""); }
  }

  async function reloadBuilds() {
    const next = await api.listBuilds();
    setBuilds(next);
    const active = targetBuildId ? next.find((item) => item.id === targetBuildId) : next.find((item) => item.isActive) ?? next[0];
    if (active && targetBuildId) {
      setLoader(active.loader);
      setGameVersion(baseVersion(active));
    } else if (targetBuildId) {
      setError("Сборка больше не найдена.");
    }
    setInstalled(active ? await api.listInstalledContent(active.id) : []);
    setCatalogReady(true);
  }

  async function search(targetPage = page) {
    const request = ++searchRequest.current;
    if (targetPage === 0) { setProjects([]); setTotalHits(0); }
    setBusy("search"); setError(undefined);
    try {
      const result = source === "curseforge"
        ? await api.searchCurseForge(query, type, gameVersion || undefined, type === "mod" ? loader || undefined : undefined, targetPage * 20)
        : await api.searchModrinth(query, type, gameVersion || undefined, type === "resourcepack" || type === "shader" ? undefined : loader || undefined, targetPage * 20, category, environment, sort);
      if (request === searchRequest.current) { setProjects(result.hits); setTotalHits(result.total_hits); setPage(targetPage); }
    } catch (reason) { if (request === searchRequest.current) setError(errorMessage(reason)); }
    finally { if (request === searchRequest.current) setBusy(undefined); }
  }

  useEffect(() => { void reloadBuilds().catch(() => setError("Не удалось загрузить список сборок.")); }, []);
  useEffect(() => { if (catalogReady) void search(0); }, [catalogReady, source, type, gameVersion, loader, category, environment, sort]);
  useEffect(() => {
    if (!creating || createLoader === "vanilla") { setLoaderVersions([]); setLoaderVersionStatus("idle"); return; }
    let current = true;
    setLoaderVersions([]); setChosenLoaderVersion(""); setLoaderVersionStatus("loading");
    void api.listLoaderVersions(createGameVersion, createLoader).then((items) => {
      if (!current) return;
      setLoaderVersions(items); setChosenLoaderVersion(items[0]?.id ?? ""); setLoaderVersionStatus("ready");
    }).catch(() => { if (current) setLoaderVersionStatus("error"); });
    return () => { current = false; };
  }, [api, creating, createGameVersion, createLoader]);
  useEffect(() => {
    if (!creating) return;
    const onEscape = (event: KeyboardEvent) => { if (event.key === "Escape" && busy !== "create") setCreating(false); };
    window.addEventListener("keydown", onEscape);
    return () => window.removeEventListener("keydown", onEscape);
  }, [creating, busy]);
  useEffect(() => {
    if (!pendingInstall) return;
    const onEscape = (event: KeyboardEvent) => { if (event.key === "Escape" && !installTask) { installRequest.current++; setPendingInstall(undefined); } };
    window.addEventListener("keydown", onEscape);
    return () => window.removeEventListener("keydown", onEscape);
  }, [pendingInstall, installTask]);

  function loadIcon(file?: File) {
    if (!file) return;
    if (!["image/png", "image/jpeg", "image/webp"].includes(file.type) || file.size > 2_000_000) {
      setError("Выберите PNG, JPG или WebP размером до 2 МБ.");
      return;
    }
    const reader = new FileReader();
    reader.onload = () => { if (typeof reader.result === "string") { setIconDataUrl(reader.result); setError(undefined); } };
    reader.onerror = () => setError("Не удалось прочитать изображение.");
    reader.readAsDataURL(file);
  }

  async function showCurseForge(project: ModrinthProject) {
    setSelectedCurseForge(project); setCurseForgeVersions([]); setError(undefined);
    try { setCurseForgeVersions(await api.curseForgeVersions(Number(project.project_id.split(":")[1]), gameVersion || undefined, type === "mod" ? loader || undefined : undefined)); }
    catch (reason) { setError(errorMessage(reason)); }
  }

  async function requestInstall(project: ModrinthProject, installSource: CatalogSource, versionId?: string) {
    if (installTask) return;
    if (project.project_type === "modpack") {
      if (installSource === "curseforge") await installCurseForge(project, versionId);
      else await install(project, versionId);
      return;
    }
    if (targetBuildId) {
      const target = builds.find((build) => build.id === targetBuildId);
      if (!target) { setError("Сборка больше не найдена."); return; }
      setBusy(project.project_id); setError(undefined);
      try {
        const releases = installSource === "curseforge"
          ? await api.curseForgeVersions(Number(project.project_id.split(":")[1]), baseVersion(target), project.project_type === "mod" ? target.loader : undefined)
          : await api.modrinthProjectVersions(project.project_id);
        const compatible = releases
          .filter((version) => (!versionId || version.id === versionId) && supportsBuild(version, target, project.project_type))
          .sort((a, b) => b.date_published.localeCompare(a.date_published));
        if (!compatible.length) { setError("Для этой сборки нет совместимой версии проекта."); return; }
        if (installed.some((item) => item.projectId === project.project_id)) { setError("Проект уже установлен в этой сборке."); return; }
        if (installSource === "curseforge") await installCurseForge(project, compatible[0].id, target);
        else await install(project, compatible[0].id, target);
      } catch (reason) { setError(errorMessage(reason)); }
      finally { setBusy(undefined); }
      return;
    }
    const request = ++installRequest.current;
    setInstallSearch("");
    setPendingInstall({ project, source: installSource, rows: [], loading: true });
    try {
      const modrinthVersions = installSource === "modrinth" ? await api.modrinthProjectVersions(project.project_id) : [];
      const rows = (await Promise.all(builds.map(async (build) => {
        const [versionsForBuild, installedForBuild] = await Promise.all([
          installSource === "curseforge"
            ? api.curseForgeVersions(Number(project.project_id.split(":")[1]), baseVersion(build), project.project_type === "mod" ? build.loader : undefined)
            : Promise.resolve(modrinthVersions),
          api.listInstalledContent(build.id),
        ]);
        const compatible = versionsForBuild
          .filter((version) => (!versionId || version.id === versionId) && supportsBuild(version, build, project.project_type))
          .sort((a, b) => b.date_published.localeCompare(a.date_published));
        const best = compatible[0];
        return best ? { build, versionId: best.id, installed: installedForBuild.some((item) => item.projectId === project.project_id) } : undefined;
      }))).filter((row): row is InstallDestination => Boolean(row));
      if (request === installRequest.current) setPendingInstall({ project, source: installSource, rows, loading: false });
    } catch (reason) {
      if (request === installRequest.current) setPendingInstall({ project, source: installSource, rows: [], loading: false, error: errorMessage(reason) });
    }
  }

  async function installCurseForge(project: ModrinthProject, versionId?: string, targetBuild?: BuildSummary) {
    if (installTask || (project.project_type !== "modpack" && !targetBuild)) return;
    setBusy(project.project_id); setError(undefined);
    onInstallTaskChange({ project, stage: "Загрузка с CurseForge", step: 1 });
    try {
      const projectId = Number(project.project_id.split(":")[1]);
      if (project.project_type === "modpack") await api.installCurseForgeModpack(projectId, versionId ? Number(versionId) : undefined);
      else await api.installCurseForgeProject(projectId, targetBuild!.id, project.project_type, versionId ? Number(versionId) : undefined);
      await reloadBuilds(); await onBuildSelected(); onInstallTaskChange(undefined); setPendingInstall(undefined);
    } catch (reason) { const message = errorMessage(reason); setError(message); onInstallTaskChange({ project, stage: "Установка не завершена", step: 0, error: message }); }
    finally { setBusy(undefined); }
  }

  async function importCurseForgeFile() {
    if (!activeBuild || type === "modpack") return;
    setBusy("local-import"); setError(undefined);
    try { await api.importLocalContent(activeBuild.id, type); await reloadBuilds(); await onBuildSelected(); }
    catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  }

  async function create() {
    setBusy("create"); setError(undefined);
    try {
      await api.createBuild(newName.trim(), createGameVersion || defaultGameVersion, createLoader, iconDataUrl, selectedLoaderVersion);
      await reloadBuilds(); await onBuildSelected(); setCreating(false);
    } catch (reason) { setError(errorMessage(reason)); }
    finally { setBusy(undefined); }
  }

  async function openProject(project: ModrinthProject) {
    setSelectedProject(project); setProjectDetails(undefined); setProjectVersions([]); setDetailTab("description"); setVersionGameFilter(""); setVersionLoaderFilter(""); setError(undefined);
    try {
      const [details, releases] = await Promise.all([api.modrinthProject(project.project_id), api.modrinthProjectVersions(project.project_id)]);
      setProjectDetails(details); setProjectVersions(releases);
    } catch (reason) { setError(errorMessage(reason)); }
  }

  async function install(project: ModrinthProject, versionId?: string, targetBuild?: BuildSummary) {
    if (installTask) return;
    setBusy(project.project_id); setError(undefined);
    onInstallTaskChange({ project, stage: "Загрузка контента", step: 1 });
    let succeeded = false;
    try {
      if (project.project_type === "modpack") {
        await api.installModrinthModpack(project.project_id, versionId);
      } else {
        if (!targetBuild) throw new Error("Выберите совместимую сборку.");
        await api.installModrinthProject(project.project_id, targetBuild.id, versionId);
      }
      await reloadBuilds(); await onBuildSelected();
      if (project.project_type === "modpack") playSound("install-complete");
      succeeded = true; setPendingInstall(undefined);
    }
    catch (reason) { const message = errorMessage(reason); setError(message); onInstallTaskChange({ project, stage: "Установка не завершена", step: 0, error: message }); }
    finally { setBusy(undefined); if (succeeded) onInstallTaskChange(undefined); }
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

  function closeInstallDialog() {
    if (installTask) return;
    installRequest.current++;
    setPendingInstall(undefined);
  }

  function renderInstallDialog() {
    if (!pendingInstall) return null;
    const { project, source: installSource, rows, loading, error: dialogError } = pendingInstall;
    const visible = rows.filter(({ build }) => build.name.toLocaleLowerCase().includes(installSearch.toLocaleLowerCase()));
    return createPortal(<div className="install-target-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) closeInstallDialog(); }}>
      <section aria-labelledby="install-target-title" aria-modal="true" className="install-target-dialog" role="dialog">
        <header><div><span className="eyebrow">{installSource === "modrinth" ? "MODRINTH" : "CURSEFORGE"} · {typeLabel(project.project_type)}</span><h2 id="install-target-title">Установить «{project.title}»</h2></div><button aria-label="Закрыть окно выбора сборки" disabled={Boolean(installTask)} onClick={closeInstallDialog} type="button"><KvanthIcon name="close" size={19} /></button></header>
        <div className="install-target-body">
          <input aria-label="Поиск сборки для установки" onChange={(event) => setInstallSearch(event.target.value)} placeholder="Поиск сборки…" value={installSearch} />
          {loading ? <p className="install-target-message">Ищем совместимые сборки…</p> : dialogError ? <p className="install-target-message" role="alert">{dialogError}</p> : rows.length === 0 ? <div className="install-target-empty"><p>Совместимых сборок пока нет.</p><button onClick={() => { closeInstallDialog(); setCreating(true); }} type="button">Создать сборку</button></div> : visible.length === 0 ? <p className="install-target-message">По вашему запросу сборки не найдены.</p> : <div className="install-target-list">{visible.map(({ build, versionId, installed: isInstalled }) => <article key={build.id}>{build.iconUrl ? <img alt="" src={build.iconUrl} /> : <span className="install-target-icon"><KvanthIcon name="custom-pack" size={22} /></span>}<div><strong>{build.name}</strong><small>{loaderLabel(build.loader)} · {baseVersion(build)}</small></div><button disabled={isInstalled || Boolean(installTask)} onClick={() => void (installSource === "modrinth" ? install(project, versionId, build) : installCurseForge(project, versionId, build))} type="button">{isInstalled ? "Установлено" : "Установить"}</button></article>)}</div>}
          {installTask?.error && <p className="install-target-message" role="alert">{installTask.error}</p>}
        </div>
        <footer><span>{rows.length} совместимых сборок</span><button disabled={Boolean(installTask)} onClick={closeInstallDialog} type="button">Отмена</button></footer>
      </section>
    </div>, document.body);
  }

  if (selectedCurseForge) {
    const already = installed.some((item) => item.projectId === selectedCurseForge.project_id);
    return <section className="content-page project-detail-page">
      <button className="project-back" onClick={() => setSelectedCurseForge(undefined)} type="button"><KvanthIcon name="back" size={20} /> Назад в каталог</button>
      <header className="project-detail-hero">
        {selectedCurseForge.icon_url ? <img alt="" src={selectedCurseForge.icon_url} /> : <span>{selectedCurseForge.title[0]}</span>}
        <div><span className="eyebrow">CURSEFORGE · {typeLabel(selectedCurseForge.project_type)}</span><h1>{selectedCurseForge.title}</h1><p>{selectedCurseForge.description}</p></div>
        <button disabled={((type === "modpack" || targetBuildId) && already) || Boolean(installTask)} onClick={() => void requestInstall(selectedCurseForge, "curseforge")} type="button"><KvanthIcon name={targetBuildId && already ? "confirm" : "download"} size={18} /> {(type === "modpack" || targetBuildId) && already ? "Установлено" : "Установить"}</button>
      </header>
      {type === "modpack" && <p className="curseforge-note">Сборка будет создана отдельно. Моды запускают код на вашем компьютере — устанавливайте только сборки доверенных авторов. Файлы с запретом сторонней загрузки установить не получится.</p>}
      {error && <div className="catalog-error" role="alert">{error}</div>}
      <div className="project-version-list">{curseForgeVersions.map((version) => <article key={version.id}><span className="release-kind release">R</span><div><h2>{version.name}</h2><p>{version.game_versions.join(", ")}</p></div><time>{formatProjectDate(version.date_published)}</time><button disabled={((type === "modpack" || targetBuildId) && already) || Boolean(installTask)} onClick={() => void requestInstall(selectedCurseForge, "curseforge", version.id)} type="button">{targetBuildId && already ? "Установлено" : "Установить"}</button></article>)}</div>
      {renderInstallDialog()}
    </section>;
  }
  if (selectedProject) {
    const already = installed.some((item) => item.projectId === selectedProject.project_id);
    return <section className="content-page project-detail-page">
      <button className="project-back" onClick={() => setSelectedProject(undefined)} type="button"><KvanthIcon name="back" size={20} /> Назад в каталог</button>
      <header className="project-detail-hero">
        {selectedProject.icon_url ? <img alt="" src={selectedProject.icon_url} /> : <span>{selectedProject.title[0]}</span>}
        <div><span className="eyebrow">MODRINTH · {typeLabel(selectedProject.project_type)}</span><h1>{selectedProject.title}</h1><p>{projectDetails?.description ?? selectedProject.description}</p><div className="project-tags">{selectedProject.categories.slice(0, 5).map((category) => <span key={category}>{category}</span>)}<span><KvanthIcon name="download" size={14} /> {compact(projectDetails?.downloads ?? selectedProject.downloads)}</span></div></div>
        <button disabled={((type === "modpack" || targetBuildId) && already) || Boolean(installTask)} onClick={() => void requestInstall(selectedProject, "modrinth")} type="button"><KvanthIcon name={(type === "modpack" || targetBuildId) && already ? "confirm" : "download"} size={18} /> {(type === "modpack" || targetBuildId) && already ? "Установлено" : "Установить"}</button>
      </header>
      <nav className="project-detail-tabs"><button className={detailTab === "description" ? "active" : ""} onClick={() => setDetailTab("description")} type="button"><KvanthIcon name="file" size={16} /> Описание</button><button className={detailTab === "versions" ? "active" : ""} onClick={() => setDetailTab("versions")} type="button"><KvanthIcon name="library" size={16} /> Версии <span>{projectVersions.length}</span></button></nav>
      {error && <div className="catalog-error" role="alert">{error}</div>}
      {detailTab === "description" ? <article className="project-description"><MarkdownBody source={projectDetails?.body || selectedProject.description} /></article> : <>
        {!targetBuildId && <div className="version-filters">
          <div className="form-field"><span>Версия Minecraft</span><AppSelect ariaLabel="Фильтр версии Minecraft" onChange={setVersionGameFilter} options={[{ value: "", label: "Все версии" }, ...versionGameOptions.map((version) => ({ value: version, label: version }))]} value={versionGameFilter} /></div>
          <div className="form-field"><span>Загрузчик</span><AppSelect ariaLabel="Фильтр загрузчика" onChange={setVersionLoaderFilter} options={[{ value: "", label: "Все загрузчики" }, ...versionLoaderOptions.map((value) => ({ value, label: loaderLabel(value) }))]} value={versionLoaderFilter} /></div>
          <button disabled={!versionGameFilter && !versionLoaderFilter} onClick={() => { setVersionGameFilter(""); setVersionLoaderFilter(""); }} type="button"><KvanthIcon name="refresh" size={18} /> Сбросить</button><span>Найдено: {filteredProjectVersions.length}</span>
        </div>}
        <div className="project-version-list">{filteredProjectVersions.map((version) => <article key={version.id}><span className={`release-kind ${version.version_type}`}>{version.version_type === "release" ? "R" : version.version_type === "beta" ? "B" : "A"}</span><div><h2>{version.name || version.version_number}</h2><p>{version.game_versions.join(", ")} · {version.loaders.map(loaderLabel).join(", ") || "Minecraft"}</p></div><time>{formatProjectDate(version.date_published)}</time><span><KvanthIcon name="download" size={14} /> {compact(version.downloads)}</span><button disabled={((type === "modpack" || targetBuildId) && already) || Boolean(installTask)} onClick={() => void requestInstall(selectedProject, "modrinth", version.id)} type="button"><KvanthIcon name="download" size={18} /> {targetBuildId && already ? "Установлено" : "Установить"}</button></article>)}{filteredProjectVersions.length === 0 && <div className="version-filter-empty">Для выбранной версии и загрузчика релизов нет.</div>}</div>
      </>}
      {renderInstallDialog()}
    </section>;
  }

  const visibleTabs = forBuild ? tabs.filter((tab) => tab.id !== "modpack") : tabs;
  return (
    <section className="content-page">
      {targetBuildId ? <div className="scoped-catalog-header"><button className="project-back" onClick={onBackToBuild} type="button"><KvanthIcon name="back" size={20} /> Библиотека</button><div className="scoped-catalog-build">{activeBuild?.iconUrl ? <img alt="" src={activeBuild.iconUrl} /> : <span><KvanthIcon name="custom-pack" size={24} /></span>}<div><h1>{activeBuild?.name ?? "Сборка"}</h1><p>Minecraft {activeBuild ? baseVersion(activeBuild) : "—"} · {activeBuild ? loaderLabel(activeBuild.loader) : "—"}</p></div></div></div> : <div className="content-heading"><div><h1>Контент и сборки</h1></div></div>}

      {creating && <div className="create-build-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget && busy !== "create") setCreating(false); }}>
        <form aria-labelledby="create-build-title" aria-modal="true" className="create-build-dialog" onSubmit={(event) => { event.preventDefault(); void create(); }} role="dialog">
          <header><h2 id="create-build-title">Создать сборку</h2><button aria-label="Закрыть" disabled={busy === "create"} onClick={() => setCreating(false)} type="button"><KvanthIcon name="close" size={19} /></button></header>
          <div className="create-build-body">
            <div className="create-build-icon-row"><div className="create-build-icon-preview">{iconDataUrl ? <img alt="Иконка новой сборки" src={iconDataUrl} /> : <KvanthIcon name="custom-pack" size={52} />}</div><div className="create-build-icon-actions"><input accept="image/png,image/jpeg,image/webp" aria-label="Файл иконки сборки" onChange={(event) => loadIcon(event.target.files?.[0])} ref={iconInput} type="file" /><button onClick={() => iconInput.current?.click()} type="button"><KvanthIcon name="import" size={18} /> Загрузить иконку</button>{iconDataUrl && <button onClick={() => { setIconDataUrl(undefined); if (iconInput.current) iconInput.current.value = ""; }} type="button">Убрать иконку</button>}</div></div>
            <label className="create-build-name">Название<input autoFocus maxLength={48} onChange={(event) => setNewName(event.target.value)} placeholder="Название сборки" value={newName} /></label>
            <div className="create-build-loader"><span>Загрузчик</span><div role="group" aria-label="Загрузчик новой сборки">{(["vanilla", "fabric", "quilt", "forge"] as const).map((value) => <button aria-pressed={createLoader === value} className={createLoader === value ? "active" : ""} key={value} onClick={() => setCreateLoader(value)} type="button">{loaderLabel(value)}</button>)}</div></div>
            <div className="form-field"><span>Версия Minecraft</span><AppSelect ariaLabel="Версия Minecraft для новой сборки" onChange={setCreateGameVersion} options={createGameVersions.map((version) => ({ value: version.id, label: version.type === "release" ? version.id : `${version.id} · ${version.type === "snapshot" ? "снапшот" : version.type === "old_beta" ? "бета" : version.type === "old_alpha" ? "альфа" : version.type}` }))} value={createGameVersion || defaultGameVersion} /><button className="create-version-toggle" onClick={() => setShowExperimentalVersions((current) => !current)} type="button">{showExperimentalVersions ? "Скрыть снапшоты и беты" : "Показать снапшоты и беты"}</button></div>
            {createLoader !== "vanilla" && <div className="create-build-loader"><span>Версия {loaderLabel(createLoader)}</span>{loaderVersionStatus === "loading" ? <small>Проверяем совместимые версии…</small> : loaderVersionStatus === "error" ? <small role="alert">Не удалось получить версии загрузчика.</small> : loaderVersions.length === 0 ? <small>Для этой версии Minecraft загрузчик недоступен.</small> : <><div role="group" aria-label="Выбор версии загрузчика">{(["stable", "latest", "other"] as const).map((mode) => <button aria-pressed={loaderVersionMode === mode} className={loaderVersionMode === mode ? "active" : ""} key={mode} onClick={() => setLoaderVersionMode(mode)} type="button">{mode === "stable" ? "Стабильная" : mode === "latest" ? "Последняя" : "Другая"}</button>)}</div>{loaderVersionMode === "other" && <AppSelect ariaLabel="Конкретная версия загрузчика" onChange={setChosenLoaderVersion} options={loaderVersions.map((version) => ({ value: version.id, label: `${version.id}${version.stable ? " · стабильная" : " · бета"}` }))} value={chosenLoaderVersion} />}</>}</div>}
            {error && <div className="catalog-error" role="alert">{error}</div>}
          </div>
          <footer><button disabled={busy === "create"} onClick={() => setCreating(false)} type="button">Отмена</button><button disabled={busy === "create" || !newName.trim() || (createLoader !== "vanilla" && (loaderVersionStatus !== "ready" || !selectedLoaderVersion))} type="submit"><KvanthIcon name="add" size={18} /> {busy === "create" ? "Создаём…" : "Создать"}</button></footer>
        </form>
      </div>}

      <div className="catalog-source-layout"><nav className="catalog-source-menu" aria-label="Источник каталога">
        {!targetBuildId && <button onClick={() => setCreating(true)} type="button"><span className="catalog-source-mark vanilla-mark"><KvanthIcon name="custom-pack" size={25} /></span><strong>Своя сборка</strong></button>}
        {!forBuild && <button disabled={Boolean(installTask) || busy === "mrpack"} onClick={() => void importMrpack()} type="button"><span className="catalog-source-mark import-mark"><KvanthIcon name="import" size={25} /></span><strong>Импорт .mrpack</strong></button>}
        <button aria-pressed={source === "modrinth"} className={source === "modrinth" ? "active" : ""} onClick={() => { setSource("modrinth"); setError(undefined); }} type="button"><span className="catalog-source-mark modrinth-mark"><KvanthIcon name="modrinth" size={25} /></span><strong>Modrinth</strong></button>
        <button aria-pressed={source === "curseforge"} className={source === "curseforge" ? "active" : ""} onClick={() => { setSource("curseforge"); setError(undefined); }} type="button"><span className="catalog-source-mark curseforge-mark"><KvanthIcon name="curseforge" size={25} /></span><strong>CurseForge</strong></button>
      </nav><div className="catalog-source-workspace">
      <div className="catalog-tabs">{visibleTabs.map((tab) => <button className={type === tab.id ? "active" : ""} key={tab.id} onClick={() => setType(tab.id)} type="button"><KvanthIcon name={tab.id === "mod" ? "mod" : tab.id === "resourcepack" ? "resources" : tab.id === "shader" ? "shaders" : "custom-pack"} size={16} /> {tab.label}</button>)}</div>
      {source === "curseforge" ? <div className="curseforge-panel">
        <div className="curseforge-panel-copy"><h2>{tabs.find((tab) => tab.id === type)?.label} на CurseForge</h2></div>
        <form className="catalog-search is-simple" onSubmit={(event) => { event.preventDefault(); void search(0); }}><input aria-label="Поиск CurseForge" placeholder="Название проекта…" value={query} onChange={(event) => setQuery(event.target.value)} /><button type="submit"><KvanthIcon name="search" size={16} /> Найти</button></form>
        <div className="curseforge-actions">
          {targetBuildId ? <div className="scoped-catalog-locks"><span>Minecraft {gameVersion}</span>{type === "mod" && <span>{loaderLabel(loader)}</span>}</div> : <><div className="form-field"><span>Версия Minecraft</span><AppSelect ariaLabel="Версия Minecraft CurseForge" onChange={setGameVersion} options={[{ value: "", label: "Все версии" }, ...versions.slice(0, 60).map((version) => ({ value: version.id, label: version.id }))]} value={gameVersion} /></div>{type === "mod" && <div className="form-field"><span>Загрузчик</span><AppSelect ariaLabel="Загрузчик CurseForge" onChange={setLoader} options={[{ value: "", label: "Все" }, { value: "fabric", label: "Fabric" }, { value: "quilt", label: "Quilt" }, { value: "forge", label: "Forge" }, { value: "neoforge", label: "NeoForge" }]} value={loader} /></div>}</>}
          {type !== "modpack" && <button className="secondary-small" disabled={!activeBuild || busy === "local-import"} onClick={() => void importCurseForgeFile()} type="button"><KvanthIcon name="import" size={18} /> Добавить скачанный файл</button>}
        </div>
        {error && <div className="catalog-error" role="alert">{error}</div>}
        <div className="project-list" aria-busy={busy === "search"}>{projects.map((project) => <article className="project-card" key={project.project_id} onClick={() => void showCurseForge(project)} tabIndex={0} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); void showCurseForge(project); } }}>
          {project.icon_url ? <img alt="" src={project.icon_url} /> : <span className="project-icon">{project.title[0]}</span>}
          <div><h2>{project.title} <small>от {project.author}</small></h2><p>{project.description}</p><div className="project-tags">{project.categories.slice(0, 4).map((value) => <span key={value}>{value}</span>)}</div></div>
          <button disabled={Boolean(installTask) || busy === project.project_id || Boolean(targetBuildId && installed.some((item) => item.projectId === project.project_id))} onClick={(event) => { event.stopPropagation(); void requestInstall(project, "curseforge"); }} type="button"><KvanthIcon name={targetBuildId && installed.some((item) => item.projectId === project.project_id) ? "confirm" : "download"} size={18} /> {targetBuildId && installed.some((item) => item.projectId === project.project_id) ? "Установлено" : installTask?.project.project_id === project.project_id ? "Установка…" : "Установить"}</button>
        </article>)}{!busy && !error && projects.length === 0 && <div className="catalog-empty">Ничего не найдено.</div>}</div>
        {totalHits > 20 && <nav aria-label="Страницы CurseForge" className="catalog-pagination"><button disabled={page === 0} onClick={() => void search(page - 1)} type="button">Назад</button><span>{page + 1} / {pageCount}</span><button disabled={page + 1 >= pageCount} onClick={() => void search(page + 1)} type="button">Далее</button></nav>}
      </div> : <>
      <form className="catalog-search is-simple" onSubmit={(event) => { event.preventDefault(); void search(0); }}>
        <input aria-label="Поиск Modrinth" placeholder={`Поиск: ${tabs.find((tab) => tab.id === type)?.label.toLowerCase()}…`} value={query} onChange={(event) => setQuery(event.target.value)} />
        <button type="submit"><KvanthIcon name="search" size={18} /> Найти</button>
      </form>
      {error && <div className="catalog-error" role="alert">{error}</div>}

      <div className="content-layout">
        <div className="project-list" aria-busy={busy === "search"}>
          {busy === "search" && projects.length === 0 ? <div className="catalog-empty">Загружаем Modrinth…</div> : projects.filter((project) => !hideInstalled || !installed.some((item) => item.projectId === project.project_id)).map((project) => {
            const already = installed.some((item) => item.projectId === project.project_id);
            return <article className="project-card" key={project.project_id} onClick={() => void openProject(project)} tabIndex={0} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") void openProject(project); }}>
              {project.icon_url ? <img alt="" src={project.icon_url} /> : <span className="project-icon">{project.title.slice(0, 1)}</span>}
              <div><h2>{project.title} <small>от {project.author}</small></h2><p>{project.description}</p><div className="project-tags">{project.categories.slice(0, 4).map((category) => <span key={category}>{category}</span>)}<span><KvanthIcon name="download" size={14} /> {compact(project.downloads)}</span></div></div>
              <button disabled={((type === "modpack" || targetBuildId) && already) || Boolean(installTask) || busy === project.project_id} onClick={(event) => { event.stopPropagation(); void requestInstall(project, "modrinth"); }} type="button"><KvanthIcon name={(type === "modpack" || targetBuildId) && already ? "confirm" : "download"} size={18} /> {(type === "modpack" || targetBuildId) && already ? "Установлено" : installTask?.project.project_id === project.project_id ? "Установка…" : "Установить"}</button>
            </article>;
          })}
          {!busy && projects.length === 0 && <div className="catalog-empty">По вашему запросу ничего не найдено.</div>}
        </div>
        <aside className="catalog-filter-panel">
          <div className="filter-panel-heading"><strong>Фильтры</strong><button onClick={resetFilters} type="button"><KvanthIcon name="refresh" size={18} /> Сбросить</button></div>
          <label className="filter-switch"><input checked={hideInstalled} onChange={(event) => setHideInstalled(event.target.checked)} type="checkbox" /><span>Скрыть установленное</span></label><hr />
          <div className="form-field"><span>Сортировка</span><AppSelect ariaLabel="Сортировка каталога" onChange={setSort} options={[{ value: "relevance", label: "По релевантности" }, { value: "downloads", label: "По загрузкам" }, { value: "follows", label: "По подписчикам" }, { value: "newest", label: "Сначала новые" }, { value: "updated", label: "Недавно обновлённые" }]} value={sort} /></div>
          {targetBuildId ? <div className="scoped-catalog-locks"><span>Minecraft {gameVersion}</span>{type === "mod" && <span>{loaderLabel(loader)}</span>}</div> : <div className="form-field"><span>Версия Minecraft</span><AppSelect ariaLabel="Версия Minecraft каталога" onChange={setGameVersion} options={[{ value: "", label: "Все версии" }, ...versions.slice(0, 60).map((version) => ({ value: version.id, label: version.id }))]} value={gameVersion} /></div>}
          <div className="form-field"><span>Категория</span><AppSelect ariaLabel="Категория каталога" onChange={setCategory} options={[{ value: "", label: "Все категории" }, ...categoryOptions(type).map(([value, label]) => ({ value, label }))]} value={category} /></div>
          <div className="form-field"><span>Среда</span><AppSelect ariaLabel="Среда каталога" onChange={setEnvironment} options={[{ value: "", label: "Любая" }, { value: "client", label: "Клиент" }, { value: "server", label: "Сервер" }]} value={environment} /></div>
          {!targetBuildId && <><hr /><div className="filter-loader-list"><strong>Загрузчик</strong>{[["", "Все"], ["fabric", "Fabric"], ["quilt", "Quilt"], ["forge", "Forge"], ["neoforge", "NeoForge"], ["vanilla", "Vanilla"]].map(([value, label]) => <button className={loader === value ? "active" : ""} disabled={type === "resourcepack" || type === "shader"} key={value || "all"} onClick={() => setLoader(value)} type="button">{label}</button>)}</div></>}
        </aside>
      </div>
      {totalHits > 20 && <nav aria-label="Страницы каталога" className="catalog-pagination"><button aria-label="Предыдущая страница" disabled={page === 0} onClick={() => void search(page - 1)} type="button"><KvanthIcon name="back" size={16} /></button>{paginationItems(page, pageCount).map((item, index) => item === "…" ? <span key={`gap-${index}`}>…</span> : <button aria-current={item === page ? "page" : undefined} className={item === page ? "active" : ""} key={item} onClick={() => void search(item)} type="button">{item + 1}</button>)}<button aria-label="Следующая страница" disabled={page + 1 >= pageCount} onClick={() => void search(page + 1)} type="button"><KvanthIcon name="forward" size={16} /></button></nav>}
      </>}
      </div></div>
      {renderInstallDialog()}
    </section>
  );
}

function baseVersion(build: BuildSummary) { const prefix = build.loaderVersion ? `${build.loader}-loader-${build.loaderVersion}-` : ""; return prefix && build.gameVersion.startsWith(prefix) ? build.gameVersion.slice(prefix.length) : build.gameVersion; }
function supportsBuild(version: ModrinthVersion, build: BuildSummary, kind: ModrinthProjectType) {
  if (!version.game_versions.includes(baseVersion(build))) return false;
  return kind !== "mod" || version.loaders.length === 0 || version.loaders.includes(build.loader);
}
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
