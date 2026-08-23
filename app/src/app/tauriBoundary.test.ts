import { describe, expect, it } from "vitest";

const sourceModules = import.meta.glob(["../**/*.ts", "../**/*.tsx"], {
  eager: true,
  import: "default",
  query: "?raw",
}) as Record<string, string>;

describe("Tauri frontend boundary", () => {
  it("keeps direct core invoke imports inside app/tauri.ts", () => {
    for (const [path, source] of Object.entries(sourceModules)) {
      if (path === "./tauri.ts" || path.endsWith("/app/tauri.ts")) continue;
      expect(source, path).not.toContain("@tauri-apps/api/core");
    }
  });
});
