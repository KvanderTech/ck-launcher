import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import RootApp from "../App";
import type {
  AccountSummary,
  JavaRuntimeStatus,
  LauncherErrorDto,
  ProgressEvent,
} from "./types";

type EventHandlers = {
  progress?: (event: ProgressEvent) => void;
  started?: (event: { operationId: string; profileId: string; pid: number }) => void;
  exited?: (event: { operationId: string; profileId: string; exitCode: number }) => void;
  error?: (event: {
    operationId: string;
    profileId: string;
    error: LauncherErrorDto;
  }) => void;
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
    removeAccount: vi.fn(async () => undefined),
    setActiveAccount: vi.fn(async () => undefined),
    listGameVersions: vi.fn(async () => [
      { id: "1.21.8", type: "release", releaseDate: "2026-07-17T00:00:00Z" },
      { id: "1.20.1", type: "release", releaseDate: "2023-06-12T00:00:00Z" },
    ]),
    getProfile: vi.fn(async () => ({
      id: "default",
      name: "Основной профиль",
      versionId: "1.20.1",
      memoryMb: 4096,
      gameDir: "C:\\safe\\game",
      javaOverride: null,
    })),
    updateProfile: vi.fn(async (profile) => profile),
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

  it("uses the backend memory maximum and persists profile memory after a 250 ms debounce", async () => {
    const handlers: EventHandlers = {};
    const api = createApi(handlers);
    renderApp(api);
    fireEvent.click(await screen.findByRole("button", { name: "Настройки" }));
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });
    expect(slider.getAttribute("max")).toBe("12288");

    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "5120" } });
    expect(api.updateProfile).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(249));
    expect(api.updateProfile).not.toHaveBeenCalled();
    await act(async () => {
      vi.advanceTimersByTime(1);
      await Promise.resolve();
    });
    expect(api.updateProfile).toHaveBeenCalledWith(
      expect.objectContaining({ id: "default", memoryMb: 5120 }),
    );
  });
});
