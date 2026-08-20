import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import {
  AdvancedFilterBuilder,
  appendAdvancedPredicate,
  buildAdvancedPredicate,
} from "./AdvancedFilterBuilder";

describe("advanced filter builder", () => {
  it.each([
    ["rating", ">=", "4", "", "rating:>=4"],
    ["size", "<", "2", "GiB", "size:<2GiB"],
    ["duration", ">=", "30", "s", "duration:>=30s"],
    ["aspect", "=", "16/9", "", "aspect:16/9"],
    [
      "modified",
      ">=",
      "2026-08-19T00:00:00+08:00",
      "",
      "modified:>=2026-08-19T00:00:00+08:00",
    ],
    ["orientation", "=", "landscape", "", "orientation:landscape"],
    [
      "root",
      "=",
      "0198a9b2-43c0-7cb0-a733-000000000001",
      "",
      "root:0198a9b2-43c0-7cb0-a733-000000000001",
    ],
    ["color-space", "=", "display-p3", "", "color-space:display-p3"],
    ["has-note", "=", "false", "", "has-note:false"],
    ["has-alpha", "=", "unknown", "", "has-alpha:unknown"],
  ] as const)(
    "emits a protocol predicate for %s",
    (field, operator, value, unit, predicate) => {
      expect(buildAdvancedPredicate({ field, operator, value, unit })).toEqual({
        ok: true,
        predicate,
      });
    },
  );

  it("emits explicit unknown only for fields that support it", () => {
    expect(
      buildAdvancedPredicate({
        field: "width",
        operator: "unknown",
        value: "ignored",
      }),
    ).toEqual({ ok: true, predicate: "width:unknown" });
    expect(
      buildAdvancedPredicate({
        field: "rating",
        operator: "unknown",
        value: "ignored",
      }),
    ).toMatchObject({ ok: false });
  });

  it.each([
    { field: "rating", operator: "=", value: "6" },
    { field: "width", operator: "=", value: "0" },
    { field: "aspect", operator: "=", value: "16/0" },
    { field: "created", operator: ">=", value: "2026-08-19" },
    { field: "root", operator: "=", value: "NOT-A-UUID" },
    { field: "path", operator: "=", value: "../escape" },
    { field: "color-space", operator: "=", value: "Display P3" },
  ] as const)("rejects invalid visual input %#", (draft) => {
    expect(buildAdvancedPredicate(draft)).toMatchObject({ ok: false });
  });

  it("normalizes and quotes a safe path without changing the existing expression", () => {
    const result = buildAdvancedPredicate({
      field: "path",
      operator: "=",
      value: "Brand Assets/e\u0301",
    });
    expect(result).toEqual({
      ok: true,
      predicate: 'path:"Brand Assets/é"',
    });
    expect(appendAdvancedPredicate("  type:image ", "width:>=1920")).toBe(
      "type:image width:>=1920",
    );
  });

  it("renders labeled field, operator and value controls", () => {
    const html = renderToStaticMarkup(
      <AdvancedFilterBuilder onAdd={() => undefined} />,
    );
    expect(html).toContain('aria-label="添加高级属性条件"');
    expect(html).toContain('aria-expanded="false"');
  });
});
