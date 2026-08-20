import { describe, expect, it, vi } from "vitest";

import { prepareMetadataBatch, releaseBatchPreflight } from "./batch-workflows";

describe("batch preflight commands", () => {
  it("sends only an opaque snapshot ID and operation parameters", async () => {
    const call = vi.fn().mockResolvedValue({ operationId: "operation-id" });
    const input = {
      snapshotId: "snapshot-id",
      patch: { addTags: ["reviewed"], removeTags: [] },
    };
    await prepareMetadataBatch(input, call);
    expect(call).toHaveBeenCalledWith("prepare_metadata_batch", { input });
    expect(JSON.stringify(call.mock.calls)).not.toContain("/Users/");
  });

  it("releases an operation by opaque ID", async () => {
    const call = vi.fn().mockResolvedValue(true);
    await expect(releaseBatchPreflight("operation-id", call)).resolves.toBe(true);
    expect(call).toHaveBeenCalledWith("release_batch_preflight", {
      operationId: "operation-id",
    });
  });
});
