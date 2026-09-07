import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";

import { BackgroundCarousel } from "../components/BackgroundCarousel";
import { GameActivity } from "../components/GameActivity";
import { Sidebar, type PageId } from "../components/Sidebar";
import { playSound, SoundEffects } from "../components/SoundEffects";
import { WindowControls } from "../components/WindowControls";
import { MicrosoftLogin } from "../features/accounts/MicrosoftLogin";
import { ContentPage, type ContentInstallTask } from "../features/content/ContentPage";
import { LibraryPage } from "../features/library/LibraryPage";
const SkinsPage = lazy(() => import("../features/skins/SkinsPage").then(module => ({ default: module.SkinsPage })));
import { HomePage, type LauncherViewState } from "../features/home/HomePage";
import { JavaSettings } from "../features/settings/JavaSettings";
import { MemorySettings } from "../features/settings/MemorySettings";
import { check, type Update } from "@tauri-apps/plugin-updater";
import "../styles/tokens.css";
import "../styles/launcher.css";
import "../styles/instance-repair.css";
import "../styles/home-redesign.css";
import "../styles/interface-redesign.css";
import { appApi, windowApi, type AppApi } from "./tauri";
import type {
  AccountSummary,
  BuildSummary,
  GameExitedEvent,
  GameStartedEvent,
  GameVersionSummary,
  JavaMajor,
  JavaRuntimeStatus,
  LauncherErrorDto,
  LauncherErrorEvent,
  LauncherProfile,
  MinecraftCosmetics,
  ModrinthProject,
  OfflineSkin,
  OperationId,
  ProgressEvent,
} from "./types";

interface AppProps {
  api?: AppApi;
}

type BootState = "loading" | "loaded" | "failed";
type MemorySaveState = "idle" | "saving" | "error";
type BufferedOperationEvent =
  | { kind: "progress"; value: ProgressEvent }
  | { kind: "started"; value: GameStartedEvent }
  | { kind: "exited"; value: GameExitedEvent }
  | { kind: "error"; value: LauncherErrorEvent };

const texturePreloadCache = new Map<string, HTMLImageElement>();

function preloadTexture(url: string) {
  const secure = url.replace("http://", "https://");
  if (texturePreloadCache.has(secure)) return;
  const image = new Image();
  image.decoding = "async";
  image.src = secure;
  texturePreloadCache.set(secure, image);
}

export default function App({ api = appApi }: AppProps) {
  const [activePage, setActivePage] = useState<PageId>("home");
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [versions, setVersions] = useState<GameVersionSummary[]>([]);
  const [versionError, setVersionError] = useState(false);
  const [builds, setBuilds] = useState<BuildSummary[]>([]);
  const [profile, setProfile] = useState<LauncherProfile>();
  const [runtimes, setRuntimes] = useState<JavaRuntimeStatus[]>([]);
  const [bootState, setBootState] = useState<BootState>("loading");
  const [cancelling, setCancelling] = useState(false);
  const [viewState, setViewState] = useState<LauncherViewState>("ready");
  const [runningGame, setRunningGame] = useState<GameStartedEvent>();
  const [progress, setProgress] = useState<ProgressEvent>();
  const [operationError, setOperationError] = useState<LauncherErrorDto>();
  const [operationWarning, setOperationWarning] = useState<LauncherErrorDto>();
  const [operationLogPath, setOperationLogPath] = useState<string>();
  const [memorySaveState, setMemorySaveState] = useState<MemorySaveState>("idle");
  const [contentInstallTask, setContentInstallTask] = useState<ContentInstallTask>();
  const [libraryTarget, setLibraryTarget] = useState<{ id: string; nonce: number }>();
  const [catalogForBuild, setCatalogForBuild] = useState(false);
  const [contentNonce, setContentNonce] = useState(0);
  const [createBuildOnOpen, setCreateBuildOnOpen] = useState(false);
  const [deleteTask, setDeleteTask] = useState<{ build: BuildSummary; deleting?: boolean; error?: string }>();
  const [skinLibraries, setSkinLibraries] = useState<Record<string, OfflineSkin[]>>({});
  const [cosmeticsByAccount, setCosmeticsByAccount] = useState<Record<string, MinecraftCosmetics>>({});
  const [cosmeticsErrors, setCosmeticsErrors] = useState<Record<string, string | undefined>>({});
  const [cosmeticsLoading, setCosmeticsLoading] = useState<Record<string, boolean>>({});
  const cosmeticsRequests = useRef(new Map<string, Promise<void>>());
  const operationId = useRef<OperationId | undefined>(undefined);
  const startedOperationId = useRef<OperationId | undefined>(undefined);
  const profileRef = useRef<LauncherProfile | undefined>(undefined);
  const awaitingOperationId = useRef(false);
  const bufferedOperationEvents = useRef<BufferedOperationEvent[]>([]);
  const savedMemory = useRef<number | undefined>(undefined);
  const desiredMemory = useRef<number | undefined>(undefined);
  const memoryTimer = useRef<number | undefined>(undefined);
  const memoryPersistence = useRef<Promise<void> | undefined>(undefined);
  const memoryActive = useRef(true);

  const warmCosmetics = useCallback((accountId: string) => {
    const pending = cosmeticsRequests.current.get(accountId);
    if (pending) return pending;
    setCosmeticsLoading((current) => ({ ...current, [accountId]: true }));
    const request = Promise.allSettled([
      api.listOfflineSkins(accountId),
      api.minecraftCosmetics(accountId),
    ]).then(([skinsResult, cosmeticsResult]) => {
      if (skinsResult.status === "fulfilled") {
        setSkinLibraries((current) => ({ ...current, [accountId]: skinsResult.value }));
        skinsResult.value.forEach((skin) => preloadTexture(skin.dataUrl));
      }
      if (cosmeticsResult.status === "fulfilled") {
        setCosmeticsByAccount((current) => ({ ...current, [accountId]: cosmeticsResult.value }));
        cosmeticsResult.value.skins.forEach((skin) => preloadTexture(skin.url));
        cosmeticsResult.value.capes.forEach((cape) => preloadTexture(cape.url));
      }
      const failed = skinsResult.status === "rejected" || cosmeticsResult.status === "rejected";
      setCosmeticsErrors((current) => ({
        ...current,
        [accountId]: failed ? "Не удалось обновить данные профиля. Показаны последние загруженные данные." : undefined,
      }));
    }).finally(() => {
      cosmeticsRequests.current.delete(accountId);
      setCosmeticsLoading((current) => ({ ...current, [accountId]: false }));
    });
    cosmeticsRequests.current.set(accountId, request);
    return request;
  }, [api]);

  const syncBuildSelection = useCallback(async () => {
    const [nextProfile, nextBuilds] = await Promise.all([api.getProfile(), api.listBuilds()]);
    profileRef.current = nextProfile;
    setProfile(nextProfile);
    setBuilds(nextBuilds);
  }, [api]);

  const importAssociatedMrpack = useCallback(async (sourcePath: string) => {
    const project: ModrinthProject = { project_id: "associated-mrpack", project_type: "modpack", title: "Сборка из файла", description: "Импорт файла .mrpack", author: "Локальный файл", categories: [], versions: [], downloads: 0, follows: 0, date_modified: "" };
    setContentInstallTask({ project, stage: "Установка сборки", step: 1 });
    try {
      const result = await api.importMrpack(sourcePath);
      if (!result) { setContentInstallTask(undefined); return; }
      playSound("install-complete");
      setContentInstallTask({ project: { ...project, title: result.title }, stage: "Сборка установлена", step: 3 });
      await syncBuildSelection();
      setActivePage("library");
      window.setTimeout(() => setContentInstallTask(undefined), 1200);
    } catch (reason) {
      const message = reason && typeof reason === "object" && "message" in reason && typeof reason.message === "string" ? reason.message : "Не удалось установить файл .mrpack.";
      setContentInstallTask({ project, stage: "Установка не завершена", step: 0, error: message });
    }
  }, [api, syncBuildSelection]);

  useEffect(() => {
    if (!api.pendingMrpackPath || !api.onOpenMrpack) return;
    let active = true;
    const registration = api.onOpenMrpack((path) => { if (active) void importAssociatedMrpack(path); });
    void api.pendingMrpackPath().then((path) => { if (active && path) void importAssociatedMrpack(path); });
    return () => { active = false; void registration.then((stop) => stop()); };
  }, [api, importAssociatedMrpack]);

  async function openSidebarBuild(buildId: string) {
    await api.selectBuild(buildId);
    playSound("build-switch");
    await syncBuildSelection();
    setLibraryTarget({ id: buildId, nonce: Date.now() });
    setActivePage("library");
  }

  async function confirmBuildDelete() {
    if (!deleteTask || deleteTask.deleting) return;
    setDeleteTask({ ...deleteTask, deleting: true, error: undefined });
    try {
      await api.deleteBuild(deleteTask.build.id);
      await syncBuildSelection();
      setLibraryTarget({ id: "", nonce: Date.now() });
      setDeleteTask(undefined);
    } catch (reason) {
      const error = reason && typeof reason === "object" && "message" in reason && typeof reason.message === "string" ? reason.message : "Не удалось удалить сборку.";
      setDeleteTask({ build: deleteTask.build, error });
    }
  }

  function applyOperationEvent(event: BufferedOperationEvent) {
    switch (event.kind) {
      case "progress":
        if (event.value.operationId === startedOperationId.current) break;
        setProgress(event.value);
        if ([
          "authenticating",
          "resolving-metadata",
          "resolving-java",
          "checking",
          "downloading",
          "installing",
        ].includes(event.value.stage)) setViewState("installing");
        if (event.value.stage === "launching") setViewState("launching");
        if (event.value.stage === "running") setViewState("running");
        break;
      case "started":
        startedOperationId.current = event.value.operationId;
        playSound("game-ready");
        setRunningGame(event.value);
        setViewState("running");
        setOperationError(undefined);
        setProgress(undefined);
        break;
      case "exited":
        playSound("game-exit");
        setRunningGame(undefined);
        operationId.current = undefined;
        startedOperationId.current = undefined;
        setOperationError(undefined);
        setOperationWarning(undefined);
        setOperationLogPath(undefined);
        setViewState("ready");
        setProgress(undefined);
        break;
      case "error":
        if ("terminal" in event.value && event.value.terminal === false) {
          setOperationWarning(event.value.error);
          break;
        }
        if (startedOperationId.current === event.value.operationId) playSound("game-exit");
        setOperationError(event.value.error);
        setRunningGame(undefined);
        setOperationLogPath(event.value.logPath);
        setViewState(event.value.error.recoverable ? "recoverable-error" : "fatal-error");
        setProgress(undefined);
        break;
    }
  }

  function receiveOperationEvent(event: BufferedOperationEvent) {
    if (event.value.operationId === operationId.current) {
      applyOperationEvent(event);
    } else if (awaitingOperationId.current && operationId.current === undefined) {
      bufferedOperationEvents.current.push(event);
    }
  }

  useEffect(() => {
    let active = true;
    void api.listGameVersions().then(
      (items) => { if (active) { setVersions(items.filter(v => v.type === "release")); setVersionError(false); } },
      () => { if (active) setVersionError(true); },
    );
    void Promise.all([
      api.listAccounts(),
      api.getProfile(),
      api.runtimeStatuses(),
      api.listBuilds(),
    ]).then(
      ([nextAccounts, nextProfile, nextRuntimes, nextBuilds]) => {
        if (!active) return;
        setAccounts(nextAccounts);
        setProfile(nextProfile);
        profileRef.current = nextProfile;
        savedMemory.current = nextProfile.memoryMb;
        desiredMemory.current = nextProfile.memoryMb;
        setRuntimes(nextRuntimes);
        setBuilds(nextBuilds);
        setBootState("loaded");
      },
      () => {
        if (!active) return;
        setBootState("failed");
      },
    );
    return () => {
      active = false;
    };
  }, [api]);

  const activeAccountId = accounts.find((account) => account.isActive)?.id ?? accounts[0]?.id;
  useEffect(() => {
    if (activeAccountId) void warmCosmetics(activeAccountId);
  }, [activeAccountId, warmCosmetics]);

  useEffect(() => {
    memoryActive.current = true;
    return () => {
      memoryActive.current = false;
      if (memoryTimer.current !== undefined) window.clearTimeout(memoryTimer.current);
    };
  }, []);

  useEffect(() => {
    let active = true;
    const registrations = [
      api.onProgress((event) => {
        receiveOperationEvent({ kind: "progress", value: event });
      }),
      api.onGameStarted((event) => {
        receiveOperationEvent({ kind: "started", value: event });
      }),
      api.onGameExited((event) => {
        receiveOperationEvent({ kind: "exited", value: event });
      }),
      api.onLauncherError((event) => {
        receiveOperationEvent({ kind: "error", value: event });
      }),
    ];
    void Promise.all(registrations).then((unlisten) => {
      if (!active) unlisten.forEach((stop) => stop());
    });
    return () => {
      active = false;
      void Promise.all(registrations).then((unlisten) => unlisten.forEach((stop) => stop()));
    };
  }, [api]);

  async function startPlay() {
    if (!profileRef.current?.versionId || ["installing", "launching", "running"].includes(viewState)) return;
    setOperationError(undefined);
    setOperationWarning(undefined);
    setOperationLogPath(undefined);
    setProgress(undefined);
    setViewState("launching");
    try {
      try {
        await flushDesiredMemory();
      } catch {
        throw {
          code: "memory_save_failed",
          message: "Не удалось сохранить память перед запуском.",
          recoverable: true,
        } satisfies LauncherErrorDto;
      }
      const confirmedProfile = profileRef.current;
      if (!confirmedProfile?.versionId) return;
      await api.updateProfile(confirmedProfile);
      operationId.current = undefined;
      startedOperationId.current = undefined;
      awaitingOperationId.current = true;
      bufferedOperationEvents.current = [];
      const nextOperationId = await api.launchOrInstall(confirmedProfile.id);
      operationId.current = nextOperationId;
      awaitingOperationId.current = false;
      const matchingEvents = bufferedOperationEvents.current.filter(
        (event) => event.value.operationId === nextOperationId,
      );
      bufferedOperationEvents.current = [];
      matchingEvents.forEach(applyOperationEvent);
    } catch (error: unknown) {
      awaitingOperationId.current = false;
      bufferedOperationEvents.current = [];
      const safeError = launcherErrorFrom(error);
      setOperationError(safeError);
      setViewState(safeError.recoverable ? "recoverable-error" : "fatal-error");
    }
  }

  function mergeMemory(memoryMb: number) {
    const current = profileRef.current;
    if (!current) return;
    const next = { ...current, memoryMb };
    profileRef.current = next;
    setProfile(next);
  }

  function changeMemory(memoryMb: number) {
    desiredMemory.current = memoryMb;
    mergeMemory(memoryMb);
    if (memoryTimer.current !== undefined) window.clearTimeout(memoryTimer.current);
    if (memoryPersistence.current) return;
    setMemorySaveState("idle");
    memoryTimer.current = window.setTimeout(() => {
      memoryTimer.current = undefined;
      void persistDesiredMemory().catch(() => undefined);
    }, 250);
  }

  function persistDesiredMemory(): Promise<void> {
    if (memoryPersistence.current) return memoryPersistence.current;
    const persistence = runMemoryPersistence();
    memoryPersistence.current = persistence;
    void persistence.finally(() => {
      if (memoryPersistence.current === persistence) memoryPersistence.current = undefined;
    }).catch(() => undefined);
    return persistence;
  }

  async function runMemoryPersistence(): Promise<void> {
    while (memoryActive.current) {
      const requestedMemory = desiredMemory.current;
      if (requestedMemory === undefined || requestedMemory === savedMemory.current) {
        setMemorySaveState("idle");
        return;
      }
      setMemorySaveState("saving");
      try {
        const saved = await api.updateMemory(requestedMemory);
        if (!memoryActive.current) return;
        savedMemory.current = saved.memoryMb;
        if (desiredMemory.current === requestedMemory) {
          desiredMemory.current = saved.memoryMb;
          mergeMemory(saved.memoryMb);
        }
      } catch (error: unknown) {
        if (!memoryActive.current) return;
        if (desiredMemory.current !== requestedMemory) continue;
        desiredMemory.current = savedMemory.current;
        if (savedMemory.current !== undefined) mergeMemory(savedMemory.current);
        setMemorySaveState("error");
        throw error;
      }
    }
  }

  async function flushDesiredMemory(): Promise<void> {
    if (memoryTimer.current !== undefined) {
      window.clearTimeout(memoryTimer.current);
      memoryTimer.current = undefined;
    }
    await persistDesiredMemory();
  }

  async function updateRuntime(
    requirement: JavaMajor,
    action: (requirement: JavaMajor) => Promise<JavaRuntimeStatus | null>,
  ) {
    const pending = runtimes.map((runtime) =>
      runtime.requirement === requirement ? { ...runtime, state: "installing" as const } : runtime,
    );
    setRuntimes(pending);
    try {
      const result = await action(requirement);
      if (result) setRuntimes((current) => replaceRuntime(current, result));
      else setRuntimes(runtimes);
    } catch {
      setRuntimes((current) => replaceRuntime(current, { requirement, state: "invalid" }));
    }
  }

  async function openLatestGameLog() {
    try {
      await api.openLatestGameLog();
    } catch {
      setOperationWarning({
        code: "game_log_open_failed",
        message: "Не удалось открыть очищенный журнал Minecraft.",
        recoverable: true,
      });
    }
  }

  async function chooseGameDirectory() {
    const selected = await api.chooseGameDirectory();
    if (!selected) return;
    profileRef.current = selected;
    setProfile(selected);
  }

  function accountAdded(account: AccountSummary) {
    setAccounts((current) => [
      ...current.map((item) => ({ ...item, isActive: false })),
      { ...account, isActive: true },
    ]);
  }

  function activeAccountChanged(account: AccountSummary) {
    setAccounts((current) => current.map((item) => ({ ...item, isActive: item.id === account.id })));
  }

  function accountRemoved(accountId: string) {
    setAccounts((current) => {
      const remaining = current.filter((account) => account.id !== accountId);
      if (!remaining.length || remaining.some((account) => account.isActive)) return remaining;
      return remaining.map((account, index) => ({ ...account, isActive: index === 0 }));
    });
  }

  if (bootState === "loading") {
    return <main aria-live="polite" className="boot-screen"><span className="boot-mark">ЦК</span><p>Подготавливаем лаунчер…</p></main>;
  }

  if (bootState === "failed" || !profile) {
    return (
      <main className="boot-screen">
        <span className="boot-mark">ЦК</span>
        <h1>Не удалось подготовить лаунчер</h1>
        <p role="alert">Перезапустите приложение. Технические сведения не показываются в интерфейсе.</p>
      </main>
    );
  }

  const signedOut = accounts.length === 0;
  const activeBuild = builds.find((build) => build.isActive);
  const runningBuild = runningGame
    ? builds.find((build) => build.id === runningGame.profileId) ?? activeBuild
    : undefined;

  return (
    <div className={`launcher-shell is-compact${activePage === "home" ? " is-home" : ""}`}>
      <SoundEffects />
      <BackgroundCarousel />
      <Sidebar
        accountApi={api}
        accounts={accounts}
        activePage={activePage}
        builds={builds}
        onAccountAdded={accountAdded}
        onAccountRemoved={accountRemoved}
        onActiveAccountChange={activeAccountChanged}
        onNavigate={(page) => { if (page === "content") { setCatalogForBuild(false); setCreateBuildOnOpen(false); setContentNonce((value) => value + 1); } setActivePage(page); }}
        onOpenBuild={(buildId) => void openSidebarBuild(buildId)}
      />
      <main className="main-pane">
        <header
          className="topbar"
          onMouseDown={(event) => {
            if (event.button !== 0 || (event.target as HTMLElement).closest(".window-controls, .game-activity, .game-console-backdrop")) return;
            void windowApi.startDragging();
          }}
        >
          <div
            className="topbar-drag-region"
            data-tauri-drag-region
            onDoubleClick={() => void windowApi.toggleMaximize()}
          >
          </div>
          {runningGame && viewState === "running" && (
            <GameActivity
              api={api}
              game={runningGame}
              iconUrl={runningBuild?.iconUrl}
              name={runningBuild?.name ?? profile.name}
            />
          )}
          {versionError && <div role="status" className="catalog-offline">Список версий недоступен. Установленные сборки доступны. <button type="button" onClick={() => { void api.listGameVersions().then(items => { setVersions(items.filter(v => v.type === "release")); setVersionError(false); }).catch(() => setVersionError(true)); }}>Повторить</button></div>}
        <WindowControls />
        </header>
        <div className="page-scroll">
          {signedOut ? (
            <section className="signed-out-panel">
              <span className="eyebrow">Лицензионный аккаунт</span>
              <h1>Войдите, чтобы продолжить</h1>
              <p>Авторизация откроется в системном браузере. Пароль и refresh-токен не передаются интерфейсу.</p>
              <MicrosoftLogin api={api} onAuthenticated={accountAdded} />
            </section>
          ) : activePage === "home" ? (
            <HomePage
              error={operationError}
              logPath={operationLogPath}
              warning={operationWarning}
              onPlay={() => builds.length ? void startPlay() : setActivePage("library")}
              onOpenLog={() => void openLatestGameLog()}
              onOpenExternal={(url) => void api.openExternalUrl(url)}
              onRetry={() => void startPlay()}
              state={viewState}
            />
          ) : activePage === "settings" ? (
            <SettingsPage
              api={api}
              memoryMb={profile.memoryMb}
              gameDir={profile.gameDir}
              memorySaveState={memorySaveState}
              onMemoryChange={changeMemory}
              onChooseGameDirectory={() => void chooseGameDirectory()}
              onRuntimeAction={(requirement, action) => void updateRuntime(requirement, action)}
              runtimes={runtimes}
            />
          ) : activePage === "content" ? (
            <ContentPage
              api={api}
              forBuild={catalogForBuild}
              key={`content-${contentNonce}-${catalogForBuild ? "build" : "root"}`}
              versions={versions}
              installTask={contentInstallTask}
              startCreating={createBuildOnOpen}
              onInstallTaskChange={setContentInstallTask}
              onBuildSelected={syncBuildSelection}
            />
          ) : activePage === "library" ? (
            <LibraryPage
              initialBuildId={libraryTarget?.id}
              key={libraryTarget?.nonce ?? "library"}
              api={api}
              onBuildSelected={syncBuildSelection}
              onOpenCatalog={() => { setCatalogForBuild(true); setActivePage("content"); }}
              onCreateBuild={() => { setCatalogForBuild(false); setCreateBuildOnOpen(true); setContentNonce((value) => value + 1); setActivePage("content"); }}
              onPlay={startPlay}
              onRequestDelete={(build) => setDeleteTask({ build })}
            />
          ) : activePage === "skins" ? (
            <Suspense fallback={<p>Загружаем библиотеку скинов…</p>}><SkinsPage
              api={api}
              account={accounts.find((account) => account.isActive) ?? accounts[0]}
              cosmetics={cosmeticsByAccount[activeAccountId!]}
              error={cosmeticsErrors[activeAccountId!]}
              loading={cosmeticsLoading[activeAccountId!] === true}
              onCosmeticsChange={(next) => setCosmeticsByAccount((current) => ({ ...current, [activeAccountId!]: next }))}
              onRefresh={() => void warmCosmetics(activeAccountId!)}
              onSkinsChange={(next) => setSkinLibraries((current) => ({ ...current, [activeAccountId!]: next }))}
              skins={skinLibraries[activeAccountId!] ?? []}
            /></Suspense>
          ) : null}
        </div>
      </main>
      {contentInstallTask && <aside className={`content-install-toast global-install-toast${contentInstallTask.error ? " is-error" : ""}`} role={contentInstallTask.error ? "alert" : "status"}>{contentInstallTask.project.icon_url ? <img alt="" src={contentInstallTask.project.icon_url} /> : <span>{contentInstallTask.project.title[0]}</span>}<div><strong>{contentInstallTask.project.title}</strong><p>{contentInstallTask.error ?? contentInstallTask.stage}</p></div>{contentInstallTask.error ? <button aria-label="Закрыть сообщение об установке" onClick={() => setContentInstallTask(undefined)} type="button">×</button> : <><i /><small>{contentInstallTask.step}/3</small></>}</aside>}
      {progress && (viewState === "installing" || viewState === "launching") && (() => { const percent = progress.totalBytes > 0 ? Math.min(100, Math.floor(progress.completedBytes / progress.totalBytes * 100)) : 0; return <aside className="content-install-toast global-install-toast game-install-toast" role="status">{activeBuild?.iconUrl ? <img alt="" src={activeBuild.iconUrl} /> : <span>ЦК</span>}<div><strong>{activeBuild?.name ?? profile.name}</strong><p>{progressLabel(progress.stage)}</p><span className="sr-only">{progress.currentFile ?? "Подготавливаем операцию…"}</span></div><i /><small>{percent}%</small></aside>; })()}
      {(viewState === "installing" || viewState === "launching") && progress && <button className="cancel-workflow" type="button" disabled={cancelling} onClick={() => {
        const id = operationId.current; if (!id) return; setCancelling(true);
        void api.cancelOperation(id).catch((reason: LauncherErrorDto) => setOperationError(reason)).finally(() => setCancelling(false));
      }}>{cancelling ? "Отменяем…" : "Отменить"}</button>}
      {deleteTask && <aside className={`delete-build-toast${deleteTask.error ? " is-error" : ""}`} role={deleteTask.error ? "alert" : "dialog"}>{deleteTask.build.iconUrl ? <img alt="" src={deleteTask.build.iconUrl} /> : <span>{deleteTask.build.name[0]}</span>}<div><strong>{deleteTask.deleting ? "Удаляем сборку…" : `Удалить «${deleteTask.build.name}»?`}</strong><p>{deleteTask.error ?? "Сборка будет перемещена во внутреннюю корзину."}</p><div className="delete-toast-actions"><button disabled={deleteTask.deleting} onClick={() => setDeleteTask(undefined)} type="button">Отмена</button><button disabled={deleteTask.deleting} onClick={() => void confirmBuildDelete()} type="button">{deleteTask.deleting ? "Удаление…" : "Удалить"}</button></div></div></aside>}
    </div>
  );
}

interface SettingsPageProps {
  api: AppApi;
  gameDir: string;
  memoryMb: number;
  memorySaveState: MemorySaveState;
  onMemoryChange(memoryMb: number): void;
  onChooseGameDirectory(): void;
  onRuntimeAction(
    requirement: JavaMajor,
    action: (requirement: JavaMajor) => Promise<JavaRuntimeStatus | null>,
  ): void;
  runtimes: JavaRuntimeStatus[];
}

function SettingsPage({
  api,
  gameDir,
  memoryMb,
  memorySaveState,
  onMemoryChange,
  onChooseGameDirectory,
  onRuntimeAction,
  runtimes,
}: SettingsPageProps) {
  const [update, setUpdate] = useState<Update>();
  const [updateStatus, setUpdateStatus] = useState("Готово к проверке");
  const [updateBusy, setUpdateBusy] = useState(false);

  async function checkForUpdates() {
    setUpdateBusy(true);
    setUpdateStatus("Проверяем GitHub…");
    try {
      const available = await check();
      setUpdate(available ?? undefined);
      setUpdateStatus(available ? `Доступна версия ${available.version}` : "Установлена последняя версия");
    } catch {
      setUpdateStatus("Не удалось проверить обновления");
    } finally {
      setUpdateBusy(false);
    }
  }

  async function installUpdate() {
    if (!update) return;
    setUpdateBusy(true);
    let downloaded = 0;
    let total = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") downloaded += event.data.chunkLength;
        if (event.event === "Finished") setUpdateStatus("Запускаем установку…");
        else if (total > 0) setUpdateStatus(`Загрузка ${Math.min(100, Math.round(downloaded / total * 100))}%`);
      });
    } catch {
      setUpdateStatus("Обновление не установлено");
      setUpdateBusy(false);
    }
  }

  return (
    <section className="settings-page">
      <div className="page-heading"><span className="eyebrow">Параметры запуска</span><h1>Настройки</h1><p>Память и реальные установки Java сохраняются через ядро лаунчера.</p></div>
      <div className="settings-grid">
        <div className="settings-column">
          <MemorySettings
            api={api}
            memoryMb={memoryMb}
            onChange={onMemoryChange}
            saveState={memorySaveState}
          />
          <section className="settings-card">
            <h2>Папка игры</h2>
            <code>{gameDir}</code>
            <button onClick={onChooseGameDirectory} type="button">Выбрать папку игры</button>
          </section>
          <section className="settings-card update-card">
            <div><h2>Обновление лаунчера</h2><p>{updateStatus}</p></div>
            {update ? <button disabled={updateBusy} onClick={() => void installUpdate()} type="button">{updateBusy ? "Загрузка…" : `Обновить до ${update.version}`}</button> : <button disabled={updateBusy} onClick={() => void checkForUpdates()} type="button">{updateBusy ? "Проверяем…" : "Проверить обновления"}</button>}
          </section>
        </div>
        <div className="settings-card java-card-group">
          <div><h2>Установки Java</h2><p>Лаунчер проверяет только поддерживаемые Java 8, 17, 21 и 25.</p></div>
          <JavaSettings
            onChoose={(major) => onRuntimeAction(major, api.chooseRuntimePath)}
            onDetect={(major) => onRuntimeAction(major, api.detectRuntime)}
            onInstall={(major) => onRuntimeAction(major, api.installRuntime)}
            statuses={runtimes}
          />
        </div>
      </div>
    </section>
  );
}

function progressLabel(stage: ProgressEvent["stage"]) {
  const labels: Record<ProgressEvent["stage"], string> = {
    idle: "Ожидание", authenticating: "Проверяем аккаунт", "resolving-metadata": "Получаем метаданные",
    "resolving-java": "Подбираем Java", checking: "Проверяем файлы", downloading: "Загружаем файлы",
    installing: "Устанавливаем игру", launching: "Запускаем игру", running: "Игра запущена", failed: "Операция остановлена",
  };
  return labels[stage];
}

function launcherErrorFrom(error: unknown): LauncherErrorDto {
  if (typeof error === "object" && error !== null) {
    const candidate = error as Partial<LauncherErrorDto>;
    if (typeof candidate.code === "string" && typeof candidate.message === "string") {
      return {
        code: candidate.code,
        message: candidate.message,
        recoverable: candidate.recoverable === true,
      };
    }
  }
  return {
    code: "launch_failed",
    message: "Не удалось начать запуск.",
    recoverable: true,
  };
}

function replaceRuntime(statuses: JavaRuntimeStatus[], next: JavaRuntimeStatus) {
  const found = statuses.some((status) => status.requirement === next.requirement);
  return found
    ? statuses.map((status) => status.requirement === next.requirement ? next : status)
    : [...statuses, next];
}
