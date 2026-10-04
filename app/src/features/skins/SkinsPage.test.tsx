import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { SkinViewer } from "skinview3d";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AppApi } from "../../app/tauri";
import { SkinsPage } from "./SkinsPage";

vi.mock("skinview3d", async (importOriginal) => {
  const actual = await importOriginal<typeof import("skinview3d")>();
  return {
    ...actual,
    SkinViewer: vi.fn().mockImplementation(() => ({
      controls: {},
      dispose: vi.fn(),
      loadCape: vi.fn(),
      playerWrapper: { position: { y: 0 }, rotation: { y: 0 }, scale: { setScalar: vi.fn() } },
    })),
  };
});

afterEach(() => { cleanup(); vi.mocked(SkinViewer).mockClear(); });

describe("SkinsPage preview", () => {
  it("opens on the active Minecraft skin, not the first saved skin", () => {
    render(<SkinsPage
      account={{ id: "account", minecraftName: "Kvander", minecraftUuid: "uuid", isActive: true }}
      api={{} as AppApi}
      cosmetics={{ id: "profile", name: "Kvander", skins: [{ id: "current", state: "ACTIVE", url: "https://example.com/current.png", variant: "CLASSIC" }], capes: [] }}
      loading={false}
      onCosmeticsChange={vi.fn()}
      onRefresh={vi.fn()}
      onSkinsChange={vi.fn()}
      skins={[{ id: "saved", accountId: "account", name: "Сохранённый", dataUrl: "data:image/png;base64,AAAA", isActive: true, isFavorite: false }]}
    />);

    const previewSources = () => vi.mocked(SkinViewer).mock.calls.filter(([options]) => options?.width === 280).map(([options]) => options?.skin);
    expect(previewSources()[previewSources().length - 1]).toBe("https://example.com/current.png");

    fireEvent.click(screen.getByRole("button", { name: "Выбрать скин Сохранённый" }));
    expect(previewSources()[previewSources().length - 1]).toBe("data:image/png;base64,AAAA");
  });
});
