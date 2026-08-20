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

  it("provides read-only consistency and stable-ID trace support data", async () => {
    const api = createDemoDesktopApi();
    const consistency = await api.inspectLibraryConsistency();
    const trace = await api.traceAssetSupport(
      "0198a9b2-43c0-7cb0-a733-000000000001",
    );

    expect(consistency.authoritative).toBe(true);
    expect(consistency.summary.catalogAssets).toBe(16);
    expect(trace.matchCount).toBe(1);
    expect(trace.steps.some((step) => step.code === "id-matched")).toBe(true);
    expect(JSON.stringify(trace)).not.toContain("/Users/demo");
  });

  it("models saved filters as versioned definitions with ephemeral results", async () => {
    const api = createDemoDesktopApi();
    const initial = await api.listSavedFilters();
    const created = await api.createSavedFilter(initial.fileVersion, {
      name: "图片",
      query: "type:image",
      scope: { kind: "all-enabled-roots" },
      sort: { field: "file-name", direction: "ascending" },
    });
    const filter = created.filter;
    expect(filter).not.toBeNull();
    if (filter === null) return;

    const execution = await api.executeSavedFilter(filter.id);
    const listed = await api.listSavedFilters();

    expect(execution.matchedAssets).toBeGreaterThan(0);
    expect(execution.orderedKeys).toEqual([...execution.orderedKeys].sort());
    expect(JSON.stringify(listed)).not.toMatch(
      /orderedKeys|assetKeys|records/u,
    );
    await expect(
      api.deleteSavedFilter(initial.fileVersion, filter.id),
    ).rejects.toMatchObject({ kind: "external-change" });
    await api.deleteSavedFilter(listed.fileVersion, filter.id);
    expect((await api.listSavedFilters()).validFilters).toEqual([]);
  });
});
