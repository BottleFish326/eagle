import { describe, expect, it, vi } from "vitest";

import {
  continueMetadataTransaction,
  dismissMetadataTransaction,
  listMetadataTransactions,
  restoreMetadataTransaction,
} from "./metadata-transactions";

describe("metadata transaction commands", () => {
  it("lists durable journals without accepting a client directory", async () => {
    const call = vi.fn().mockResolvedValue([]);

    await listMetadataTransactions(call);

    expect(call).toHaveBeenCalledWith("list_metadata_transactions");
  });

  it("continues, restores and dismisses by opaque transaction id", async () => {
    const call = vi.fn().mockResolvedValue({
      summary: { id: "transaction-id" },
      failures: [],
    });

    await continueMetadataTransaction("transaction-id", call);
    await restoreMetadataTransaction("transaction-id", call);
    await dismissMetadataTransaction("transaction-id", call);

    expect(call).toHaveBeenNthCalledWith(1, "continue_metadata_transaction", {
      id: "transaction-id",
    });
    expect(call).toHaveBeenNthCalledWith(2, "restore_metadata_transaction", {
      id: "transaction-id",
    });
    expect(call).toHaveBeenNthCalledWith(3, "dismiss_metadata_transaction", {
      id: "transaction-id",
    });
  });
});
