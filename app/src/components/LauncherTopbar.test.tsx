import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AppApi } from "../app/tauri";
import { LauncherTopbar } from "./LauncherTopbar";

describe("LauncherTopbar", () => {
  it("opens the task manager and exposes real navigation actions", () => {
    const onGoBack = vi.fn();
    const onGoForward = vi.fn();
    render(<LauncherTopbar
      activeBuild={{ id: "build-1", name: "Fabric 1.21", gameVersion: "1.21", loader: "fabric", gameDir: "C:/game", isActive: true }}
      api={{} as AppApi}
      canGoBack
      canGoForward={false}
      cancelling={false}
      completedTasks={[{ id: "task-1", title: "Sodium", detail: "Мод установлен", completedAt: Date.now() }]}
      onClearTasks={vi.fn()}
      onCancelProgress={vi.fn()}
      onGoBack={onGoBack}
      onGoForward={onGoForward}
      progress={{ operationId: "operation-1", stage: "downloading", completedBytes: 50, totalBytes: 100, currentFile: "minecraft.jar" }}
    />);

    fireEvent.click(screen.getByRole("button", { name: "Назад" }));
    expect(onGoBack).toHaveBeenCalledOnce();
    expect((screen.getByRole("button", { name: "Вперёд" }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Менеджер загрузок" }));
    expect(screen.getByRole("region", { name: "Менеджер загрузок" }).textContent).toContain("minecraft.jar");
    expect(screen.getByRole("region", { name: "Менеджер загрузок" }).textContent).toContain("Sodium");
    expect(screen.getByRole("button", { name: "Отменить текущую задачу" })).toBeTruthy();

    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("region", { name: "Менеджер загрузок" })).toBeNull();
  });
});
