import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AppApi } from "../../app/tauri";
import { LibraryPage } from "./LibraryPage";

afterEach(cleanup);

describe("LibraryPage catalog navigation", () => {
  it("passes the exact build whose catalog button was pressed without changing the active build", async () => {
    const build = { id: "target", name: "My Fabric", gameVersion: "1.21.1", loader: "fabric", gameDir: "C:/target", isActive: false };
    const selectBuild = vi.fn();
    const onOpenCatalog = vi.fn();
    const api = {
      listBuilds: vi.fn(async () => [build]),
      buildPreferences: vi.fn(async () => ({ groupName: "", javaOverride: null, accountId: null })),
      listInstalledContent: vi.fn(async () => []),
      selectBuild,
    } as unknown as AppApi;
    render(<LibraryPage api={api} initialBuildId="target" onBuildSelected={vi.fn()} onOpenCatalog={onOpenCatalog} onCreateBuild={vi.fn()} onPlay={vi.fn()} onRequestDelete={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Каталог" }));
    await waitFor(() => expect(onOpenCatalog).toHaveBeenCalledWith(build));
    expect(selectBuild).not.toHaveBeenCalled();
  });
});
