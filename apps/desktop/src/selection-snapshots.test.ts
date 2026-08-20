import { describe, expect, it, vi } from "vitest";

import {
  createExplicitSelectionSnapshot,
  createQuerySelectionSnapshot,
  createRangeSelectionSnapshot,
  getSelectionSessionStats,
  releaseSelectionSnapshot,
} from "./selection-snapshots";

describe("selection snapshot commands", () => {
  it("creates query, range, and explicit snapshots without sending paths", async () => {
    const summary = {
      id: "019c0000-0000-7000-8000-000000000001",
      catalogRevision: 11,
      itemCount: 2,
      createdAt: "2026-08-21T00:00:00.000Z",
      expiresAt: "2026-08-21T00:15:00.000Z",
    };
    const call = vi.fn().mockResolvedValue(summary);
    const query = {
      expectedCatalogRevision: 11,
      expression: "tag:ui type:image",
      scopeRootIds: ["019c0000-0000-7000-8000-000000000002"],
      sort: { field: "file-name" as const, direction: "ascending" as const },
    };

    await createQuerySelectionSnapshot(query, call);
    await createRangeSelectionSnapshot(
      { ...query, anchorKey: "asset-a", targetKey: "asset-b" },
      call,
    );
    await createExplicitSelectionSnapshot(
      { expectedCatalogRevision: 11, keys: ["asset-b", "asset-a"] },
      call,
    );

    expect(call.mock.calls).toEqual([
      ["create_query_selection_snapshot", { input: query }],
      [
        "create_range_selection_snapshot",
        { input: { ...query, anchorKey: "asset-a", targetKey: "asset-b" } },
      ],
      [
        "create_explicit_selection_snapshot",
        { input: { expectedCatalogRevision: 11, keys: ["asset-b", "asset-a"] } },
      ],
    ]);
    expect(JSON.stringify(call.mock.calls)).not.toContain("/Users/");
  });

  it("releases opaque IDs and reads bounded counters", async () => {
    const call = vi.fn().mockResolvedValueOnce(true).mockResolvedValueOnce({
      snapshotCount: 0,
      totalItemCount: 0,
      maximumSnapshotCount: 32,
      maximumItemCount: 100_000,
      maximumTotalItemCount: 200_000,
    });
    await expect(releaseSelectionSnapshot("snapshot-id", call)).resolves.toBe(true);
    await getSelectionSessionStats(call);
    expect(call).toHaveBeenNthCalledWith(1, "release_selection_snapshot", {
      snapshotId: "snapshot-id",
    });
    expect(call).toHaveBeenNthCalledWith(2, "selection_session_stats");
  });
});
