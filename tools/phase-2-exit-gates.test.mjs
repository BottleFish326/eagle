import assert from "node:assert/strict";
import test from "node:test";

import { buildP2LocalFaultGatesReport } from "./p2-local-fault-gates.mjs";
import { REQUIRED_P2_HOSTED_JOBS } from "./p2-hosted-evidence.mjs";
import { expectedPlatformPathTests } from "./platform-path-evidence.mjs";
import {
  buildPhase2ExitGatesReport,
  inspectPhase2ExitGatesReceipt,
} from "./phase-2-exit-gates.mjs";
import { FORMAL_SOAK_BASELINE_COMMIT } from "./soak-baseline-audit.mjs";

const candidateCommit = "d".repeat(40);
const localCommit = "c".repeat(40);
const externalReport = makeExternalReport();

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
    "P2 commits are not ordered soak <= hosted matrix <= local faults <= candidate",
    "local fault candidate does not contain the exact external gate evidence",
    "local fault evidence is not committed in the exit candidate",
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
  const inspection = inspectPhase2ExitGatesReceipt(report);
  assert.equal(inspection.accepted, false);
  for (const expected of [
    "phase 2 exit receipt fields are invalid",
    "phase 2 exit receipt local fault commits do not match",
    "phase 2 exit receipt evidence time is not deterministic",
    "phase 2 external receipt digest is invalid",
    "phase 2 local fault environment Node.js is not 24.x",
    "phase 2 local fault points are invalid",
  ])
    assert.ok(inspection.failures.includes(expected), expected);
});

function acceptedInputs() {
  const localFaultReceipt = makeLocalFaultReceipt();
  const externalReplay = structuredClone(externalReport);
  return {
    externalBytes: Buffer.from(`${JSON.stringify(externalReport)}\n`),
    externalReport: structuredClone(externalReport),
    externalReplay,
    localFaultBytes: Buffer.from(`${JSON.stringify(localFaultReceipt)}\n`),
    localFaultReceipt,
    candidateCommit,
    workingTreeClean: true,
    commitOrderVerified: true,
    externalEvidenceInLocalCandidate: true,
    localEvidenceCommitted: true,
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

function makeExternalReport() {
  const soakCommit = "a".repeat(40);
  const matrixCommit = "b".repeat(40);
  const runId = "123456";
  const attempt = "1";
  const repository = "owner/repository";
  const workflowRef = `${repository}/.github/workflows/ci.yml@refs/heads/main`;
  const runUrl = `https://github.com/${repository}/actions/runs/${runId}`;
  const matrixVerifiedAt = "2026-08-20T02:30:00.000Z";
  const hostedRunVerifiedAt = "2026-08-20T02:45:00.000Z";
  const verificationEnvironment = {
    nodeVersion: "v24.19.0",
    githubActions: "true",
    githubSha: matrixCommit,
    githubRunId: runId,
    githubRunAttempt: attempt,
    githubWorkflowRef: workflowRef,
    githubRepository: repository,
    githubServerUrl: "https://github.com",
    runnerOs: "Linux",
    runnerArch: "X64",
    runnerEnvironment: "github-hosted",
  };
  const artifacts = ["darwin", "linux", "win32"].map((platform) => {
    const runnerOs = {
      darwin: "macOS",
      linux: "Linux",
      win32: "Windows",
    }[platform];
    const tests = expectedPlatformPathTests(platform);
    return {
      artifactName: `p2-a12-source-${runnerOs}-${matrixCommit}-attempt-${attempt}`,
      fileName: "p2-a12-platform-paths.json",
      sha256:
        platform === "darwin"
          ? "3".repeat(64)
          : platform === "linux"
            ? "4".repeat(64)
            : "5".repeat(64),
      platform,
      accepted: true,
      startedAt: "2026-08-20T02:10:00.000Z",
      completedAt: "2026-08-20T02:20:00.000Z",
      environment: {
        architecture: "x64",
        nodeVersion: "v24.19.0",
        rustc: "rustc 1.89.0",
        cargo: "cargo 1.89.0",
        runnerOs,
        runnerArch: "X64",
        runnerEnvironment: "github-hosted",
        githubRunId: runId,
        githubRunAttempt: attempt,
        githubWorkflowRef: workflowRef,
        githubRepository: repository,
        githubServerUrl: "https://github.com",
      },
      expectedTests: tests,
      listedTests: tests,
      executedTests: tests,
      summary: {
        result: "ok",
        passed: tests.length,
        failed: 0,
        ignored: 0,
        measured: 0,
        filteredOut: 100,
      },
    };
  });
  const hostedJobs = REQUIRED_P2_HOSTED_JOBS.map((name, index) => {
    const databaseId = 900_000 + index;
    return {
      databaseId,
      name,
      status: "completed",
      conclusion: "success",
      startedAt: "2026-08-20T02:05:00.000Z",
      completedAt: "2026-08-20T02:40:00.000Z",
      url: `${runUrl}/job/${String(databaseId)}`,
    };
  });
  return {
    schema: 1,
    accepted: true,
    failures: [],
    evidenceAt: hostedRunVerifiedAt,
    commitOrderVerified: true,
    p2A11: {
      accepted: true,
      fileName: "p2-06-resource-soak.json",
      sha256: "1".repeat(64),
      gitCommit: soakCommit,
      startedAt: "2026-08-19T18:00:00.000Z",
      completedAt: "2026-08-20T02:00:00.000Z",
      durationSeconds: 28_800,
      fixtureCount: 100_000,
      environment: {
        platform: "darwin",
        architecture: "arm64",
        nodeVersion: "v24.19.0",
      },
      summary: {
        nativeSampleCount: 5_749,
        minimumNativeSampleCount: 4_311,
        internalSampleCount: 5_761,
        minimumInternalSampleCount: 4_320,
        invalidInternalSampleCount: 0,
        invalidExternalSampleCount: 0,
        rssGrowthKiB: 1_024,
        rssSlopeKiBPerMinute: 2,
        maxRssKiB: 200_000,
        handleGrowth: 2,
        maxHandles: 12,
        threadBaseline: 4,
        maxThreads: 5,
        maxCpuPercent: 25,
        scanPasses: 1_440,
        generatedEvents: 40_000,
        watcherBatches: 20_000,
        thumbnailRequests: 400_000,
        hashRequests: 400_000,
        cacheEntries: 20_000,
        scheduler: {
          mode: "foreground",
          foregroundLimit: 4,
          maxWaiters: 256,
          activeTotal: 1,
          waitingTotal: 0,
          peakActiveTotal: 2,
          peakWaitingTotal: 0,
        },
      },
    },
    p2A12: {
      accepted: true,
      matrixArtifactName: `p2-a12-matrix-${matrixCommit}-attempt-${attempt}`,
      matrixSha256: "6".repeat(64),
      hostedRunReceiptSha256: "7".repeat(64),
      hostedRunVerifiedAt,
      gitCommit: matrixCommit,
      verifiedAt: matrixVerifiedAt,
      githubRunAttempt: attempt,
      runUrl,
      verificationEnvironment,
      artifacts,
      hostedJobs,
    },
  };
}

function makeLocalFaultReceipt() {
  const transactionId = "01912345-6789-7abc-8def-0123456789ab";
  const report = buildP2LocalFaultGatesReport({
    gitCommit: localCommit,
    executedAt: "2026-08-20T03:00:00.000Z",
    environment: {
      platform: "darwin",
      architecture: "arm64",
      nodeVersion: "v24.19.0",
      rustc: "rustc 1.89.0",
      cargo: "cargo 1.89.0",
    },
    binarySha256: {
      transactionFault: "1".repeat(64),
      cacheFault: "2".repeat(64),
    },
    repositoryClean: true,
    transaction: {
      abort: aborted(),
      recover: succeeded(
        `discovered ${transactionId} Active applied=317\nrecovered ${transactionId} 1000\n`,
      ),
    },
    cacheCases: ["after-cache-rename", "after-cache-recreate"].map(
      (faultPoint) => ({
        faultPoint,
        seed: succeeded("seeded cache=1 asset=true sidecar=true\n"),
        abort: aborted(),
        recover: succeeded(
          "recovered disposition=maintained cache=1 asset=true sidecar=true\n",
        ),
      }),
    ),
    temporaryWorkspacesRemoved: true,
  });
  assert.equal(report.accepted, true, report.failures.join("; "));
  return report;
}

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
