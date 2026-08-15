import { describe, expect, it } from "vitest";

import { createDemoDesktopApi, demoAssetCountFromSearch } from "./demo-api";

describe("demo dataset selection", () => {
  it("enables the M dataset only for an explicit development URL", () => {
    expect(demoAssetCountFromSearch("?demoDataset=medium", true)).toBe(10_000);
    expect(demoAssetCountFromSearch("?demoDataset=medium", false)).toBe(16);
    expect(demoAssetCountFromSearch("?demoDataset=large", true)).toBe(16);
    expect(demoAssetCountFromSearch("", true)).toBe(16);
  });

  it("resolves every M dataset Vault reference through the key index", async () => {
    const api = createDemoDesktopApi({ assetCount: 10_000 });
    const [vault] = await api.listObsidianVaults();
    const query = await api.queryAssets({ expression: "" });
    const result = await api.resolveObsidianVaultReferences({
      vaultId: vault.id,
      assetKeys: query.keys,
    });

    expect(result.failures).toEqual([]);
    expect(result.resolved).toHaveLength(10_000);
    expect(new Set(result.resolved.map((item) => item.assetKey)).size).toBe(
      10_000,
    );
  });
});
