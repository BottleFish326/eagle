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
  };
  const blockedReport = buildP2ExitStatus(blocked);
  assert.equal(blockedReport.stage, "hosted-environment-blocked");
  assert.equal(
    blockedReport.nextAction.command,
    "npm run audit:p2-hosted-readiness",
  );

  const ready = base();
  ready.externalGates = { state: "missing", failures: [] };
  ready.hostedReadiness = {
    ready: true,
    failures: [],
    commands: ["gh workflow run ci.yml --ref main"],
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
  assert.equal(
    report.nextAction.command,
    "node tools/verify-phase-2-external-gates.mjs",
  );
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
    hostedReadiness: { ready: false, failures: [], commands: [] },
    externalGates: { state: "accepted", failures: [], summary: null },
    localFaults: {
      state: "accepted",
      failures: [],
      committed: true,
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
