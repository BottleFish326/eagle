import { describe, expect, it } from "vitest";

import { calculateAssetGridWindow } from "./AssetGrid";

describe("asset grid virtual window", () => {
  it("bounds the rendered window for a medium dataset", () => {
    const result = calculateAssetGridWindow({
      containerWidth: 700,
      itemCount: 10_000,
      viewportHeight: 800,
      viewportOffset: 0,
      windowWidth: 1_100,
    });

    expect(result.columns).toBe(4);
    expect(result.startIndex).toBe(0);
    expect(result.endIndex).toBeLessThan(100);
    expect(result.totalHeight).toBeGreaterThan(100_000);
  });

  it("keeps far-scroll indices inside the collection", () => {
    const result = calculateAssetGridWindow({
      containerWidth: 700,
      itemCount: 10_000,
      viewportHeight: 800,
      viewportOffset: 1_000_000,
      windowWidth: 1_100,
    });

    expect(result.startIndex).toBeLessThan(10_000);
    expect(result.endIndex).toBe(10_000);
    expect(result.endIndex - result.startIndex).toBeLessThan(100);
  });

  it("renders a small collection in one bounded window", () => {
    const result = calculateAssetGridWindow({
      containerWidth: 900,
      itemCount: 16,
      viewportHeight: 800,
      viewportOffset: 0,
      windowWidth: 1_300,
    });

    expect(result.startIndex).toBe(0);
    expect(result.endIndex).toBe(16);
  });
});
