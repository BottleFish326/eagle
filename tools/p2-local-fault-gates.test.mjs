import assert from "node:assert/strict";
import test from "node:test";

import {
  buildP2LocalFaultGatesReport,
  inspectP2LocalFaultGatesReceipt,
  P2_CACHE_FAULT_POINTS,
} from "./p2-local-fault-gates.mjs";

const base = {
  gitCommit: "a".repeat(40),
  executedAt: "2026-08-20T01:00:00.000Z",
  environment: {
    platform: "darwin",
    architecture: "arm64",
    nodeVersion: "v24.0.0",
    rustc: "rustc 1.89.0",
    cargo: "cargo 1.89.0",
  },
  binarySha256: {
    transactionFault: "b".repeat(64),
    cacheFault: "c".repeat(64),
  },
  repositoryClean: true,
  transaction: {
    abort: aborted(),
    recover: succeeded(
      "discovered 01912345-6789-7abc-8def-0123456789ab Active applied=317\n" +
        "recovered 01912345-6789-7abc-8def-0123456789ab 1000\n",
    ),
  },
  cacheCases: P2_CACHE_FAULT_POINTS.map((faultPoint) => ({
    faultPoint,
    seed: succeeded("seeded cache=1 asset=true sidecar=true\n"),
    abort: aborted(),
    recover: succeeded(
      "recovered disposition=maintained cache=1 asset=true sidecar=true\n",
    ),
  })),
  temporaryWorkspacesRemoved: true,
};

test("accepts the 317 -> 1000 transaction crash and both cache crash boundaries", () => {
  const report = buildP2LocalFaultGatesReport(structuredClone(base));
  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.equal(report.p2A04.appliedAtDiscovery, 317);
  assert.equal(report.p2A04.recoveredCount, 1_000);
  assert.equal(report.p2A10.cases.length, 2);
  assert.ok(report.p2A10.cases.every((value) => value.recoveryVerified));
});

test("offline receipt inspection accepts the exact generated report", () => {
  const report = buildP2LocalFaultGatesReport(structuredClone(base));
  const inspection = inspectP2LocalFaultGatesReceipt(report);
  assert.equal(inspection.accepted, true, inspection.failures.join("; "));
});

test("rejects a normal exit, wrong transaction count, and incomplete cache recovery", () => {
  const input = structuredClone(base);
  input.transaction.abort = succeeded("");
  input.transaction.recover.stdout = input.transaction.recover.stdout.replace(
    "1000",
    "999",
  );
  input.cacheCases[0].recover.stdout = "recovered cache=1\n";
  const report = buildP2LocalFaultGatesReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.includes(
      "P2-A04 transaction process did not abort as required",
    ),
  );
  assert.ok(
    report.failures.includes(
      "P2-A04 recovery output does not prove 317 -> 1000",
    ),
  );
  assert.ok(
    report.failures.includes(
      "P2-A10 recovery output is incomplete: after-cache-rename",
    ),
  );
});

test("rejects missing cases, invalid provenance, and retained workspaces", () => {
  const input = structuredClone(base);
  input.gitCommit = "main";
  input.environment.nodeVersion = "v25.0.0";
  input.binarySha256.cacheFault = "bad";
  input.repositoryClean = false;
  input.cacheCases.pop();
  input.temporaryWorkspacesRemoved = false;
  const report = buildP2LocalFaultGatesReport(input);
  assert.equal(report.accepted, false);
  assert.ok(report.failures.includes("local fault gate commit is invalid"));
  assert.ok(report.failures.includes("local fault gates require Node.js 24.x"));
  assert.ok(
    report.failures.includes(
      "P2-A10 requires exactly one cache case: after-cache-recreate",
    ),
  );
  assert.ok(
    report.failures.includes(
      "local fault gate temporary workspaces were not removed",
    ),
  );
  assert.ok(
    report.failures.includes("local fault gate repository was not clean"),
  );
});

test("rejects a recovery receipt whose transaction identifier is not UUIDv7", () => {
  const input = structuredClone(base);
  input.transaction.recover.stdout =
    input.transaction.recover.stdout.replaceAll(
      "01912345-6789-7abc-8def-0123456789ab",
      "01912345-6789-4abc-8def-0123456789ab",
    );
  const report = buildP2LocalFaultGatesReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.includes(
      "P2-A04 recovery output does not prove 317 -> 1000",
    ),
  );
});

test("rejects contradictory abort state and extra cache cases", () => {
  const input = structuredClone(base);
  input.transaction.abort.status = 0;
  input.cacheCases.push({
    faultPoint: "unknown",
    seed: succeeded("seeded cache=1 asset=true sidecar=true\n"),
    abort: aborted(),
    recover: succeeded(
      "recovered disposition=maintained cache=1 asset=true sidecar=true\n",
    ),
  });
  const report = buildP2LocalFaultGatesReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.includes(
      "P2-A04 transaction process did not abort as required",
    ),
  );
  assert.ok(
    report.failures.includes("P2-A10 requires exactly two cache fault cases"),
  );
});

test("offline receipt inspection rejects field, state, digest, and order tampering", () => {
  const report = buildP2LocalFaultGatesReport(structuredClone(base));
  report.unexpected = true;
  report.build.binaries.cacheFault = "bad";
  report.p2A04.abort = {
    ...report.p2A04.abort,
    status: 0,
    signal: null,
  };
  report.p2A10.cases.reverse();
  const inspection = inspectP2LocalFaultGatesReceipt(report);
  assert.equal(inspection.accepted, false);
  assert.ok(
    inspection.failures.includes("local fault receipt fields are invalid"),
  );
  assert.ok(
    inspection.failures.includes(
      "local fault receipt binary digest is invalid: cacheFault",
    ),
  );
  assert.ok(
    inspection.failures.includes("P2-A04 abort process state is invalid"),
  );
  assert.equal(
    inspection.failures.filter((failure) =>
      failure.includes("receipt fault point is out of order"),
    ).length,
    2,
  );
});

function succeeded(stdout) {
  return { status: 0, signal: null, error: null, stdout, stderr: "" };
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
