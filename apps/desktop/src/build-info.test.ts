import { describe, expect, it } from "vitest";

import { loadBuildInfo } from "./build-info";

describe("loadBuildInfo", () => {
  it("returns traceable backend build information", async () => {
    const result = await loadBuildInfo(async (command) => {
      expect(command).toBe("build_info");
      return {
        version: "0.1.0",
        gitCommit: "abc123",
        buildTarget: "aarch64-apple-darwin",
        buildProfile: "debug",
        rustcVersion: "rustc 1.97.1",
      };
    });

    expect(result.gitCommit).toBe("abc123");
    expect(result.buildTarget).toContain("apple");
  });

  it("uses a deterministic browser fallback when IPC is unavailable", async () => {
    const result = await loadBuildInfo(async () => {
      throw new Error("not running in Tauri");
    });

    expect(result).toMatchObject({
      gitCommit: "web-preview",
      buildTarget: "browser",
    });
  });
});
