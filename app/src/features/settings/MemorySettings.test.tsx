import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SettingsApi } from "../../app/tauri";
import { MemorySettings } from "./MemorySettings";

describe("MemorySettings", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("loads backend limits and saves only memory after a 250 ms debounce", async () => {
    const onSaved = vi.fn();
    const api: SettingsApi = {
      memoryStatus: vi.fn(async () => ({
        memoryMb: 2048,
        minMemoryMb: 512,
        maxMemoryMb: 12288,
        stepMemoryMb: 512,
      })),
      updateMemory: vi.fn(async (memoryMb) => ({
        id: "default",
        name: "Default",
        versionId: "1.21.8",
        memoryMb,
        gameDir: "game",
        javaOverride: null,
      })),
    };
    render(<MemorySettings api={api} onSaved={onSaved} />);

    const slider = await screen.findByRole("slider", { name: "Оперативная память" });
    expect(slider.getAttribute("step")).toBe("512");
    expect(slider.getAttribute("max")).toBe("12288");
    expect(screen.getByText("2048 МБ")).toBeTruthy();
    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "3584" } });
    expect(api.updateMemory).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(api.updateMemory).toHaveBeenCalledWith(3584);
    expect(onSaved).toHaveBeenCalledWith(3584);
  });

  it("reverts to the last saved value and reports a failed memory save", async () => {
    const api: SettingsApi = {
      memoryStatus: vi.fn(async () => ({
        memoryMb: 4096,
        minMemoryMb: 512,
        maxMemoryMb: 12288,
        stepMemoryMb: 512,
      })),
      updateMemory: vi.fn(async () => { throw new Error("database unavailable"); }),
    };
    render(<MemorySettings api={api} onSaved={vi.fn()} />);
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });

    vi.useFakeTimers();
    fireEvent.change(slider, { target: { value: "5120" } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    vi.useRealTimers();
    expect((await screen.findByRole("alert")).textContent).toContain("Значение восстановлено");
    expect((slider as HTMLInputElement).value).toBe("4096");
  });
});
