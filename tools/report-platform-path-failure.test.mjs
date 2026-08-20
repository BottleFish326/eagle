import assert from "node:assert/strict";
import { test } from "node:test";

import {
  formatMissingPlatformPathReportAnnotation,
  formatPlatformPathFailureAnnotation,
} from "./report-platform-path-failure.mjs";

test("formats the receipt failure and Cargo tail as one escaped annotation", () => {
  const annotation = formatPlatformPathFailureAnnotation({
    failures: ["suite failed: one, two"],
    processResults: {
      test: {
        stdout: "test first ... ok\r\n",
        stderr: "thread panicked at 50%\nlast line",
      },
    },
  });

  assert.equal(
    annotation,
    "::error title=P2-A12 platform path rejection::suite failed: one, two%0A%0Atest first ... ok%0A%0Athread panicked at 50%25%0Alast line",
  );
});

test("keeps the final diagnostic characters when output is bounded", () => {
  const annotation = formatPlatformPathFailureAnnotation(
    {
      failures: ["failure"],
      processResults: { test: { stdout: "prefix-123456789", stderr: "" } },
    },
    { maxDetailCharacters: 8 },
  );

  assert.match(annotation, /final 8 characters/);
  assert.match(annotation, /23456789$/);
  assert.doesNotMatch(annotation, /prefix/);
});

test("formats a missing report without throwing", () => {
  assert.equal(
    formatMissingPlatformPathReportAnnotation(new Error("missing\nreport")),
    "::error title=P2-A12 report unavailable::missing%0Areport",
  );
});
