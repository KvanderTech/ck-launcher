import { describe, expect, it, vi } from "vitest";

import { invokeLaunchOrInstall } from "./tauri";

describe("final launch command contract", () => {
  it("uses only launch_or_install for the Play operation", async () => {
    const invoke = vi.fn(async () => "operation-current");

    await expect(invokeLaunchOrInstall("default", invoke)).resolves.toBe("operation-current");
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("launch_or_install", { profileId: "default" });
  });
});
