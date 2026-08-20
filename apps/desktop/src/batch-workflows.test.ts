import { describe, expect, it, vi } from "vitest";

import {
  cancelMetadataBatch,
  executeMetadataBatch,
  preflightConfirmation,
  prepareMetadataBatch,
  releaseBatchPreflight,
} from "./batch-workflows";

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

  it("binds execution to the complete preflight digest and streams progress", async () => {
    const summary = {
      operationId: "operation-id",
      snapshotId: "snapshot-id",
      catalogRevision: 9,
      requestedCount: 10,
      executableCount: 8,
      confirmationDigest: "a".repeat(64),
    };
    const call = vi.fn().mockResolvedValue({ stopped: false });
    const receive = vi.fn();
    const channel = { onmessage: vi.fn() };
    await executeMetadataBatch(
      preflightConfirmation(summary as never),
      receive,
      call,
      () => channel,
    );
    expect(channel.onmessage).toBe(receive);
    expect(call).toHaveBeenCalledWith("execute_metadata_batch", {
      confirmation: summary,
      onEvent: channel,
    });
    await cancelMetadataBatch("operation-id", call);
    expect(call).toHaveBeenLastCalledWith("cancel_metadata_batch", {
      operationId: "operation-id",
    });
  });
});
