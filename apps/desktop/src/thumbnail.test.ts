import { describe, expect, it, vi } from "vitest";

import {
  clearThumbnailCache,
  readThumbnail,
  requestThumbnail,
  type ThumbnailOutcome,
} from "./thumbnail";

describe("thumbnail commands", () => {
  it("requests generation only for the asset selected by the caller", async () => {
    const outcome: ThumbnailOutcome = {
      status: "ready",
      thumbnail: {
        assetKey: "/assets/logo.png",
        cacheKey: "a".repeat(64),
        mime: "image/png",
        width: 128,
        height: 64,
        sourceSize: 1024,
        sourceModifiedUnixMs: 1234,
        cacheHit: false,
        decoderVersion: "test-v1",
      },
    };
    const call = vi.fn().mockResolvedValue(outcome);

    await expect(
      requestThumbnail({ assetKey: "/assets/logo.png", maxEdge: 256 }, call),
    ).resolves.toEqual(outcome);
    expect(call).toHaveBeenCalledOnce();
    expect(call).toHaveBeenCalledWith("request_thumbnail", {
      input: { assetKey: "/assets/logo.png", maxEdge: 256 },
    });
  });

  it("reads raw cached bytes without a JSON byte-array wrapper", async () => {
    const bytes = new Uint8Array([137, 80, 78, 71]).buffer;
    const call = vi.fn().mockResolvedValue(bytes);

    await expect(readThumbnail("b".repeat(64), call)).resolves.toBe(bytes);
    expect(call).toHaveBeenCalledWith("read_thumbnail", {
      cacheKey: "b".repeat(64),
    });
  });

  it("clears only the backend-owned thumbnail cache", async () => {
    const report = { removedFiles: 4, removedBytes: 2048 };
    const call = vi.fn().mockResolvedValue(report);

    await expect(clearThumbnailCache(call)).resolves.toEqual(report);
    expect(call).toHaveBeenCalledWith("clear_thumbnail_cache");
  });
});
