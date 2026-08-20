import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";
import test from "node:test";

import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

import {
  buildP3FilterGatesReport,
  inspectP3FilterGatesReceipt,
  P3_FILTER_COUNT,
  P3_TAG_RENAME_FAULT_CASES,
} from "./p3-filter-gates.mjs";

const base = {
  gitCommit: "a".repeat(40),
  executedAt: "2026-08-21T01:00:00.000Z",
  repositoryClean: true,
  environment: {
    platform: "darwin",
    architecture: "arm64",
    nodeVersion: "v24.0.0",
    rustc: "rustc 1.89.0",
    cargo: "cargo 1.89.0",
  },
  binarySha256: "b".repeat(64),
  p3A04: {
    seed: {
      schema: 1,
      activeRootId: "019b76d0-0000-7000-8000-000000000001",
      offlineRootId: "019b76d0-0000-7000-8000-000000000002",
      assetCount: 4,
      savedFilterCount: 2,
      cacheCreated: true,
    },
    mutation: { schema: 1, assetCount: 5 },
    cacheRemoved: true,
    verify: {
      schema: 1,
      accepted: true,
      scannedAssetCount: 5,
      scanProblemCount: 0,
      allEnabledMatchCount: 4,
      selectedRootsMatchCount: 5,
      selectedMissingRootCount: 1,
      cacheAbsent: true,
      resultSnapshotAbsent: true,
      sourceSha256Unchanged: true,
    },
    adversarial: {
      schema: 1,
      accepted: true,
      validCount: 1,
      unavailableCount: 1,
      invalidCount: 6,
      unknownFieldsPreserved: true,
      externalChangeBlocked: true,
      externalBytesPreserved: true,
    },
  },
  faultCases: P3_TAG_RENAME_FAULT_CASES.map(({ point, action }) => ({
    point,
    action,
    abort: aborted(),
    recovery: recovery(
      action === "restore" ? "restored" : "completed",
      action === "restore"
        ? "restored"
        : action === "retain"
          ? "retained"
          : "updated",
      action,
    ),
  })),
  externalCases: [
    {
      target: "filter",
      faultPoint: "filter-before-replace",
      abort: aborted(),
      recovery: recovery("conflict", "pending", "restore"),
    },
    {
      target: "sidecar",
      faultPoint: "filter-after-replace",
      abort: aborted(),
      recovery: recovery("conflict", "updated", "restore"),
    },
  ],
  temporaryWorkspacesRemoved: true,
};

const schema = JSON.parse(
  await readFile(
    path.resolve(
      import.meta.dirname,
      "../schemas/p3-filter-gates-evidence.schema.json",
    ),
    "utf8",
  ),
);
const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);
const validateSchema = ajv.compile(schema);

test("accepts restart/cache isolation and all five real process fault boundaries", () => {
  const report = buildP3FilterGatesReport(structuredClone(base));
  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.equal(report.p3A05.assetCount, P3_FILTER_COUNT);
  assert.equal(report.p3A05.faultCases.length, 5);
  assert.equal(report.p3A05.externalCases.length, 2);
});

test("offline inspection accepts the exact compact receipt", () => {
  const report = buildP3FilterGatesReport(structuredClone(base));
  const inspection = inspectP3FilterGatesReceipt(report);
  assert.equal(inspection.accepted, true, inspection.failures.join("; "));
  assert.equal(
    validateSchema(report),
    true,
    JSON.stringify(validateSchema.errors),
  );
});

test("rejects missing aborts, incorrect recovery, cache snapshots, and source drift", () => {
  const input = structuredClone(base);
  input.faultCases[1].abort = successful();
  input.faultCases[3].recovery.filterOutcome = "updated";
  input.p3A04.verify.resultSnapshotAbsent = false;
  input.externalCases[1].recovery.externalSidecarPreserved = false;
  input.faultCases[4].recovery.sourceSha256Unchanged = false;
  const report = buildP3FilterGatesReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.includes("P3-A05 process did not abort: sidecars-mid"),
  );
  assert.ok(
    report.failures.includes("P3-A04 restart/cache receipt is invalid"),
  );
  assert.ok(
    report.failures.includes(
      "P3-A05 external Sidecar bytes were not preserved",
    ),
  );
});

test("offline inspection rejects provenance, ordering, digest, and accepted-only tampering", () => {
  const report = buildP3FilterGatesReport(structuredClone(base));
  report.extra = true;
  report.accepted = false;
  report.failures.push("tampered");
  report.build.binarySha256 = "bad";
  report.p3A05.faultCases.reverse();
  const inspection = inspectP3FilterGatesReceipt(report);
  assert.equal(inspection.accepted, false);
  assert.ok(
    inspection.failures.includes("P3 filter receipt fields are invalid"),
  );
  assert.ok(
    inspection.failures.includes("P3 filter receipt is not accepted-only"),
  );
  assert.ok(
    inspection.failures.includes("P3 filter receipt binary digest is invalid"),
  );
});

function recovery(state, filterOutcome, action) {
  return {
    schema: 1,
    operationId: "019b76d0-2000-7000-8000-000000000001",
    state,
    filterOutcome,
    action,
    assetCount: P3_FILTER_COUNT,
    sourceSha256Unchanged: true,
    externalFilterPreserved: true,
    externalSidecarPreserved: true,
  };
}

function aborted() {
  return {
    status: null,
    signal: "SIGABRT",
    error: null,
    stdout: "",
    stderr: "",
  };
}

function successful() {
  return { status: 0, signal: null, error: null, stdout: "", stderr: "" };
}
