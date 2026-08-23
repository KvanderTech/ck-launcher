import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SettingsApi } from "../../app/tauri";
import { MemorySettings } from "./MemorySettings";

describe("MemorySettings", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("loads backend limits and emits the selected memory to the stable owner", async () => {
    const onChange = vi.fn();
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
    render(<MemorySettings api={api} memoryMb={2048} onChange={onChange} saveState="idle" />);

    const slider = await screen.findByRole("slider", { name: "Оперативная память" });
    expect(slider.getAttribute("step")).toBe("512");
    expect(slider.getAttribute("max")).toBe("12288");
    expect(screen.getByText("2048 МБ")).toBeTruthy();
    fireEvent.change(slider, { target: { value: "3584" } });
    expect(api.updateMemory).not.toHaveBeenCalled();
    expect(onChange).toHaveBeenCalledWith(3584);
  });

  it("shows the owner-provided reverted value and failed save state", async () => {
    const api: SettingsApi = {
      memoryStatus: vi.fn(async () => ({
        memoryMb: 4096,
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
    render(<MemorySettings api={api} memoryMb={4096} onChange={vi.fn()} saveState="error" />);
    const slider = await screen.findByRole("slider", { name: "Оперативная память" });

    expect(screen.getByRole("alert").textContent).toContain("Значение восстановлено");
    expect((slider as HTMLInputElement).value).toBe("4096");
  });
});
