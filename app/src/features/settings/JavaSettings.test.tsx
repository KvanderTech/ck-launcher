import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { JavaRuntimeStatus } from "../../app/types";
import { JavaSettings } from "./JavaSettings";

const statuses: JavaRuntimeStatus[] = [
  { requirement: 8, state: "valid", path: "C:\\java8.exe", source: "managed", version: "1.8.0_431" },
  { requirement: 17, state: "missing" },
  { requirement: 21, state: "installing" },
  { requirement: 25, state: "invalid", path: "C:\\wrong.exe" },
];

describe("JavaSettings", () => {
  it("renders all supported major states and disables only the active major", () => {
    const onInstall = vi.fn();
    const onDetect = vi.fn();
    const onChoose = vi.fn();
    render(
      <JavaSettings
        statuses={statuses}
        onInstall={onInstall}
        onDetect={onDetect}
        onChoose={onChoose}
      />,
    );

    for (const major of [8, 17, 21, 25]) {
      expect(screen.getByRole("heading", { name: `Java ${major}` })).toBeTruthy();
    }
    expect(screen.getByText("Готова")).toBeTruthy();
    expect(screen.getByText("Не найдена")).toBeTruthy();
    expect(screen.getByText("Устанавливается…")).toBeTruthy();
    expect(screen.getByText("Не подходит")).toBeTruthy();

    expect((screen.getByRole("button", { name: "Установить Java 21" }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "Установить Java 17" }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Найти Java 17" }));
    expect(onDetect).toHaveBeenCalledWith(17);
  });
});
