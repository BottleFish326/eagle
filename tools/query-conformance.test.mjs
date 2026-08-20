import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";

import {
  buildQueryConformanceReport,
  evaluateOracleCase,
  inspectQueryManifest,
  loadQueryManifest,
} from "./query-conformance.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const examplePath = path.join(
  repository,
  "specs",
  "examples",
  "query-conformance-manifest.example.json",
);

function productReport(manifest) {
  return {
    schema: 1,
    recordCount: manifest.records.length,
    validCases: manifest.validCases.map((entry) => ({
      id: entry.id,
      elapsedNanoseconds: 100,
      keys: [...entry.expectedKeys],
      error: null,
    })),
    invalidCases: manifest.invalidCases.map((entry) => ({
      id: entry.id,
      elapsedNanoseconds: 100,
      keys: null,
      error: { kind: entry.errorKind, offset: entry.offset },
    })),
  };
}

test("accepts the schema-bound example with an independent oracle", async () => {
  const { manifest, sha256 } = await loadQueryManifest(examplePath);
  assert.equal(inspectQueryManifest(manifest).accepted, true);
  assert.deepEqual(
    evaluateOracleCase(manifest.records, manifest.validCases[0]),
    ["asset-a"],
  );
  const report = buildQueryConformanceReport({
    manifest,
    manifestSha256: sha256,
    productReport: productReport(manifest),
    gitCommit: "a".repeat(40),
  });
  assert.equal(report.accepted, true, report.failures.join("; "));
});

test("detects independent product, expected, and oracle mutations", async () => {
  const { manifest, sha256 } = await loadQueryManifest(examplePath);

  const changedProduct = productReport(manifest);
  changedProduct.validCases[0].keys = [];
  const productFailure = buildQueryConformanceReport({
    manifest,
    manifestSha256: sha256,
    productReport: changedProduct,
  });
  assert.ok(
    productFailure.failures.some((failure) =>
      failure.includes("product differs"),
    ),
  );

  const changedExpected = structuredClone(manifest);
  changedExpected.validCases[0].expectedKeys = [];
  const expectedFailure = buildQueryConformanceReport({
    manifest: changedExpected,
    manifestSha256: sha256,
    productReport: productReport(manifest),
  });
  assert.ok(
    expectedFailure.failures.some((failure) =>
      failure.includes("oracle differs"),
    ),
  );

  const changedOracle = structuredClone(manifest);
  changedOracle.validCases[0].oracle.find(
    (predicate) => predicate.field === "rating",
  ).value = 6;
  const oracleFailure = buildQueryConformanceReport({
    manifest: changedOracle,
    manifestSha256: sha256,
    productReport: productReport(manifest),
  });
  assert.ok(
    oracleFailure.failures.some((failure) =>
      failure.includes("oracle differs"),
    ),
  );
});

test("rejects semantic duplicates, non-NFC inputs, and undersized formal corpora", async () => {
  const { manifest } = await loadQueryManifest(examplePath);
  const changed = structuredClone(manifest);
  changed.records[1].key = changed.records[0].key;
  changed.records[0].relativePath = "Brand Assets/e\u0301.png";
  changed.validCases[0].expectedKeys.reverse();
  const inspection = inspectQueryManifest(changed, { formal: true });
  assert.equal(inspection.accepted, false);
  assert.ok(
    inspection.failures.some((failure) =>
      failure.includes("duplicate record key"),
    ),
  );
  assert.ok(inspection.failures.some((failure) => failure.includes("not NFC")));
  assert.ok(
    inspection.failures.some((failure) => failure.includes("fewer than 40")),
  );
  assert.ok(
    inspection.failures.some((failure) => failure.includes("fewer than 60")),
  );
  assert.ok(
    inspection.failures.some((failure) => failure.includes("fewer than 24")),
  );
});
