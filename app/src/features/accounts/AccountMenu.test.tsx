import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AccountSummary } from "../../app/types";
import type { LauncherApi } from "../../app/tauri";
import { AccountMenu } from "./AccountMenu";
import { MicrosoftLogin } from "./MicrosoftLogin";

const accounts: AccountSummary[] = [
  {
    id: "one",
    minecraftName: "Alex",
    minecraftUuid: "uuid-one",
    headUrl: "https://example.test/alex.png",
    isActive: true,
  },
  {
    id: "two",
    minecraftName: "Steve",
    minecraftUuid: "uuid-two",
    headUrl: "https://example.test/steve.png",
    isActive: false,
  },
];

afterEach(cleanup);

function mockApi(overrides: Partial<LauncherApi> = {}): LauncherApi {
  return {
    listAccounts: vi.fn(async () => accounts),
    beginMicrosoftLogin: vi.fn(async () => accounts[0]),
    removeAccount: vi.fn(async () => undefined),
    setActiveAccount: vi.fn(async () => undefined),
    ...overrides,
  };
}

describe("AccountMenu", () => {
  it("switches to the clicked account and updates the lower account panel", async () => {
    const api = mockApi();
    render(<AccountMenu accounts={accounts} api={api} />);

    fireEvent.click(screen.getByRole("button", { name: /Alex.*Minecraft account/ }));

    await act(async () => {
      fireEvent.click(screen.getByRole("menuitemradio", { name: /Steve/ }));
    });

    expect(api.setActiveAccount).toHaveBeenCalledWith("two");
    expect(screen.getByTestId("active-account-panel").textContent).toContain("Steve");
    expect(screen.queryByRole("menu", { name: "Аккаунты Minecraft" })).toBeNull();
  });

  it("keeps the add-account action open and retryable after a safe login error", async () => {
    let rejectLogin: ((reason: unknown) => void) | undefined;
    const pending = new Promise<AccountSummary>((_resolve, reject) => { rejectLogin = reject; });
    const api = mockApi({ beginMicrosoftLogin: vi.fn(() => pending) });
    render(<AccountMenu accounts={accounts} api={api} />);
    fireEvent.click(screen.getByRole("button", { name: /Alex.*Minecraft account/ }));

    fireEvent.click(screen.getByRole("menuitem", { name: "Добавить аккаунт" }));
    expect(screen.getByRole("menuitem", { name: "Входим…" })).toBeTruthy();
    await act(async () => {
      rejectLogin?.({ code: "auth_network_error", message: "Не удалось войти.", recoverable: true });
      await pending.catch(() => undefined);
    });

    expect(screen.getByRole("alert").textContent).toBe("Не удалось войти.");
    expect(screen.getByRole("menuitem", { name: "Повторить вход" })).toBeTruthy();
    expect(screen.getByRole("menu", { name: "Аккаунты Minecraft" })).toBeTruthy();
  });
});

describe("MicrosoftLogin", () => {
  it("shows idle, loading, and safe error states without token props", async () => {
    let rejectLogin: ((reason: unknown) => void) | undefined;
    const pending = new Promise<AccountSummary>((_resolve, reject) => {
      rejectLogin = reject;
    });
    const api = mockApi({ beginMicrosoftLogin: vi.fn(() => pending) });
    render(<MicrosoftLogin api={api} />);

    const idleButton = screen.getByRole("button", { name: "Войти через Microsoft" });
    expect((idleButton as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(idleButton);
    expect(screen.getByRole("button", { name: "Входим…" })).toBeTruthy();

    await act(async () => {
      rejectLogin?.({
        code: "auth_network_error",
        message: "Не удалось войти.",
        recoverable: true,
      });
      await pending.catch(() => undefined);
    });

    expect(screen.getByRole("alert").textContent).toBe("Не удалось войти.");
    expect(screen.getByRole("button", { name: "Повторить вход" })).toBeTruthy();
  });
});
