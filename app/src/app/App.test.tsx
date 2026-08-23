import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { StrictMode, type ReactElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import RootApp from "../App";
import type {
  AccountSummary,
  JavaRuntimeStatus,
  LauncherErrorEvent,
  LauncherProfile,
  ProgressEvent,
} from "./types";

type EventHandlers = {
  progress?: (event: ProgressEvent) => void;
  started?: (event: { operationId: string; profileId: string; pid: number }) => void;
  exited?: (event: { operationId: string; profileId: string; exitCode: number }) => void;
  error?: (event: LauncherErrorEvent) => void;
};

const accounts: AccountSummary[] = [
  {
    id: "kvander",
    minecraftName: "Kvander",
    minecraftUuid: "uuid-kvander",
    headUrl: "https://example.test/kvander.png",
    isActive: true,
  },
  {
    id: "comfort",
    minecraftName: "ComfortPlayer",
    minecraftUuid: "uuid-comfort",
    isActive: false,
  },
];

const runtimes: JavaRuntimeStatus[] = [
  {
    requirement: 21,
    state: "valid",
    source: "managed",
    version: "21.0.8",
    path: "C:\\safe\\javaw.exe",
  },
  { requirement: 8, state: "missing" },
];

function createApi(handlers: EventHandlers) {
  return {
    listAccounts: vi.fn(async () => accounts),
    beginMicrosoftLogin: vi.fn(async () => accounts[0]),
    createOfflineAccount: vi.fn(async () => accounts[0]),
    cancelMicrosoftLogin: vi.fn(async () => undefined),
    removeAccount: vi.fn(async () => undefined),
    setActiveAccount: vi.fn(async () => undefined),
    listGameVersions: vi.fn(async () => [
      { id: "1.21.8", type: "release", releaseDate: "2026-07-17T00:00:00Z" },
      { id: "1.20.1", type: "release", releaseDate: "2023-06-12T00:00:00Z" },
    ]),
    requiredJavaForVersion: vi.fn(async (versionId: string) => versionId === "1.20.1" ? 8 as const : 21 as const),
    getProfile: vi.fn(async () => ({
      id: "default",
      name: "Основной профиль",
      versionId: "1.20.1",
      memoryMb: 4096,
      gameDir: "C:\\safe\\game",
      javaOverride: null,
    })),
    updateProfile: vi.fn(async (profile) => profile),
    chooseGameDirectory: vi.fn(async () => null as LauncherProfile | null),
    updateMemory: vi.fn(async (memoryMb: number) => ({
      id: "default",
      name: "Основной профиль",
      versionId: "1.20.1",
      memoryMb,
      gameDir: "C:\\safe\\game",
      javaOverride: null,
    })),
    memoryStatus: vi.fn(async () => ({
      memoryMb: 4096,
      minMemoryMb: 512,
      maxMemoryMb: 12_288,
      stepMemoryMb: 512,
    })),
    runtimeStatuses: vi.fn(async () => runtimes),
    detectRuntime: vi.fn(async (requirement) => ({ requirement, state: "valid" as const })),
    installRuntime: vi.fn(async (requirement) => ({ requirement, state: "valid" as const })),
    chooseRuntimePath: vi.fn(async (requirement) => ({ requirement, state: "valid" as const })),
    launchOrInstall: vi.fn(async () => "operation-current"),
    cancelOperation: vi.fn(async () => undefined),
    openLatestGameLog: vi.fn(async () => undefined),
    onProgress: vi.fn(async (handler) => {
      handlers.progress = handler;
      return () => undefined;
    }),
    onGameStarted: vi.fn(async (handler) => {
      handlers.started = handler;
      return () => undefined;
    }),
    onGameExited: vi.fn(async (handler) => {
      handlers.exited = handler;
      return () => undefined;
    }),
    onLauncherError: vi.fn(async (handler) => {
      handlers.error = handler;
      return () => undefined;
    }),
  };
}

function renderApp(api: ReturnType<typeof createApi>) {
  const AppWithApi = RootApp as unknown as (props: { api: typeof api }) => ReactElement;
  return render(<AppWithApi api={api} />);
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("launcher application", () => {
  it("selects a stable version, launches the current profile, and shows current 50% progress", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);

    const version = await screen.findByRole("combobox", { name: "Версия Minecraft" });
    expect(screen.getByText("Kvander")).toBeTruthy();
    expect(screen.getByText("4096 МБ")).toBeTruthy();
    fireEvent.change(version, { target: { value: "1.21.8" } });
    fireEvent.click(screen.getByRole("button", { name: "Играть" }));

    await waitFor(() => expect(api.launchOrInstall).toHaveBeenCalledWith("default"));
    act(() => {
      handlers.progress?.({
        operationId: "operation-current",
        stage: "downloading",
        completedBytes: 50,
        totalBytes: 100,
        currentFile: "client.jar",
      });
    });

    expect(screen.getByText("50%")).toBeTruthy();
    expect(screen.getByText("client.jar")).toBeTruthy();
  });

  it("shows the current metadata stage and allows cancelling the active workflow", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Играть" }));
    await waitFor(() => expect(api.launchOrInstall).toHaveBeenCalledTimes(1));

    act(() => {
      handlers.progress?.({
        operationId: "operation-current",
        stage: "resolving-metadata" as ProgressEvent["stage"],
        completedBytes: 0,
        totalBytes: 0,
      });
    });

    expect(screen.getByText("Получаем метаданные")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Отменить" }));
    expect(api.cancelOperation).toHaveBeenCalledWith("operation-current");
  });

  it("allows cancellation while the workflow is still at the pre-spawn launching stage", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Играть" }));
    await waitFor(() => expect(api.launchOrInstall).toHaveBeenCalledTimes(1));

    act(() => {
      handlers.progress?.({
        operationId: "operation-current",
        stage: "launching",
        completedBytes: 0,
        totalBytes: 0,
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "Отменить" }));
    expect(api.cancelOperation).toHaveBeenCalledWith("operation-current");
  });

  it("limits the Tauri drag region to the safe topbar area outside window buttons", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    const { container } = renderApp(api);
    await screen.findByRole("button", { name: "Играть" });

    const dragRegion = container.querySelector("[data-tauri-drag-region]");
    expect(dragRegion?.classList.contains("topbar-drag-region")).toBe(true);
    for (const button of screen.getAllByRole("button", { name: /Свернуть|Развернуть|Закрыть/ })) {
      expect(button.hasAttribute("data-tauri-drag-region")).toBe(false);
      expect(button.closest("[data-tauri-drag-region]")).toBeNull();
    }
  });

  it("shows runtime readiness for the Java major required by the selected version", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);

    const version = await screen.findByRole("combobox", { name: "Версия Minecraft" });
    expect(await screen.findByText("Java 8 не найдена")).toBeTruthy();
    fireEvent.change(version, { target: { value: "1.21.8" } });

    expect(await screen.findByText("Java 21 готова")).toBeTruthy();
    expect(api.requiredJavaForVersion).toHaveBeenCalledWith("1.21.8");
  });

  it("ignores stale events, disables play while busy, and separates recoverable from fatal errors", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);

    await screen.findByRole("button", { name: "Играть" });
    fireEvent.click(screen.getByRole("button", { name: "Играть" }));
    await waitFor(() => expect(api.launchOrInstall).toHaveBeenCalledTimes(1));

    act(() => {
      handlers.progress?.({
        operationId: "operation-stale",
        stage: "downloading",
        completedBytes: 90,
        totalBytes: 100,
        currentFile: "stale.jar",
      });
    });
    expect(screen.queryByText("stale.jar")).toBeNull();

    act(() => {
      handlers.progress?.({
        operationId: "operation-current",
        stage: "downloading",
        completedBytes: 10,
        totalBytes: 100,
        currentFile: "current.jar",
      });
    });
    expect((screen.getByRole("button", { name: "Играть" }) as HTMLButtonElement).disabled).toBe(true);

    act(() => {
      handlers.error?.({
        operationId: "operation-current",
        profileId: "default",
        terminal: true,
        error: {
          code: "download_timeout",
          message: "Соединение прервано.",
          recoverable: true,
        },
      });
    });
    expect(screen.getByRole("button", { name: "Повторить" })).toBeTruthy();

    act(() => {
      handlers.error?.({
        operationId: "operation-current",
        profileId: "default",
        terminal: true,
        error: {
          code: "invalid_path",
          message: "Запуск остановлен.",
          details: "C:\\Users\\secret\\token=never-show",
          recoverable: false,
        },
      });
    });
    const logButton = screen.getByRole("button", { name: "Открыть очищенный журнал" });
    expect((logButton as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByText(/never-show/)).toBeNull();
  });

  it("keeps a nonzero game exit in retryable error state and opens only the backend-owned log", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Играть" }));
    await waitFor(() => expect(api.launchOrInstall).toHaveBeenCalledTimes(1));
    act(() => {
      handlers.started?.({ operationId: "operation-current", profileId: "default", pid: 42 });
      handlers.error?.({
        operationId: "operation-current",
        profileId: "default",
        terminal: true,
        logPath: "C:\\safe\\logs\\latest.log",
        error: {
          code: "game_exit",
          message: "Minecraft завершился с кодом 7.",
          recoverable: true,
        },
      });
    });

    expect(screen.getByText("Можно повторить операцию")).toBeTruthy();
    expect(screen.getByText("C:\\safe\\logs\\latest.log")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Открыть очищенный журнал" }));
    expect(api.openLatestGameLog).toHaveBeenCalledWith();
  });

  it("accepts a matching game-started event emitted before the launch command returns", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    let resolveLaunch: ((operationId: string) => void) | undefined;
    api.launchOrInstall.mockImplementation(() => new Promise((resolve) => {
      resolveLaunch = resolve;
    }));
    renderApp(api);

    fireEvent.click(await screen.findByRole("button", { name: "Играть" }));
    await waitFor(() => expect(api.launchOrInstall).toHaveBeenCalledTimes(1));
    act(() => {
      handlers.started?.({ operationId: "operation-current", profileId: "default", pid: 42 });
    });
    await act(async () => {
      resolveLaunch?.("operation-current");
      await Promise.resolve();
    });

    expect(screen.getByText("Minecraft запущен")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Игра запущена" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("keeps running and shows no Retry for a nonterminal process warning", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Играть" }));
    await waitFor(() => expect(api.launchOrInstall).toHaveBeenCalledTimes(1));
    act(() => {
      handlers.started?.({ operationId: "operation-current", profileId: "default", pid: 42 });
    });

    act(() => {
      handlers.error?.({
        operationId: "operation-current",
        profileId: "default",
        terminal: false,
        error: { code: "log_warning", message: "Журнал неполон.", recoverable: true },
      });
    });
    expect(screen.getByText("Minecraft запущен")).toBeTruthy();
    expect(screen.getByText("Журнал неполон.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Повторить" })).toBeNull();

    act(() => {
      handlers.exited?.({ operationId: "operation-current", profileId: "default", exitCode: 0 });
    });
    expect(screen.getByText("Готово к запуску")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Повторить" })).toBeNull();
    expect(screen.queryByText("Журнал неполон.")).toBeNull();
  });

  it("shows cancellation in progress on the active progress panel", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    let finishCancel = () => undefined;
    api.cancelOperation.mockImplementation(() => new Promise<undefined>((resolve) => {
      finishCancel = () => {
        resolve(undefined);
        return undefined;
      };
    }));
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Играть" }));
    await waitFor(() => expect(api.launchOrInstall).toHaveBeenCalledTimes(1));
    act(() => {
      handlers.progress?.({
        operationId: "operation-current",
        stage: "downloading",
        completedBytes: 1,
        totalBytes: 2,
        currentFile: "client.jar",
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "Отменить" }));
    const cancellingButton = screen.getByRole("button", { name: "Отменяем…" });
    expect((cancellingButton as HTMLButtonElement).disabled).toBe(true);
    expect(api.cancelOperation).toHaveBeenCalledWith("operation-current");
    await act(async () => finishCancel());
  });

  it("closes the scoped account popup after navigation and after switching", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);

    const accountButton = await screen.findByRole("button", { name: /Kvander.*Minecraft account/ });
    fireEvent.click(accountButton);
    expect(screen.getByRole("menu", { name: "Аккаунты Minecraft" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Настройки" }));
    expect(screen.queryByRole("menu", { name: "Аккаунты Minecraft" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Kvander.*Minecraft account/ }));
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitemradio", { name: /ComfortPlayer/ }));
    });
    expect(api.setActiveAccount).toHaveBeenCalledWith("comfort");
    expect(screen.queryByRole("menu", { name: "Аккаунты Minecraft" })).toBeNull();
  });

  it("changes the persisted game directory only through the backend picker", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    api.chooseGameDirectory.mockResolvedValue({
      id: "default",
      name: "Основной профиль",
      versionId: "1.20.1",
      memoryMb: 4096,
      gameDir: "C:\\selected\\minecraft",
      javaOverride: null,
    });
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Настройки" }));

    expect(screen.getByText("C:\\safe\\game")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Выбрать папку игры" }));

    expect(await screen.findByText("C:\\selected\\minecraft")).toBeTruthy();
    expect(api.chooseGameDirectory).toHaveBeenCalledWith();
  });

  it("saves only memory without reverting a version changed while the save is in flight", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Настройки" }));
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });
    expect(slider.getAttribute("max")).toBe("12288");

    let resolveMemory: ((profile: Awaited<ReturnType<typeof api.updateMemory>>) => void) | undefined;
    api.updateMemory.mockImplementation(() => new Promise((resolve) => { resolveMemory = resolve; }));
    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "5120" } });
    expect(api.updateMemory).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(249));
    expect(api.updateMemory).not.toHaveBeenCalled();
    await act(async () => {
      vi.advanceTimersByTime(1);
      await Promise.resolve();
    });
    expect(api.updateMemory).toHaveBeenCalledWith(5120);

    vi.useRealTimers();
    fireEvent.click(screen.getByRole("button", { name: "Главная" }));
    const version = await screen.findByRole("combobox", { name: "Версия Minecraft" });
    fireEvent.change(version, { target: { value: "1.21.8" } });
    await act(async () => {
      resolveMemory?.({
        id: "default",
        name: "Основной профиль",
        versionId: "1.20.1",
        memoryMb: 5120,
        gameDir: "C:\\safe\\game",
        javaOverride: null,
      });
      await Promise.resolve();
    });

    expect((version as HTMLSelectElement).value).toBe("1.21.8");
    expect(screen.getByText("5120 МБ")).toBeTruthy();
  });

  it("persists a debounced memory change after immediate page navigation", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Настройки" }));
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });

    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "6144" } });
    fireEvent.click(screen.getByRole("button", { name: "Главная" }));
    expect(screen.queryByRole("slider", { name: "Оперативная память" })).toBeNull();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    expect(api.updateMemory).toHaveBeenCalledWith(6144);
    expect(screen.getByText("6144 МБ")).toBeTruthy();
  });

  it("reverts memory and exposes a retryable status when persistence fails", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    api.updateMemory.mockRejectedValueOnce(new Error("database unavailable"));
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Настройки" }));
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });

    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "6144" } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    expect((slider as HTMLInputElement).value).toBe("4096");
    expect(screen.getByRole("alert").textContent).toContain("Значение восстановлено");
  });

  it("serializes saves and reverts a failed latest value to the preceding confirmation", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    type SavedProfile = Awaited<ReturnType<typeof api.updateMemory>>;
    let resolveFirst: ((profile: SavedProfile) => void) | undefined;
    let rejectSecond: ((reason: unknown) => void) | undefined;
    api.updateMemory
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectSecond = reject; }));
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Настройки" }));
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });

    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "5120" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(250); });
    fireEvent.change(slider, { target: { value: "6144" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(250); });
    expect(api.updateMemory).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Сохраняем…")).toBeTruthy();

    await act(async () => {
      resolveFirst?.({
        id: "default",
        name: "Основной профиль",
        versionId: "1.20.1",
        memoryMb: 5120,
        gameDir: "C:\\safe\\game",
        javaOverride: null,
      });
      await Promise.resolve();
    });
    expect(api.updateMemory).toHaveBeenNthCalledWith(2, 6144);

    await act(async () => {
      rejectSecond?.(new Error("database unavailable"));
      await Promise.resolve();
    });
    expect((slider as HTMLInputElement).value).toBe("5120");
    expect(screen.getByRole("alert").textContent).toContain("Значение восстановлено");
  });

  it("coalesces rapid queued changes across navigation to the latest desired memory", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    type SavedProfile = Awaited<ReturnType<typeof api.updateMemory>>;
    let resolveFirst: ((profile: SavedProfile) => void) | undefined;
    let resolveLatest: ((profile: SavedProfile) => void) | undefined;
    api.updateMemory
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockImplementationOnce(() => new Promise((resolve) => { resolveLatest = resolve; }));
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Настройки" }));
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });

    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "5120" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(250); });
    fireEvent.change(slider, { target: { value: "6144" } });
    fireEvent.change(slider, { target: { value: "7168" } });
    fireEvent.click(screen.getByRole("button", { name: "Главная" }));
    expect(screen.queryByRole("slider", { name: "Оперативная память" })).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(250); });
    expect(api.updateMemory).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveFirst?.({
        id: "default",
        name: "Основной профиль",
        versionId: "1.20.1",
        memoryMb: 5120,
        gameDir: "C:\\safe\\game",
        javaOverride: null,
      });
      await Promise.resolve();
    });
    expect(api.updateMemory).toHaveBeenNthCalledWith(2, 7168);

    await act(async () => {
      resolveLatest?.({
        id: "default",
        name: "Основной профиль",
        versionId: "1.20.1",
        memoryMb: 7168,
        gameDir: "C:\\safe\\game",
        javaOverride: null,
      });
      await Promise.resolve();
    });
    expect(api.updateMemory.mock.calls.map(([memoryMb]) => memoryMb)).toEqual([5120, 7168]);
    expect(screen.getByText("7168 МБ")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("persists memory after the StrictMode effect replay used by the real entrypoint", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    const AppWithApi = RootApp as unknown as (props: { api: typeof api }) => ReactElement;
    render(<StrictMode><AppWithApi api={api} /></StrictMode>);
    fireEvent.click(await screen.findByRole("button", { name: "Настройки" }));
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });

    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "5120" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(250); });

    expect(api.updateMemory).toHaveBeenCalledWith(5120);
  });

  it("waits for the latest queued memory confirmation before saving and launching", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    type SavedProfile = Awaited<ReturnType<typeof api.updateMemory>>;
    let resolveFirst: ((profile: SavedProfile) => void) | undefined;
    let resolveLatest: ((profile: SavedProfile) => void) | undefined;
    api.updateMemory
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockImplementationOnce(() => new Promise((resolve) => { resolveLatest = resolve; }));
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Настройки" }));
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });

    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "5120" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(250); });
    fireEvent.change(slider, { target: { value: "6144" } });
    fireEvent.click(screen.getByRole("button", { name: "Главная" }));
    fireEvent.click(screen.getByRole("button", { name: "Играть" }));

    expect(api.updateProfile).not.toHaveBeenCalled();
    expect(api.launchOrInstall).not.toHaveBeenCalled();
    await act(async () => {
      resolveFirst?.({
        id: "default",
        name: "Основной профиль",
        versionId: "1.20.1",
        memoryMb: 5120,
        gameDir: "C:\\safe\\game",
        javaOverride: null,
      });
      await Promise.resolve();
    });

    expect(api.updateMemory).toHaveBeenNthCalledWith(2, 6144);
    expect(api.updateProfile).not.toHaveBeenCalled();
    expect(api.launchOrInstall).not.toHaveBeenCalled();
    await act(async () => {
      resolveLatest?.({
        id: "default",
        name: "Основной профиль",
        versionId: "1.20.1",
        memoryMb: 6144,
        gameDir: "C:\\safe\\game",
        javaOverride: null,
      });
      await Promise.resolve();
    });

    vi.useRealTimers();
    await waitFor(() => expect(api.launchOrInstall).toHaveBeenCalledWith("default"));
    expect(api.updateProfile).toHaveBeenCalledWith(expect.objectContaining({ memoryMb: 6144 }));
    expect(api.updateProfile.mock.invocationCallOrder[0]).toBeLessThan(api.launchOrInstall.mock.invocationCallOrder[0]);
  });

  it("does not launch and shows a recoverable error when the Play memory flush fails", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    let rejectMemory: ((reason: unknown) => void) | undefined;
    api.updateMemory.mockImplementationOnce(() => new Promise((_resolve, reject) => {
      rejectMemory = reject;
    }));
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Настройки" }));
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });

    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "6144" } });
    fireEvent.click(screen.getByRole("button", { name: "Главная" }));
    fireEvent.click(screen.getByRole("button", { name: "Играть" }));

    expect(api.updateMemory).toHaveBeenCalledWith(6144);
    expect(api.updateProfile).not.toHaveBeenCalled();
    await act(async () => {
      rejectMemory?.({
        code: "profile_not_found",
        message: "The launcher profile was not found.",
        recoverable: false,
      });
      await Promise.resolve();
    });

    expect(api.updateProfile).not.toHaveBeenCalled();
    expect(api.launchOrInstall).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Повторить" })).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("Не удалось сохранить память перед запуском");
  });
});
