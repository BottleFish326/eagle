import { describe, expect, it, vi } from "vitest";
import {
  createSavedFilter,
  deleteSavedFilter,
  executeSavedFilter,
  listSavedFilters,
  renameSavedFilter,
  updateSavedFilter,
  type SavedFilterFileVersion,
  type SavedFilterInput,
} from "./saved-filters";

const version: SavedFilterFileVersion = {
  exists: true,
  size: 128,
  modifiedUnixMs: 1_700_000_000_000,
  sha256: "a".repeat(64),
};

const input: SavedFilterInput = {
  name: "本周参考",
  query: "tag:reference modified:>=2026-08-14",
  scope: { kind: "selected-roots", rootIds: ["root-1"] },
  sort: { field: "modified-at", direction: "descending" },
};

describe("saved filter command contract", () => {
  it("lists and executes without accepting a filesystem path", async () => {
    const call = vi.fn().mockResolvedValue({});

    await listSavedFilters(call);
    await executeSavedFilter("filter-1", call);

    expect(call).toHaveBeenNthCalledWith(1, "list_saved_filters");
    expect(call).toHaveBeenNthCalledWith(2, "execute_saved_filter", {
      id: "filter-1",
    });
  });

  it("passes the full optimistic file version to every mutation", async () => {
    const call = vi.fn().mockResolvedValue({});

    await createSavedFilter(version, input, call);
    await updateSavedFilter(version, "filter-1", input, call);
    await renameSavedFilter(version, "filter-1", "新名称", call);
    await deleteSavedFilter(version, "filter-1", call);

    expect(call).toHaveBeenNthCalledWith(1, "create_saved_filter", {
      expectedVersion: version,
      input,
    });
    expect(call).toHaveBeenNthCalledWith(2, "update_saved_filter", {
      expectedVersion: version,
      id: "filter-1",
      input,
    });
    expect(call).toHaveBeenNthCalledWith(3, "rename_saved_filter", {
      expectedVersion: version,
      id: "filter-1",
      name: "新名称",
    });
    expect(call).toHaveBeenNthCalledWith(4, "delete_saved_filter", {
      expectedVersion: version,
      id: "filter-1",
    });
  });

  it("keeps persisted inputs free of result keys and asset snapshots", () => {
    expect(Object.keys(input).sort()).toEqual([
      "name",
      "query",
      "scope",
      "sort",
    ]);
    expect(JSON.stringify(input)).not.toMatch(/orderedKeys|assetKeys|records/u);
  });
});
