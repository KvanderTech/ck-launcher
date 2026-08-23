import { describe, expect, it } from "vitest";

import { progressLabel } from "./types";

describe("progressLabel", () => {
  it("formats a download fixture with its file name and percentage", () => {
    expect(
      progressLabel({
        operationId: "download-client",
        stage: "downloading",
        completedBytes: 50,
        totalBytes: 100,
        currentFile: "client.jar",
      }),
    ).toBe("Загрузка client.jar · 50%");
  });
});
