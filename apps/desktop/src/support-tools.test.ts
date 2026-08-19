import { describe, expect, it, vi } from "vitest";

import { inspectLibraryConsistency, traceAssetSupport } from "./support-tools";

describe("support tool command wire", () => {
  it("uses fixed read-only commands without accepting filesystem paths", async () => {
    const call = vi.fn().mockResolvedValue({});
    const assetId = "0198a9b2-43c0-7cb0-a733-6dc58f829815";

    await inspectLibraryConsistency(call);
    await traceAssetSupport(assetId, call);

    expect(call.mock.calls).toEqual([
      ["inspect_library_consistency"],
      ["trace_asset_support", { assetId }],
    ]);
  });
});
