import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { MemorySettings } from "./MemorySettings";

describe("MemorySettings", () => {
  it("uses 512 MB steps, shows the exact value, and emits the backend-clamped result", async () => {
    const onChange = vi.fn();
    render(
      <MemorySettings
        memoryMb={2048}
        maxMemoryMb={4096}
        onChange={onChange}
      />,
    );

    const slider = screen.getByRole("slider", { name: "Оперативная память" });
    expect(slider.getAttribute("step")).toBe("512");
    expect(slider.getAttribute("max")).toBe("4096");
    expect(screen.getByText("2048 МБ")).toBeTruthy();
    fireEvent.change(slider, { target: { value: "3584" } });
    expect(onChange).toHaveBeenCalledWith(3584);
  });
});
