import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { WindowApi } from "../app/tauri";
import { WindowControls } from "./WindowControls";

describe("WindowControls", () => {
  it("routes minimize, maximize, and close through the injected window boundary", () => {
    const api: WindowApi = {
      minimize: vi.fn(async () => undefined),
      toggleMaximize: vi.fn(async () => undefined),
      close: vi.fn(async () => undefined),
    };
    render(<WindowControls api={api} />);

    fireEvent.click(screen.getByRole("button", { name: "Свернуть" }));
    fireEvent.click(screen.getByRole("button", { name: "Развернуть" }));
    fireEvent.click(screen.getByRole("button", { name: "Закрыть" }));

    expect(api.minimize).toHaveBeenCalledOnce();
    expect(api.toggleMaximize).toHaveBeenCalledOnce();
    expect(api.close).toHaveBeenCalledOnce();
  });
});
