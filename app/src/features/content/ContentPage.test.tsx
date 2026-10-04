import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { AppApi } from "../../app/tauri";
import { ContentPage } from "./ContentPage";

afterEach(cleanup);

describe("ContentPage create build", () => {
  it("offers Forge and sends it to the native build creator", async () => {
    const createBuild = vi.fn(async () => ({ id: "new", name: "Моя сборка", gameVersion: "1.20.1", loader: "forge", gameDir: "C:/game", isActive: true }));
    const api = {
      listBuilds: vi.fn(async () => []),
      listLoaderVersions: vi.fn(async () => [{ id: "47.4.0", stable: true }]),
      searchModrinth: vi.fn(async () => ({ hits: [], offset: 0, limit: 20, total_hits: 0 })),
      createBuild,
    } as unknown as AppApi;
    render(<ContentPage api={api} versions={[{ id: "1.20.1", type: "release", releaseDate: "" }]} onBuildSelected={vi.fn()} onInstallTaskChange={vi.fn()} startCreating />);

    expect(screen.getByRole("dialog", { name: "Создать сборку" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Установить .mrpack" })).toBeNull();
    fireEvent.click(within(screen.getByRole("dialog", { name: "Создать сборку" })).getByRole("button", { name: "Forge" }));
    await waitFor(() => expect((screen.getByRole("button", { name: /^Создать$/ }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: /^Создать$/ }));

    await waitFor(() => expect(createBuild).toHaveBeenCalledWith("Моя сборка", "1.20.1", "forge", undefined, "47.4.0"));
  });

  it("previews and sends an uploaded icon", async () => {
    const createBuild = vi.fn(async () => ({ id: "new", name: "С иконкой", gameVersion: "1.20.1", loader: "fabric", gameDir: "C:/game", isActive: true }));
    const api = {
      listBuilds: vi.fn(async () => []),
      listLoaderVersions: vi.fn(async () => [{ id: "0.17.3", stable: true }]),
      searchModrinth: vi.fn(async () => ({ hits: [], offset: 0, limit: 20, total_hits: 0 })),
      createBuild,
    } as unknown as AppApi;
    render(<ContentPage api={api} versions={[{ id: "1.20.1", type: "release", releaseDate: "" }]} onBuildSelected={vi.fn()} onInstallTaskChange={vi.fn()} startCreating />);

    fireEvent.change(screen.getByRole("textbox", { name: "Название" }), { target: { value: "С иконкой" } });
    fireEvent.change(screen.getByLabelText("Файл иконки сборки"), { target: { files: [new File([new Uint8Array([137, 80, 78, 71])], "icon.png", { type: "image/png" })] } });
    await screen.findByRole("img", { name: "Иконка новой сборки" });
    await waitFor(() => expect((screen.getByRole("button", { name: /^Создать$/ }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(screen.getByRole("button", { name: /^Создать$/ }));

    await waitFor(() => expect(createBuild).toHaveBeenCalledWith("С иконкой", "1.20.1", "fabric", expect.stringContaining("data:image/png;base64,"), "0.17.3"));
  });

  it("can show snapshots and choose a specific compatible loader version", async () => {
    const createBuild = vi.fn(async () => ({ id: "new", name: "Моя сборка", gameVersion: "26w01a", loader: "fabric", gameDir: "C:/game", isActive: true }));
    const listLoaderVersions = vi.fn(async () => [{ id: "0.17.3", stable: true }, { id: "0.18.0-beta.1", stable: false }]);
    const api = { listBuilds: vi.fn(async () => []), listLoaderVersions, searchModrinth: vi.fn(async () => ({ hits: [], offset: 0, limit: 20, total_hits: 0 })), createBuild } as unknown as AppApi;
    render(<ContentPage api={api} versions={[{ id: "1.21.11", type: "release", releaseDate: "" }, { id: "26w01a", type: "snapshot", releaseDate: "" }]} onBuildSelected={vi.fn()} onInstallTaskChange={vi.fn()} startCreating />);
    fireEvent.click(screen.getByRole("button", { name: "Показать снапшоты и беты" }));
    fireEvent.click(screen.getByRole("button", { name: "Версия Minecraft для новой сборки" }));
    fireEvent.click(screen.getByRole("option", { name: /26w01a/ }));
    await waitFor(() => expect(listLoaderVersions).toHaveBeenCalledWith("26w01a", "fabric"));
    fireEvent.click(screen.getByRole("button", { name: "Другая" }));
    fireEvent.click(screen.getByRole("button", { name: "Конкретная версия загрузчика" }));
    fireEvent.click(screen.getByRole("option", { name: /0.18.0-beta.1/ }));
    fireEvent.click(screen.getByRole("button", { name: /^Создать$/ }));
    await waitFor(() => expect(createBuild).toHaveBeenCalledWith("Моя сборка", "26w01a", "fabric", undefined, "0.18.0-beta.1"));
  });
});

describe("ContentPage install destination", () => {
  it("shows all versions and loaders by default in the general catalog", async () => {
    const active = { id: "active", name: "Fabric 1.20.1", gameVersion: "1.20.1", loader: "fabric", gameDir: "C:/game", isActive: true };
    const searchModrinth = vi.fn(async () => ({ hits: [], offset: 0, limit: 20, total_hits: 0 }));
    const api = {
      listBuilds: vi.fn(async () => [active]),
      listInstalledContent: vi.fn(async () => []),
      searchModrinth,
    } as unknown as AppApi;
    render(<ContentPage api={api} versions={[{ id: "1.20.1", type: "release", releaseDate: "" }]} onBuildSelected={vi.fn()} onInstallTaskChange={vi.fn()} />);

    await waitFor(() => expect(searchModrinth).toHaveBeenCalledWith("", "modpack", undefined, undefined, 0, "", "", "relevance"));
    expect(screen.getByRole("button", { name: "Версия Minecraft каталога" }).textContent).toContain("Все версии");
    expect(screen.getByRole("button", { name: "Все" }).getAttribute("class")).toContain("active");
  });

  it("removes the top build selector and installs a mod only into a compatible chosen build", async () => {
    const fabric = { id: "fabric-build", name: "Fabric 1.20.1", gameVersion: "fabric-loader-0.16.0-1.20.1", loader: "fabric", loaderVersion: "0.16.0", gameDir: "C:/fabric", isActive: true };
    const forge = { id: "forge-build", name: "Forge 1.19.4", gameVersion: "forge-loader-45.0.0-1.19.4", loader: "forge", loaderVersion: "45.0.0", gameDir: "C:/forge", isActive: false };
    const project = { project_id: "test-mod", project_type: "mod", title: "Test Mod", description: "", author: "Author", categories: [], versions: [], downloads: 0, follows: 0, date_modified: "" };
    const installModrinthProject = vi.fn(async () => ({}));
    const api = {
      listBuilds: vi.fn(async () => [fabric, forge]),
      listInstalledContent: vi.fn(async () => []),
      searchModrinth: vi.fn(async () => ({ hits: [project], offset: 0, limit: 20, total_hits: 1 })),
      modrinthProjectVersions: vi.fn(async () => [{ id: "mod-version", name: "1.0", version_number: "1.0", version_type: "release", date_published: "2026-01-01", downloads: 1, loaders: ["fabric"], game_versions: ["1.20.1"] }]),
      installModrinthProject,
    } as unknown as AppApi;
    render(<ContentPage api={api} versions={[{ id: "1.20.1", type: "release", releaseDate: "" }]} onBuildSelected={vi.fn()} onInstallTaskChange={vi.fn()} forBuild />);
    expect(document.querySelector(".build-strip")).toBeNull();
    fireEvent.click(within(await screen.findByText("Test Mod").then((node) => node.closest("article")!)).getByRole("button", { name: "Установить" }));
    const dialog = await screen.findByRole("dialog", { name: "Установить «Test Mod»" });
    expect(within(dialog).getByText("Fabric 1.20.1")).toBeTruthy();
    expect(within(dialog).queryByText("Forge 1.19.4")).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: "Установить" }));
    await waitFor(() => expect(installModrinthProject).toHaveBeenCalledWith("test-mod", "fabric-build", "mod-version"));
  });

  it("locks a build-scoped catalog to that build and installs there without a destination dialog", async () => {
    const other = { id: "other", name: "Other Forge", gameVersion: "forge-loader-47.0.0-1.20.1", loader: "forge", loaderVersion: "47.0.0", gameDir: "C:/other", isActive: true };
    const target = { id: "target", name: "My Fabric", gameVersion: "fabric-loader-0.16.0-1.21.1", loader: "fabric", loaderVersion: "0.16.0", gameDir: "C:/target", isActive: false };
    const project = { project_id: "scoped-mod", project_type: "mod", title: "Scoped Mod", description: "", author: "Author", categories: [], versions: [], downloads: 0, follows: 0, date_modified: "" };
    const searchModrinth = vi.fn(async () => ({ hits: [project], offset: 0, limit: 20, total_hits: 1 }));
    const installModrinthProject = vi.fn(async () => ({}));
    const api = {
      listBuilds: vi.fn(async () => [other, target]),
      listInstalledContent: vi.fn(async () => []),
      searchModrinth,
      modrinthProjectVersions: vi.fn(async () => [
        { id: "forge-version", name: "Forge", version_number: "1", version_type: "release", date_published: "2026-02-01", downloads: 1, loaders: ["forge"], game_versions: ["1.20.1"] },
        { id: "fabric-version", name: "Fabric", version_number: "1", version_type: "release", date_published: "2026-01-01", downloads: 1, loaders: ["fabric"], game_versions: ["1.21.1"] },
      ]),
      installModrinthProject,
    } as unknown as AppApi;
    render(<ContentPage api={api} versions={[{ id: "1.21.1", type: "release", releaseDate: "" }]} forBuild targetBuildId="target" onBuildSelected={vi.fn()} onInstallTaskChange={vi.fn()} />);
    await waitFor(() => expect(searchModrinth).toHaveBeenCalledWith("", "mod", "1.21.1", "fabric", 0, "", "", "relevance"));
    expect(screen.getByRole("heading", { name: "My Fabric" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Версия Minecraft каталога" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Все" })).toBeNull();
    fireEvent.click(within(await screen.findByText("Scoped Mod").then((node) => node.closest("article")!)).getByRole("button", { name: "Установить" }));
    await waitFor(() => expect(installModrinthProject).toHaveBeenCalledWith("scoped-mod", "target", "fabric-version"));
    expect(screen.queryByRole("dialog", { name: /Установить «Scoped Mod»/ })).toBeNull();
  });

  it("uses the same scoped build for CurseForge downloads", async () => {
    const target = { id: "target", name: "My Fabric", gameVersion: "fabric-loader-0.16.0-1.21.1", loader: "fabric", loaderVersion: "0.16.0", gameDir: "C:/target", isActive: false };
    const project = { project_id: "curseforge:123", project_type: "mod", title: "CF Mod", description: "", author: "Author", categories: [], versions: [], downloads: 0, follows: 0, date_modified: "" };
    const curseForgeVersions = vi.fn(async () => [{ id: "456", name: "Release", version_number: "1", version_type: "release", date_published: "2026-01-01", downloads: 1, loaders: [], game_versions: ["1.21.1", "Fabric"] }]);
    const installCurseForgeProject = vi.fn(async () => ({}));
    const api = {
      listBuilds: vi.fn(async () => [target]),
      listInstalledContent: vi.fn(async () => []),
      searchModrinth: vi.fn(async () => ({ hits: [], offset: 0, limit: 20, total_hits: 0 })),
      searchCurseForge: vi.fn(async () => ({ hits: [project], offset: 0, limit: 20, total_hits: 1 })),
      curseForgeVersions,
      installCurseForgeProject,
    } as unknown as AppApi;
    render(<ContentPage api={api} versions={[{ id: "1.21.1", type: "release", releaseDate: "" }]} forBuild targetBuildId="target" onBuildSelected={vi.fn()} onInstallTaskChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "CurseForge" }));
    const card = (await screen.findByText("CF Mod")).closest("article")!;
    fireEvent.click(within(card).getByRole("button", { name: "Установить" }));
    await waitFor(() => expect(curseForgeVersions).toHaveBeenCalledWith(123, "1.21.1", "fabric"));
    await waitFor(() => expect(installCurseForgeProject).toHaveBeenCalledWith(123, "target", "mod", 456));
    expect(document.querySelector(".project-detail-page")).toBeNull();
  });

  it("opens build selection directly from a CurseForge card in the general catalog", async () => {
    const build = { id: "fabric", name: "Fabric Build", gameVersion: "fabric-loader-0.16.0-1.21.1", loader: "fabric", loaderVersion: "0.16.0", gameDir: "C:/fabric", isActive: true };
    const project = { project_id: "curseforge:123", project_type: "mod", title: "CF Mod", description: "", author: "Author", categories: [], versions: [], downloads: 0, follows: 0, date_modified: "" };
    const api = {
      listBuilds: vi.fn(async () => [build]),
      listInstalledContent: vi.fn(async () => []),
      searchModrinth: vi.fn(async () => ({ hits: [], offset: 0, limit: 20, total_hits: 0 })),
      searchCurseForge: vi.fn(async () => ({ hits: [project], offset: 0, limit: 20, total_hits: 1 })),
      curseForgeVersions: vi.fn(async () => [{ id: "456", name: "Release", version_number: "1", version_type: "release", date_published: "2026-01-01", downloads: 1, loaders: [], game_versions: ["1.21.1", "Fabric"] }]),
    } as unknown as AppApi;
    render(<ContentPage api={api} versions={[{ id: "1.21.1", type: "release", releaseDate: "" }]} forBuild onBuildSelected={vi.fn()} onInstallTaskChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "CurseForge" }));
    const card = (await screen.findByText("CF Mod")).closest("article")!;
    fireEvent.click(within(card).getByRole("button", { name: "Установить" }));
    expect(await screen.findByRole("dialog", { name: "Установить «CF Mod»" })).toBeTruthy();
    expect(document.querySelector(".project-detail-page")).toBeNull();
  });
});
