import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import { buildQueryConformanceFixture } from "./generate-query-conformance-fixture.mjs";
import {
  buildQueryConformanceReport,
  executeProductQueryGate,
  inspectQueryManifest,
  loadQueryManifest,
} from "./query-conformance.mjs";

const repository = path.resolve(import.meta.dirname, "..");

test("tracked query corpus exactly matches deterministic generation", async () => {
  const generated = buildQueryConformanceFixture();
  const manifestPath = path.join(
    repository,
    "fixtures",
    "queries",
    "manifest.json",
  );
  const tracked = JSON.parse(await readFile(manifestPath, "utf8"));
  const schemaValidated = await loadQueryManifest(manifestPath);
  assert.deepEqual(tracked, generated);
  assert.deepEqual(schemaValidated.manifest, generated);
  assert.equal(generated.records.length, 64);
  assert.equal(generated.validCases.length, 64);
  assert.equal(generated.invalidCases.length, 25);
  const inspection = inspectQueryManifest(generated, { formal: true });
  assert.equal(inspection.accepted, true, inspection.failures.join("; "));
});

test("formal product parser and index match every fixed expected result", async () => {
  const manifestPath = path.join(
    repository,
    "fixtures",
    "queries",
    "manifest.json",
  );
  const { manifest, sha256 } = await loadQueryManifest(manifestPath);
  const productReport = await executeProductQueryGate(manifestPath);
  const report = buildQueryConformanceReport({
    manifest,
    manifestSha256: sha256,
    productReport,
    formal: true,
  });
  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.equal(
    report.validCases.every((entry) => entry.productMatchesExpected),
    true,
  );
  assert.equal(
    report.invalidCases.every((entry) => entry.productMatchesExpected),
    true,
  );
});

test("formal corpus covers every field and stable parse error family", () => {
  const generated = buildQueryConformanceFixture();
  const fields = new Set(
    generated.validCases.flatMap((entry) =>
      entry.oracle.map((predicate) => predicate.field),
    ),
  );
  assert.deepEqual(
    fields,
    new Set([
      "tag",
      "type",
      "extension",
      "favorite",
      "rating",
      "size",
      "width",
      "height",
      "aspect",
      "created",
      "modified",
      "duration",
      "pages",
      "orientation",
      "root",
      "path",
      "color-space",
      "has-note",
      "has-alpha",
    ]),
  );
  const errorKinds = new Set(
    generated.invalidCases.map((entry) => entry.errorKind),
  );
  assert.deepEqual(
    errorKinds,
    new Set([
      "unclosed-quote",
      "trailing-escape",
      "empty-tag",
      "tag-too-long",
      "invalid-wildcard",
      "invalid-or-group",
      "unknown-filter",
      "invalid-type",
      "invalid-extension",
      "invalid-favorite",
      "conflicting-favorite",
      "invalid-operator",
      "invalid-integer",
      "invalid-unit",
      "numeric-overflow",
      "invalid-ratio",
      "invalid-date",
      "invalid-enum",
      "invalid-root-id",
      "invalid-path",
      "unsupported-unknown",
      "conflicting-range",
      "conflicting-value",
    ]),
  );
  assert.ok(
    generated.validCases.find((entry) => entry.id === "combined-advanced")
      .expectedKeys.length > 0,
  );
  assert.ok(
    generated.validCases.find((entry) => entry.id === "large-candidate-set")
      .expectedKeys.length >= 10,
  );
});
