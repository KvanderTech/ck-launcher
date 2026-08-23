import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { SettingsApi } from "../../app/tauri";
import { MemorySettings } from "./MemorySettings";

describe("MemorySettings", () => {
  it("loads the backend-clamped maximum, uses its 512 MB step, and emits the exact value", async () => {
    const onChange = vi.fn();
    const api: SettingsApi = {
      memoryStatus: vi.fn(async () => ({
        memoryMb: 2048,
        minMemoryMb: 512,
        maxMemoryMb: 12288,
        stepMemoryMb: 512,
      })),
    };
    render(<MemorySettings api={api} onChange={onChange} />);

    const slider = await screen.findByRole("slider", { name: "Оперативная память" });
    expect(slider.getAttribute("step")).toBe("512");
    expect(slider.getAttribute("max")).toBe("12288");
    expect(screen.getByText("2048 МБ")).toBeTruthy();
    fireEvent.change(slider, { target: { value: "3584" } });
    expect(onChange).toHaveBeenCalledWith(3584);
  });
});
