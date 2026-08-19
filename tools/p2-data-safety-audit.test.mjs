import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  makeP2ExternalReceiptFixture,
  makeP2LocalFaultReceiptFixture,
  TEST_P2_CANDIDATE_COMMIT,
} from "./p2-acceptance-test-fixtures.mjs";
import {
  buildP2DataSafetyAuditReport,
  inspectDefectRegister,
  inspectP2DataSafetyAuditReceipt,
  P2_DATA_SAFETY_REPORTS,
} from "./p2-data-safety-audit.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const receiptInspector = path.join(
  repository,
  "tools",
  "inspect-p2-data-safety-audit.mjs",
);

test("accepts a reviewed zero-P0/P1 register and all committed control evidence", () => {
  const report = buildP2DataSafetyAuditReport(acceptedInputs());
  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.equal(report.candidateCommit, TEST_P2_CANDIDATE_COMMIT);
  assert.equal(report.defectRegister.counts.open.P0, 0);
  assert.equal(report.defectRegister.counts.open.P1, 0);
  assert.equal(report.reports.length, P2_DATA_SAFETY_REPORTS.length);
  assert.equal(report.controls.length, 10);
  assert.deepEqual(inspectP2DataSafetyAuditReceipt(report), {
    accepted: true,
    failures: [],
  });
});

test("rejects draft, open high-priority, duplicate, and unplanned P2 findings", () => {
  const draft = reviewedRegister();
  draft.status = "draft";
  draft.reviewedAt = null;
  assert.equal(
    inspectDefectRegister(draft, { requireReviewed: true }).accepted,
    false,
  );

  const findings = [
    finding("DEF-0001", "P1", "open"),
    finding("DEF-0001", "P3", "resolved"),
    finding("DEF-0002", "P2", "open"),
  ];
  findings[2].workaround = null;
  findings[2].targetVersion = null;
  const register = reviewedRegister(findings);
  const inspection = inspectDefectRegister(register, {
    requireReviewed: true,
    minimumReviewedAt: "2026-08-20T03:00:00.000Z",
  });
  assert.equal(inspection.accepted, false);
  for (const expected of [
    "finding[1] ID is duplicated",
    "finding[2] open P2 lacks a workaround or target version",
    "one or more P0/P1 defects remain open",
  ])
    assert.ok(inspection.failures.includes(expected), expected);

  const future = inspectDefectRegister(reviewedRegister(), {
    requireReviewed: true,
    maximumReviewedAt: "2026-08-20T03:29:59.000Z",
  });
  assert.equal(future.accepted, false);
  assert.ok(future.failures.includes("review postdates the candidate commit"));
});

test("rejects stale review, missing reports, uncommitted inputs, and receipt tampering", () => {
  const input = acceptedInputs();
  input.defectRegister.reviewedAt = "2026-08-20T02:59:59.000Z";
  input.defectRegisterBytes = bytes(input.defectRegister);
  input.reportFiles.pop();
  input.inputsCommitted = false;
  const rejected = buildP2DataSafetyAuditReport(input);
  assert.equal(rejected.accepted, false);
  assert.ok(
    rejected.failures.includes(
      "defect register: review predates the final gate evidence",
    ),
  );
  assert.ok(
    rejected.failures.includes(
      "data safety inputs are not committed in the candidate",
    ),
  );
  assert.ok(
    rejected.failures.some((failure) =>
      failure.startsWith("data safety report is missing:"),
    ),
  );

  const receipt = buildP2DataSafetyAuditReport(acceptedInputs());
  receipt.unexpected = true;
  receipt.evidenceAt = "2026-08-20T02:00:00.000Z";
  receipt.defectRegister.counts.open.P0 = 1;
  receipt.reports[0].sha256 = "bad";
  receipt.controls.reverse();
  const inspection = inspectP2DataSafetyAuditReceipt(receipt);
  assert.equal(inspection.accepted, false);
  for (const expected of [
    "data safety audit receipt fields are invalid",
    "data safety defect register counts are invalid",
    "data safety report receipt digest is invalid: docs/reports/phase-1-acceptance.md",
    "data safety audit controls are invalid",
    "data safety audit evidence time is not deterministic",
  ])
    assert.ok(inspection.failures.includes(expected), expected);
});

test("offline data safety CLI accepts the receipt and rejects tampering", async (context) => {
  const directory = await mkdtemp(
    path.join(os.tmpdir(), "material-eagle-data-safety-test-"),
  );
  context.after(() => rm(directory, { recursive: true, force: true }));
  const report = buildP2DataSafetyAuditReport(acceptedInputs());
  const acceptedPath = path.join(directory, "accepted.json");
  await writeFile(acceptedPath, bytes(report));
  const accepted = runInspector(acceptedPath);
  assert.equal(accepted.status, 0, accepted.stderr || accepted.stdout);
  assert.deepEqual(JSON.parse(accepted.stdout), {
    accepted: true,
    failures: [],
  });

  report.defectRegister.findings.push(finding("DEF-0099", "P1", "open"));
  const rejectedPath = path.join(directory, "rejected.json");
  await writeFile(rejectedPath, bytes(report));
  const rejected = runInspector(rejectedPath);
  assert.equal(rejected.status, 1);
  assert.equal(JSON.parse(rejected.stdout).accepted, false);
});

function acceptedInputs() {
  const defectRegister = reviewedRegister();
  const externalReceipt = makeP2ExternalReceiptFixture();
  const localFaultReceipt = makeP2LocalFaultReceiptFixture();
  return {
    candidateCommit: TEST_P2_CANDIDATE_COMMIT,
    candidateCommittedAt: "2026-08-20T04:00:00.000Z",
    repositoryClean: true,
    commitOrderVerified: true,
    inputsCommitted: true,
    defectRegister,
    defectRegisterBytes: bytes(defectRegister),
    externalReceipt,
    externalBytes: bytes(externalReceipt),
    localFaultReceipt,
    localFaultBytes: bytes(localFaultReceipt),
    reportFiles: P2_DATA_SAFETY_REPORTS.map((fileName) => ({
      fileName,
      bytes: Buffer.from(`# ${fileName}\n`),
    })),
  };
}

function reviewedRegister(findings = []) {
  return {
    schema: 1,
    scope: "phase-2-exit",
    status: "reviewed",
    reviewedAt: "2026-08-20T03:30:00.000Z",
    findings,
  };
}

function finding(id, severity, status) {
  return {
    id,
    severity,
    status,
    summary: `${severity} fixture`,
    evidence: ["automated reproduction"],
    workaround: status === "open" ? "avoid the affected operation" : null,
    targetVersion: status === "open" ? "0.2.0" : null,
    resolvedAt: status === "resolved" ? "2026-08-20T03:10:00.000Z" : null,
  };
}

function bytes(value) {
  return Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
}

function runInspector(receiptPath) {
  return spawnSync(process.execPath, [receiptInspector, receiptPath], {
    cwd: repository,
    encoding: "utf8",
  });
}
