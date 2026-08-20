import { buildP2LocalFaultGatesReport } from "./p2-local-fault-gates.mjs";
import { REQUIRED_P2_HOSTED_JOBS } from "./p2-hosted-evidence.mjs";
import { expectedPlatformPathTests } from "./platform-path-evidence.mjs";

export const TEST_P2_CANDIDATE_COMMIT = "d".repeat(40);
export const TEST_P2_LOCAL_COMMIT = "c".repeat(40);

export function makeP2ExternalReceiptFixture() {
  const soakCommit = "a".repeat(40);
  const matrixCommit = "b".repeat(40);
  const runId = "123456";
  const attempt = "1";
  const repository = "owner/repository";
  const workflowRef = `${repository}/.github/workflows/ci.yml@refs/heads/main`;
  const runUrl = `https://github.com/${repository}/actions/runs/${runId}`;
  const matrixVerifiedAt = "2026-08-20T02:30:00.000Z";
  const hostedRunVerifiedAt = "2026-08-20T02:45:00Z";
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
      startedAt: "2026-08-20T02:05:00Z",
      completedAt: "2026-08-20T02:40:00Z",
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
          backgroundLimit: 2,
          scan: makeWorkSnapshot(1_440),
          hash: makeWorkSnapshot(400_000),
          decode: makeWorkSnapshot(400_000),
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

function makeWorkSnapshot(completed) {
  return {
    active: 0,
    waiting: 0,
    peakActive: 1,
    peakWaiting: 0,
    completed,
    rejected: 0,
    timedOut: 0,
    cancelled: 0,
  };
}

export function makeP2LocalFaultReceiptFixture() {
  const transactionId = "01912345-6789-7abc-8def-0123456789ab";
  return buildP2LocalFaultGatesReport({
    gitCommit: TEST_P2_LOCAL_COMMIT,
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
