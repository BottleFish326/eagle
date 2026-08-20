import { describe, expect, it } from "vitest";

import type { AssetRecord } from "./scanner";
import {
  composeAssetQuery,
  cycleTagFilter,
  formatQueryError,
  matchesDemoExpression,
  nextGridIndex,
  reconcileSelectedKeys,
  reconcileSelectionAnchor,
  settleFailedQuery,
  settleSuccessfulQuery,
  summarizeTags,
} from "./ui-model";

describe("desktop UI model", () => {
  it("cycles a tag through neutral, include and exclude", () => {
    expect(cycleTagFilter("neutral")).toBe("include");
    expect(cycleTagFilter("include")).toBe("exclude");
    expect(cycleTagFilter("exclude")).toBe("neutral");
  });

  it("composes explicit, escaped tag filters with the search expression", () => {
    expect(
      composeAssetQuery("type:image", {
        "state/draft": "exclude",
        "visual style/minimal": "include",
        unused: "neutral",
      }),
    ).toBe('type:image -tag:"state/draft" tag:"visual style/minimal"');
  });

  it("summarizes tags and keeps active states", () => {
    const assets = [
      record("one", ["ui/icon", "color/blue"]),
      record("two", ["ui/icon", "color/red"]),
    ];

    expect(summarizeTags(assets, { "ui/icon": "exclude" })[0]).toEqual({
      tag: "ui/icon",
      count: 2,
      state: "exclude",
    });
  });

  it("moves keyboard focus without escaping grid bounds", () => {
    expect(nextGridIndex(1, 10, 4, "ArrowDown")).toBe(5);
    expect(nextGridIndex(9, 10, 4, "ArrowDown")).toBe(9);
    expect(nextGridIndex(0, 10, 4, "ArrowLeft")).toBe(0);
    expect(nextGridIndex(5, 10, 4, "Home")).toBe(0);
    expect(nextGridIndex(5, 10, 4, "End")).toBe(9);
  });

  it("keeps selection and anchor on an asset that moved with its stable sidecar", () => {
    const moves = [{ fromKey: "/assets/old.png", toKey: "/assets/new.png" }];
    const removed = ["/assets/old.png", "/assets/deleted.png"];

    expect(
      reconcileSelectedKeys(
        new Set(["/assets/old.png", "/assets/deleted.png"]),
        moves,
        removed,
      ),
    ).toEqual(new Set(["/assets/new.png"]));
    expect(reconcileSelectionAnchor("/assets/old.png", moves, removed)).toBe(
      "/assets/new.png",
    );
  });

  it("matches the demo query with the same visible filter forms", () => {
    const asset = {
      ...record("one", ["ui/icon", "color/blue"]),
      rating: 4,
      modifiedUnixMs: Date.parse("2026-08-20T00:00:00Z"),
    };
    expect(
      matchesDemoExpression(
        asset,
        'type:image tag:"ui/icon" -tag:"state/draft"',
      ),
    ).toBe(true);
    expect(matchesDemoExpression(asset, 'tag:"color/red"')).toBe(false);
    expect(
      matchesDemoExpression(
        asset,
        "rating:>=4 width:>=100 aspect:1/1 modified:>=2026-08-19T00:00:00Z",
      ),
    ).toBe(true);
    expect(matchesDemoExpression(asset, "rating:>4")).toBe(false);
    expect(matchesDemoExpression(asset, "duration:unknown")).toBe(true);
  });

  it("reports structured UTF-8 query positions and preserves legal results", () => {
    const successful = settleSuccessfulQuery(["one", "two"]);
    const failed = settleFailedQuery(successful, {
      kind: "parse",
      error: {
        kind: "invalid-integer",
        offset: 7,
        token: "width:abc",
        message: "value must be an unsigned decimal integer",
      },
    });

    expect(failed.visibleKeys).toEqual(["one", "two"]);
    expect(failed.error).toContain("invalid-integer");
    expect(failed.error).toContain("UTF-8 字节 7");
    expect(failed.error).toContain("width:abc");
    expect(formatQueryError("offline")).toBe("offline");
  });
});

function record(key: string, tags: string[]): AssetRecord {
  return {
    key,
    id: null,
    rootId: "root",
    path: `/assets/${key}.png`,
    relativePath: `${key}.png`,
    sidecarPath: null,
    sidecarState: null,
    fileName: `${key}.png`,
    extension: "png",
    mime: "image/png",
    kind: "image",
    size: 1024,
    createdUnixMs: 0,
    modifiedUnixMs: 0,
    fileReadOnly: false,
    dimensions: { width: 100, height: 100 },
    nativeMetadata: null,
    media: null,
    tags,
    rating: 0,
    favorite: false,
    note: "",
    aliases: [],
    issues: [],
  };
}
