import assert from "node:assert/strict";
import test from "node:test";

import { buildP2ExitStatus } from "./p2-exit-status.mjs";

test("keeps a healthy partial soak isolated as the only next action", () => {
  const input = base();
  input.soak = { state: "running", failures: [], summary: { elapsedMs: 123 } };
  input.hostedReadiness = {
    ready: false,
    failures: ["GitHub CLI is not installed"],
    commands: [],
  };
  input.externalGates = { state: "missing", failures: [] };
  const report = buildP2ExitStatus(input);
  assert.equal(report.stage, "soak-running");
  assert.equal(report.readyToAdvanceStage3, false);
  assert.equal(report.nextAction.kind, "wait-for-soak");
  assert.deepEqual(report.failures, []);
});

test("reports a failed soak before any hosted work", () => {
  const input = base();
  input.soak = { state: "failed", failures: ["cache exceeded"] };
  const report = buildP2ExitStatus(input);
  assert.equal(report.stage, "soak-failed");
  assert.deepEqual(report.failures, ["P2-A11: cache exceeded"]);
  assert.equal(report.nextAction.command, null);
});

test("distinguishes hosted environment preparation from a runnable hosted job", () => {
  const blocked = base();
  blocked.externalGates = { state: "missing", failures: [] };
  blocked.hostedReadiness = {
    ready: false,
    failures: ["origin/main commit is unavailable"],
    commands: [],
    remediations: [
      {
        kind: "publish-main",
        command: "git push --set-upstream origin main",
        message: "Publish the exact candidate.",
      },
    ],
  };
  const blockedReport = buildP2ExitStatus(blocked);
  assert.equal(blockedReport.stage, "hosted-environment-blocked");
  assert.equal(
    blockedReport.nextAction.command,
    "git push --set-upstream origin main",
  );
  assert.equal(blockedReport.nextAction.kind, "publish-main");

  const ready = base();
  ready.externalGates = { state: "missing", failures: [] };
  ready.hostedReadiness = {
    ready: true,
    failures: [],
    commands: ["gh workflow run ci.yml --ref main"],
    remediations: [],
  };
  const readyReport = buildP2ExitStatus(ready);
  assert.equal(readyReport.stage, "hosted-run-pending");
  assert.equal(
    readyReport.nextAction.command,
    "gh workflow run ci.yml --ref main",
  );
});

test("routes complete hosted inputs through external verification", () => {
  const input = base();
  input.externalGates = { state: "ready", failures: [] };
  const report = buildP2ExitStatus(input);
  assert.equal(report.stage, "external-gates-pending");
  assert.equal(report.nextAction.command, "npm run verify:p2-external");

  const dirty = structuredClone(input);
  dirty.git.cleanTracked = false;
  dirty.git.cleanAll = false;
  const dirtyReport = buildP2ExitStatus(dirty);
  assert.equal(dirtyReport.stage, "candidate-dirty");
  assert.equal(dirtyReport.nextAction.kind, "commit-external-inputs");
  assert.equal(dirtyReport.nextAction.command, "git status --short");
});

test("routes accepted external evidence through local faults and a clean final candidate", () => {
  const missing = base();
  missing.localFaults = { state: "missing", failures: [], committed: false };
  assert.equal(buildP2ExitStatus(missing).stage, "local-faults-pending");

  const uncommitted = base();
  uncommitted.localFaults.committed = false;
  assert.equal(
    buildP2ExitStatus(uncommitted).stage,
    "local-faults-uncommitted",
  );

  const safetyMissing = base();
  safetyMissing.dataSafety = {
    state: "missing",
    failures: [],
    committed: false,
  };
  const safetyMissingReport = buildP2ExitStatus(safetyMissing);
  assert.equal(safetyMissingReport.stage, "data-safety-pending");
  assert.equal(
    safetyMissingReport.nextAction.command,
    "npm run verify:p2-data-safety",
  );

  const reviewDraft = structuredClone(safetyMissing);
  reviewDraft.dataSafetyReadiness = {
    ready: false,
    registerState: "draft",
    registerCommitted: true,
    reportsCommitted: true,
    candidateClean: true,
    failures: ["status is not reviewed"],
  };
  const reviewDraftReport = buildP2ExitStatus(reviewDraft);
  assert.equal(reviewDraftReport.stage, "data-safety-review-pending");
  assert.equal(reviewDraftReport.nextAction.kind, "review-defect-register");
  assert.equal(reviewDraftReport.nextAction.command, null);

  const reviewUncommitted = structuredClone(safetyMissing);
  reviewUncommitted.dataSafetyReadiness = {
    ready: false,
    registerState: "reviewed",
    registerCommitted: false,
    reportsCommitted: true,
    candidateClean: false,
    failures: ["register is not committed"],
  };
  const reviewUncommittedReport = buildP2ExitStatus(reviewUncommitted);
  assert.equal(
    reviewUncommittedReport.nextAction.kind,
    "commit-data-safety-inputs",
  );
  assert.equal(
    reviewUncommittedReport.nextAction.command,
    "git status --short",
  );

  const reviewBlocked = structuredClone(safetyMissing);
  reviewBlocked.dataSafetyReadiness = {
    ready: false,
    registerState: "reviewed",
    registerCommitted: true,
    reportsCommitted: true,
    candidateClean: true,
    failures: ["one or more P0/P1 defects remain open"],
  };
  assert.equal(
    buildP2ExitStatus(reviewBlocked).nextAction.kind,
    "resolve-data-safety-findings",
  );

  const safetyInvalid = base();
  safetyInvalid.dataSafety = {
    state: "invalid",
    failures: ["open P1"],
    committed: false,
  };
  assert.equal(buildP2ExitStatus(safetyInvalid).stage, "data-safety-invalid");

  const safetyUncommitted = base();
  safetyUncommitted.dataSafety.committed = false;
  assert.equal(
    buildP2ExitStatus(safetyUncommitted).stage,
    "data-safety-uncommitted",
  );

  const dirty = base();
  dirty.git.cleanAll = false;
  assert.equal(buildP2ExitStatus(dirty).stage, "candidate-dirty");

  const finalPending = buildP2ExitStatus(base());
  assert.equal(finalPending.stage, "final-exit-pending");
  assert.equal(finalPending.nextAction.command, "npm run verify:p2-exit");
});

test("only a committed final receipt with accepted upstream gates advances stage 3", () => {
  const accepted = base();
  accepted.finalExit = { state: "accepted", failures: [], committed: true };
  const acceptedReport = buildP2ExitStatus(accepted);
  assert.equal(acceptedReport.stage, "accepted");
  assert.equal(acceptedReport.readyToAdvanceStage3, true);
  assert.equal(acceptedReport.nextAction, null);

  const uncommitted = base();
  uncommitted.finalExit = {
    state: "accepted",
    failures: [],
    committed: false,
  };
  assert.equal(buildP2ExitStatus(uncommitted).stage, "final-exit-uncommitted");

  const conflict = base();
  conflict.finalExit = { state: "accepted", failures: [], committed: true };
  conflict.externalGates = { state: "missing", failures: [] };
  assert.equal(buildP2ExitStatus(conflict).stage, "evidence-conflict");
});

test("surfaces invalid immutable receipts instead of suggesting overwrite", () => {
  const local = base();
  local.localFaults = {
    state: "invalid",
    failures: ["binary digest is invalid"],
    committed: false,
  };
  const localReport = buildP2ExitStatus(local);
  assert.equal(localReport.stage, "local-faults-invalid");
  assert.equal(localReport.nextAction.command, null);

  const final = base();
  final.finalExit = {
    state: "invalid",
    failures: ["summary was tampered"],
    committed: true,
  };
  const finalReport = buildP2ExitStatus(final);
  assert.equal(finalReport.stage, "final-exit-invalid");
  assert.equal(finalReport.nextAction.command, "npm run inspect:p2-exit");
});

function base() {
  return {
    git: {
      currentCommit: "a".repeat(40),
      cleanTracked: true,
      cleanAll: true,
    },
    soak: { state: "passed", failures: [], summary: null },
    hostedReadiness: {
      ready: false,
      failures: [],
      commands: [],
      remediations: [],
    },
    externalGates: { state: "accepted", failures: [], summary: null },
    localFaults: {
      state: "accepted",
      failures: [],
      committed: true,
      summary: null,
    },
    dataSafety: {
      state: "accepted",
      failures: [],
      committed: true,
      summary: null,
    },
    dataSafetyReadiness: {
      ready: true,
      registerState: "reviewed",
      registerCommitted: true,
      reportsCommitted: true,
      candidateClean: true,
      failures: [],
      summary: null,
    },
    finalExit: {
      state: "missing",
      failures: [],
      committed: false,
      summary: null,
    },
  };
}
