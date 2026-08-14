import { describe, expect, it, vi } from "vitest";

import { queryAssets } from "./asset-query";

describe("asset query command", () => {
  it("sends the expression through a typed input object", async () => {
    const result = {
      expression: "ui/* -draft type:image",
      query: {
        allTags: ["ui/*"],
        anyTagGroups: [],
        excludedTags: ["draft"],
        kinds: ["image" as const],
        extensions: [],
        favorite: null,
      },
      keys: ["/assets/logo.png"],
      totalAssets: 2,
    };
    const call = vi.fn().mockResolvedValue(result);

    await expect(
      queryAssets({ expression: result.expression }, call),
    ).resolves.toEqual(result);
    expect(call).toHaveBeenCalledWith("query_assets", {
      input: { expression: result.expression },
    });
  });
});
