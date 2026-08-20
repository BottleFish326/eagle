import assert from "node:assert/strict";
import test from "node:test";

import {
  buildSoakBaselineAudit,
  FORMAL_SOAK_BASELINE_COMMIT,
  FORMAL_SOAK_LOADED_PATHS,
  FORMAL_SOAK_PRODUCT_SCOPES,
} from "./soak-baseline-audit.mjs";

const currentCommit = "a".repeat(40);

test("accepts an unchanged descendant and emits the fixed audit scope", () => {
  assert.equal(
    FORMAL_SOAK_BASELINE_COMMIT,
    "c0508c66ec6905decc1e70cc328e585a887fb6c5",
  );
  const report = buildSoakBaselineAudit({
    baselineCommit: FORMAL_SOAK_BASELINE_COMMIT,
    currentCommit,
    descendantOfBaseline: true,
    loadedChangedPaths: [],
    productChangedPaths: [],
  });

  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.deepEqual(report.loadedInputs.scopes, FORMAL_SOAK_LOADED_PATHS);
  assert.deepEqual(report.productInputs.scopes, FORMAL_SOAK_PRODUCT_SCOPES);
  assert.deepEqual(report.loadedInputs.changedPaths, []);
  assert.deepEqual(report.productInputs.changedPaths, []);
});

test("rejects source drift, an unrelated history, and invalid input paths", () => {
  const report = buildSoakBaselineAudit({
    baselineCommit: FORMAL_SOAK_BASELINE_COMMIT,
    currentCommit,
    descendantOfBaseline: false,
    loadedChangedPaths: [
      "tools/verify-resource-stability.mjs",
      "tools/verify-resource-stability.mjs",
      "/outside",
    ],
    productChangedPaths: ["crates/asset-core/src/lib.rs", "../outside"],
  });

  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.includes(
      "current commit does not descend from the soak baseline",
    ),
  );
  assert.ok(
    report.failures.includes(
      "formal soak-loaded inputs differ from the baseline: tools/verify-resource-stability.mjs",
    ),
  );
  assert.ok(
    report.failures.includes(
      "formal soak product inputs differ from the baseline: crates/asset-core/src/lib.rs",
    ),
  );
  assert.equal(
    report.failures.filter((failure) =>
      failure.includes("invalid repository path"),
    ).length,
    2,
  );
});

test("rejects malformed commits and non-array change sets", () => {
  const report = buildSoakBaselineAudit({
    baselineCommit: "main",
    currentCommit: null,
    descendantOfBaseline: true,
    loadedChangedPaths: null,
    productChangedPaths: {},
  });

  assert.equal(report.accepted, false);
  assert.ok(report.failures.includes("baseline commit is invalid"));
  assert.ok(report.failures.includes("current commit is invalid"));
  assert.ok(report.failures.includes("loaded changed paths are not an array"));
  assert.ok(report.failures.includes("product changed paths are not an array"));
});
