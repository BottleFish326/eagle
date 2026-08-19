import assert from "node:assert/strict";
import test from "node:test";

import {
  makeP2ExternalReceiptFixture,
  makeP2LocalFaultReceiptFixture,
  TEST_P2_CANDIDATE_COMMIT,
  TEST_P2_LOCAL_COMMIT,
} from "./p2-acceptance-test-fixtures.mjs";
import {
  buildPhase2ExitGatesReport,
  inspectPhase2ExitGatesReceipt,
} from "./phase-2-exit-gates.mjs";
import {
  buildP2DataSafetyAuditReport,
  P2_DATA_SAFETY_REPORTS,
} from "./p2-data-safety-audit.mjs";
import { FORMAL_SOAK_BASELINE_COMMIT } from "./soak-baseline-audit.mjs";

const candidateCommit = TEST_P2_CANDIDATE_COMMIT;
const localCommit = TEST_P2_LOCAL_COMMIT;
const externalReport = makeP2ExternalReceiptFixture();

test("accepts replayed external evidence and committed local fault evidence in order", () => {
  const fixture = acceptedInputs();
  const report = buildPhase2ExitGatesReport(fixture);
  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.equal(report.candidateCommit, candidateCommit);
  assert.equal(report.soakBaseline.verified, true);
  assert.equal(report.localCandidate.gitCommit, localCommit);
  assert.deepEqual(report.localCandidate.driftPaths, []);
  assert.equal(report.p2External.p2A11Commit, "a".repeat(40));
  assert.equal(report.p2LocalFaults.transaction.recoveredCount, 1_000);
  assert.deepEqual(report.p2LocalFaults.cacheFaultPoints, [
    "after-cache-rename",
    "after-cache-recreate",
  ]);
  assert.equal(report.p2DataSafety.openP0, 0);
  assert.equal(report.p2DataSafety.openP1, 0);
  const inspection = inspectPhase2ExitGatesReceipt(report);
  assert.equal(inspection.accepted, true, inspection.failures.join("; "));
});

test("rejects replay drift, local tampering, source drift, and missing commit bindings", () => {
  const fixture = acceptedInputs();
  fixture.externalReplay = structuredClone(fixture.externalReplay);
  fixture.externalReplay.p2A12.runUrl =
    "https://github.com/owner/repository/actions/runs/999999";
  fixture.localFaultReceipt = structuredClone(fixture.localFaultReceipt);
  fixture.localFaultReceipt.p2A04.recoveredCount = 999;
  fixture.workingTreeClean = false;
  fixture.commitOrderVerified = false;
  fixture.externalEvidenceInLocalCandidate = false;
  fixture.localEvidenceCommitted = false;
  fixture.dataSafetyReplay = structuredClone(fixture.dataSafetyReplay);
  fixture.dataSafetyReplay.defectRegister.counts.open.P1 = 1;
  fixture.dataSafetyEvidenceCommitted = false;
  fixture.soakBaselineAudit = structuredClone(fixture.soakBaselineAudit);
  fixture.soakBaselineAudit.productInputs.changedPaths = [
    "crates/preview/src/cache.rs",
  ];
  fixture.localCandidateDriftPaths = ["tools/transaction-fault/src/main.rs"];

  const report = buildPhase2ExitGatesReport(fixture);
  assert.equal(report.accepted, false);
  for (const expected of [
    "phase 2 exit candidate working tree is not clean",
    "stored external gate evidence does not equal its replay",
    "P2-A04/P2-A10: P2-A04 receipt recovery count is not 1000",
    "P2 commits are not ordered soak <= hosted matrix <= local faults <= data safety <= candidate",
    "local fault candidate does not contain the exact external gate evidence",
    "local fault evidence is not committed in the exit candidate",
    "stored data safety evidence does not equal its replay",
    "data safety evidence is not committed in the exit candidate",
    "formal soak baseline audit is not accepted for the candidate",
    "non-documentation paths changed after local fault execution: tools/transaction-fault/src/main.rs",
  ])
    assert.ok(report.failures.includes(expected), expected);
});

test("rejects malformed path and candidate inputs", () => {
  const fixture = acceptedInputs();
  fixture.candidateCommit = "main";
  fixture.localCandidateDriftPaths = ["../outside"];
  const report = buildPhase2ExitGatesReport(fixture);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.includes("phase 2 exit candidate commit is invalid"),
  );
  assert.ok(
    report.failures.includes(
      "local fault candidate drift contains an invalid repository path",
    ),
  );
});

test("offline final receipt inspection rejects structural and summary tampering", () => {
  const report = buildPhase2ExitGatesReport(acceptedInputs());
  report.unexpected = true;
  report.evidenceAt = "2026-08-20T04:00:00.000Z";
  report.localCandidate.gitCommit = "e".repeat(40);
  report.p2External.sha256 = "bad";
  report.p2LocalFaults.environment.nodeVersion = "v25.0.0";
  report.p2LocalFaults.cacheFaultPoints.reverse();
  report.p2DataSafety.openP1 = 1;
  const inspection = inspectPhase2ExitGatesReceipt(report);
  assert.equal(inspection.accepted, false);
  for (const expected of [
    "phase 2 exit receipt fields are invalid",
    "phase 2 exit receipt local fault commits do not match",
    "phase 2 exit receipt evidence time is not deterministic",
    "phase 2 external receipt digest is invalid",
    "phase 2 local fault environment Node.js is not 24.x",
    "phase 2 local fault points are invalid",
    "phase 2 data safety receipt has open P0/P1 defects",
  ])
    assert.ok(inspection.failures.includes(expected), expected);
});

function acceptedInputs() {
  const localFaultReceipt = makeP2LocalFaultReceiptFixture();
  const externalReplay = structuredClone(externalReport);
  const dataSafetyReceipt = makeDataSafetyReceipt(
    externalReplay,
    localFaultReceipt,
  );
  return {
    externalBytes: Buffer.from(`${JSON.stringify(externalReport)}\n`),
    externalReport: structuredClone(externalReport),
    externalReplay,
    localFaultBytes: Buffer.from(`${JSON.stringify(localFaultReceipt)}\n`),
    localFaultReceipt,
    dataSafetyBytes: Buffer.from(`${JSON.stringify(dataSafetyReceipt)}\n`),
    dataSafetyReceipt: structuredClone(dataSafetyReceipt),
    dataSafetyReplay: structuredClone(dataSafetyReceipt),
    candidateCommit,
    workingTreeClean: true,
    commitOrderVerified: true,
    externalEvidenceInLocalCandidate: true,
    localEvidenceCommitted: true,
    dataSafetyEvidenceCommitted: true,
    soakBaselineAudit: {
      schema: 1,
      accepted: true,
      failures: [],
      baselineCommit: FORMAL_SOAK_BASELINE_COMMIT,
      currentCommit: candidateCommit,
      descendantOfBaseline: true,
      loadedInputs: { changedPaths: [] },
      productInputs: { changedPaths: [] },
    },
    localCandidateDriftPaths: [],
  };
}

function makeDataSafetyReceipt(externalReceipt, localFaultReceipt) {
  const defectRegister = {
    schema: 1,
    scope: "phase-2-exit",
    status: "reviewed",
    reviewedAt: "2026-08-20T03:30:00.000Z",
    findings: [],
  };
  const report = buildP2DataSafetyAuditReport({
    candidateCommit,
    candidateCommittedAt: "2026-08-20T04:00:00.000Z",
    repositoryClean: true,
    commitOrderVerified: true,
    inputsCommitted: true,
    defectRegister,
    defectRegisterBytes: Buffer.from(JSON.stringify(defectRegister)),
    externalReceipt,
    externalBytes: Buffer.from(JSON.stringify(externalReceipt)),
    localFaultReceipt,
    localFaultBytes: Buffer.from(JSON.stringify(localFaultReceipt)),
    reportFiles: P2_DATA_SAFETY_REPORTS.map((fileName) => ({
      fileName,
      bytes: Buffer.from(fileName),
    })),
  });
  assert.equal(report.accepted, true, report.failures.join("; "));
  return report;
}
