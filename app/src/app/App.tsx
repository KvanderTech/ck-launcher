import { useEffect, useRef, useState } from "react";

import { BackgroundCarousel } from "../components/BackgroundCarousel";
import { Sidebar, type PageId } from "../components/Sidebar";
import { WindowControls } from "../components/WindowControls";
import { MicrosoftLogin } from "../features/accounts/MicrosoftLogin";
import { OfflineLogin } from "../features/accounts/OfflineLogin";
import { ContentPage } from "../features/content/ContentPage";
import { SkinsPage } from "../features/skins/SkinsPage";
import { HomePage, type LauncherViewState } from "../features/home/HomePage";
import { JavaSettings } from "../features/settings/JavaSettings";
import { MemorySettings } from "../features/settings/MemorySettings";
import "../styles/tokens.css";
import "../styles/launcher.css";
import { appApi, windowApi, type AppApi } from "./tauri";
import type {
  AccountSummary,
  GameExitedEvent,
  GameStartedEvent,
  GameVersionSummary,
  JavaMajor,
  JavaRuntimeStatus,
  LauncherErrorDto,
  LauncherErrorEvent,
  LauncherProfile,
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

export default function App({ api = appApi }: AppProps) {
  const [activePage, setActivePage] = useState<PageId>("home");
  const [accounts, setAccounts] = useState<AccountSummary[]>([]);
  const [versions, setVersions] = useState<GameVersionSummary[]>([]);
  const [profile, setProfile] = useState<LauncherProfile>();
  const [runtimes, setRuntimes] = useState<JavaRuntimeStatus[]>([]);
  const [requiredJava, setRequiredJava] = useState<JavaMajor>();
  const [bootState, setBootState] = useState<BootState>("loading");
  const [viewState, setViewState] = useState<LauncherViewState>("ready");
  const [progress, setProgress] = useState<ProgressEvent>();
  const [operationError, setOperationError] = useState<LauncherErrorDto>();
  const [operationWarning, setOperationWarning] = useState<LauncherErrorDto>();
  const [operationLogPath, setOperationLogPath] = useState<string>();
  const [cancelling, setCancelling] = useState(false);
  const [memorySaveState, setMemorySaveState] = useState<MemorySaveState>("idle");
  const operationId = useRef<OperationId | undefined>(undefined);
  const profileRef = useRef<LauncherProfile | undefined>(undefined);
  const awaitingOperationId = useRef(false);
  const bufferedOperationEvents = useRef<BufferedOperationEvent[]>([]);
  const requiredJavaRequest = useRef(0);
  const savedMemory = useRef<number | undefined>(undefined);
  const desiredMemory = useRef<number | undefined>(undefined);
  const memoryTimer = useRef<number | undefined>(undefined);
  const memoryPersistence = useRef<Promise<void> | undefined>(undefined);
  const memoryActive = useRef(true);

  function applyOperationEvent(event: BufferedOperationEvent) {
    switch (event.kind) {
      case "progress":
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
        setViewState("running");
        setOperationError(undefined);
        setProgress(undefined);
        break;
      case "exited":
        operationId.current = undefined;
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
        setOperationError(event.value.error);
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
    void Promise.all([
      api.listAccounts(),
      api.listGameVersions(),
      api.getProfile(),
      api.runtimeStatuses(),
    ]).then(
      ([nextAccounts, nextVersions, nextProfile, nextRuntimes]) => {
        if (!active) return;
        setAccounts(nextAccounts);
        setVersions(nextVersions.filter((version) => version.type === "release"));
        setProfile(nextProfile);
        profileRef.current = nextProfile;
        savedMemory.current = nextProfile.memoryMb;
        desiredMemory.current = nextProfile.memoryMb;
        setRuntimes(nextRuntimes);
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

  useEffect(() => {
    memoryActive.current = true;
    return () => {
      memoryActive.current = false;
      if (memoryTimer.current !== undefined) window.clearTimeout(memoryTimer.current);
    };
  }, []);

  useEffect(() => {
    const versionId = profile?.versionId;
    const request = ++requiredJavaRequest.current;
    setRequiredJava(undefined);
    if (!versionId) return;
    void api.requiredJavaForVersion(versionId).then(
      (requirement) => {
        if (request === requiredJavaRequest.current) setRequiredJava(requirement);
      },
      () => {
        if (request === requiredJavaRequest.current) setRequiredJava(undefined);
      },
    );
  }, [api, profile?.versionId]);

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

  function updateVersion(versionId: string) {
    const current = profileRef.current;
    if (!current) return;
    const next = { ...current, versionId };
    profileRef.current = next;
    setProfile(next);
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

  async function cancelCurrentOperation() {
    if (!operationId.current || cancelling) return;
    setCancelling(true);
    try {
      await api.cancelOperation(operationId.current);
    } finally {
      setCancelling(false);
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

  return (
    <div className="launcher-shell">
      <BackgroundCarousel />
      <Sidebar
        accountApi={api}
        accounts={accounts}
        activePage={activePage}
        onAccountAdded={accountAdded}
        onActiveAccountChange={activeAccountChanged}
        onNavigate={setActivePage}
      />
      <main className="main-pane">
        <header
          className="topbar"
          onMouseDown={(event) => {
            if (event.button !== 0 || (event.target as HTMLElement).closest(".window-controls")) return;
            void windowApi.startDragging();
          }}
        >
          <div
            className="topbar-drag-region"
            data-tauri-drag-region
            onDoubleClick={() => void windowApi.toggleMaximize()}
          >
            <span data-tauri-drag-region>ЦК Лаунчер</span>
          </div>
          <WindowControls />
        </header>
        <div className="page-scroll">
          {signedOut ? (
            <section className="signed-out-panel">
              <span className="eyebrow">Лицензионный аккаунт</span>
              <h1>Войдите, чтобы продолжить</h1>
              <p>Авторизация откроется в системном браузере. Пароль и refresh-токен не передаются интерфейсу.</p>
              <MicrosoftLogin api={api} onAuthenticated={accountAdded} />
              <div className="auth-divider"><span>или</span></div>
              <OfflineLogin api={api} onAuthenticated={accountAdded} />
            </section>
          ) : activePage === "home" ? (
            <HomePage
              error={operationError}
              logPath={operationLogPath}
              warning={operationWarning}
              cancelling={cancelling}
              onCancel={() => void cancelCurrentOperation()}
              onPlay={() => void startPlay()}
              onOpenLog={() => void openLatestGameLog()}
              onRetry={() => void startPlay()}
              onVersionChange={updateVersion}
              profile={profile}
              progress={progress}
              requiredJava={requiredJava}
              runtimes={runtimes}
              state={viewState}
              versions={versions}
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
              versions={versions}
              onBuildSelected={async () => {
                const next = await api.getProfile();
                profileRef.current = next;
                setProfile(next);
              }}
            />
          ) : activePage === "skins" ? (
            <SkinsPage api={api} account={accounts.find((account) => account.isActive) ?? accounts[0]} />
          ) : null}
        </div>
      </main>
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
          <section className="settings-card">
            <h2>Фоновые кадры</h2>
            <p>Затемнённые кадры меняются каждые 12 секунд. При уменьшенном движении смена отключена.</p>
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
