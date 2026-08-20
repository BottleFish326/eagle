import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { AssetThumbnailFallback } from "./AssetThumbnail";
import type { AssetRecord } from "./scanner";

describe("AssetThumbnailFallback", () => {
  it.each(["codec-unavailable", "preview-unavailable"] as const)(
    "renders %s as a neutral type card",
    (reason) => {
      const html = renderToStaticMarkup(
        <AssetThumbnailFallback
          asset={asset("clip.mp4", "mp4", "video")}
          message="provider is not installed"
          reason={reason}
        />,
      );

      expect(html).toContain("preview-placeholder--neutral");
      expect(html).toContain("MP4 视频");
      expect(html).not.toContain("无法预览");
    },
  );

  it.each([
    "invalid-content",
    "unreadable",
    "resource-limited",
    "timed-out",
  ] as const)("renders %s as a visible error state", (reason) => {
    const html = renderToStaticMarkup(
      <AssetThumbnailFallback
        asset={asset("broken.png", "png", "image")}
        message="preview failed"
        reason={reason}
      />,
    );

    expect(html).toContain("preview-placeholder--error");
    expect(html).toContain("无法预览");
    expect(html).toContain("preview failed");
  });
});

function asset(fileName: string, extension: string, kind: AssetRecord["kind"]) {
  return { fileName, extension, kind } as AssetRecord;
}
