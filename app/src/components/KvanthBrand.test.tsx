import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LauncherApi } from "../app/tauri";
import { Sidebar } from "./Sidebar";

vi.mock("../app/edition", () => ({ isWindows11Edition: true }));
vi.mock("../features/accounts/AccountMenu", () => ({ AccountMenu: () => null }));

describe("Kvanth Windows 11 branding", () => {
  it("uses the approved standalone K in the Windows 11 sidebar", () => {
    render(<Sidebar activePage="home" accounts={[]} builds={[]} accountApi={{} as LauncherApi}
      onAccountAdded={vi.fn()} onAccountRemoved={vi.fn()} onActiveAccountChange={vi.fn()}
      onNavigate={vi.fn()} onOpenBuild={vi.fn()} />);
    const brand = screen.getByRole("img", { name: "Kvanth Launcher" }) as HTMLImageElement;
    expect(brand.src).toContain("kvanth-icon.png");
    expect(screen.queryByAltText("Логотип ЦК")).toBeNull();
    expect(screen.queryByText("ЦК Лаунчер")).toBeNull();
    for (const label of ["Главная", "Библиотека", "Каталог", "Скины и плащи", "Настройки", "Добавить сборку"]) {
      const icon = screen.getByRole('button', { name: label }).querySelector('img');
      expect(icon?.getAttribute('data-tone')).toBe('blue');
      expect(icon?.getAttribute('src')).toContain('/blue/');
    }
  });
});
