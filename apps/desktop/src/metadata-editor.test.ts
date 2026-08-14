import { describe, expect, it, vi } from "vitest";

import { editAssetMetadata } from "./metadata-editor";

describe("metadata editor command", () => {
  it("sends per-asset versions with a shared batch patch", async () => {
    const call = vi.fn().mockResolvedValue({ updated: [], failures: [] });
    const input = {
      targets: [
        { key: "/assets/one.png", expectedSidecarDigest: null },
        { key: "/assets/two.png", expectedSidecarDigest: "abc123" },
      ],
      patch: { addTags: ["project/eagle", "ui/icon"], favorite: true },
    };

    await editAssetMetadata(input, call);

    expect(call).toHaveBeenCalledWith("edit_asset_metadata", { input });
  });
});
