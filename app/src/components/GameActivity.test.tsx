import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { GameActivity } from "./GameActivity";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("GameActivity", () => {
  it("opens the live console and stops the matching Minecraft process", async () => {
    vi.useFakeTimers();
    const api = {
      readLatestGameLog: vi.fn(async () => "[main/INFO] Minecraft started"),
      stopGame: vi.fn(async () => undefined),
    };

    render(
      <GameActivity
        api={api}
        game={{ operationId: "operation-1", profileId: "profile-1", pid: 4321 }}
        name="Fabulously Optimized"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Открыть консоль Minecraft" }));
    await act(async () => { await Promise.resolve(); });
    expect(screen.getByRole("dialog", { name: "Консоль Minecraft" }).textContent).toContain("Minecraft started");

    fireEvent.click(screen.getByRole("button", { name: "Остановить игру" }));
    await act(async () => { await Promise.resolve(); });
    expect(api.stopGame).toHaveBeenCalledWith("operation-1");

    fireEvent.mouseDown(screen.getByRole("button", { name: "Закрыть консоль" }));
    fireEvent.click(screen.getByRole("button", { name: "Закрыть консоль" }));
    expect(screen.queryByRole("dialog", { name: "Консоль Minecraft" })).toBeNull();
  });
});
