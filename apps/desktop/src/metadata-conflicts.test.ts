import { describe, expect, it, vi } from "vitest";

import {
  dismissMetadataConflict,
  resolveMetadataConflict,
} from "./metadata-conflicts";

describe("metadata conflict commands", () => {
  it("resolves an opaque conflict id with explicit field choices", async () => {
    const call = vi.fn().mockResolvedValue({ key: "/assets/one.png" });
    const resolution = {
      tags: "merge" as const,
      fields: { note: "use-mine" as const },
    };

    await resolveMetadataConflict("conflict-id", resolution, call);

    expect(call).toHaveBeenCalledWith("resolve_metadata_conflict", {
      input: { conflictId: "conflict-id", resolution },
    });
  });

  it("dismisses an opaque conflict id without accepting a file path", async () => {
    const call = vi.fn().mockResolvedValue(undefined);

    await dismissMetadataConflict("conflict-id", call);

    expect(call).toHaveBeenCalledWith("dismiss_metadata_conflict", {
      conflictId: "conflict-id",
    });
  });
});
