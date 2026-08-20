import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { SavedFilterManager } from "./SavedFilterManager";
import type { SavedFilterCatalog } from "./saved-filters";

const catalog: SavedFilterCatalog = {
  fileVersion: {
    exists: true,
    size: 128,
    modifiedUnixMs: 1_700_000_000_000,
    sha256: "a".repeat(64),
  },
  validFilters: [
    {
      id: "filter-1",
      name: "参考",
      query: "tag:reference",
      scope: { kind: "all-enabled-roots" },
      sort: { field: "modified-at", direction: "descending" },
      createdAt: "2026-08-20T00:00:00.000Z",
      updatedAt: "2026-08-20T00:00:00.000Z",
    },
  ],
  unavailableFilters: [],
  invalidEntries: [],
  fileIssues: [],
};

describe("SavedFilterManager", () => {
  it("does not render while closed", () => {
    expect(render(false)).toBe("");
  });

  it("explains filesystem-only execution and exposes labeled controls", () => {
    const html = render(true);
    expect(html).toContain('role="dialog"');
    expect(html).toContain("结果会从当前文件重新计算");
    expect(html).toContain("tag:reference");
    expect(html).toContain("保存当前视图");
    expect(html).not.toContain("assetKeys");
  });
});

function render(open: boolean): string {
  return renderToStaticMarkup(
    <SavedFilterManager
      activeFilterId="filter-1"
      busy={false}
      catalog={catalog}
      currentQuery="tag:reference"
      onActivate={async () => undefined}
      onClose={() => undefined}
      onCreate={async () => undefined}
      onDelete={async () => undefined}
      onRefresh={async () => undefined}
      onRename={async () => undefined}
      onUpdate={async () => undefined}
      open={open}
      roots={[]}
    />,
  );
}
