import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import {
  buildPhase2ExternalGatesReport,
  inspectPhase2ExternalGatesReceipt,
} from "./phase-2-external-gates.mjs";
import {
  buildP2HostedRunReceipt,
  inspectP2HostedRun,
  REQUIRED_P2_HOSTED_JOBS,
} from "./p2-hosted-evidence.mjs";
import { buildPlatformMatrixReport } from "./platform-matrix-analysis.mjs";
import { platformMatrixBundleEntries } from "./platform-matrix-bundle.mjs";
import { expectedPlatformPathTests } from "./platform-path-evidence.mjs";
import { buildResourceStabilityReport } from "./resource-stability-analysis.mjs";
import { FORMAL_RESOURCE_STABILITY_OPTIONS } from "./resource-stability-report.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const verifier = path.join(
  repository,
  "tools",
  "verify-phase-2-external-gates.mjs",
);
const inspector = path.join(
  repository,
  "tools",
  "inspect-phase-2-external-gates.mjs",
);
const workflowRef = "owner/repository/.github/workflows/ci.yml@refs/heads/main";

test("accepts and writes one deterministic verdict after replaying both external gates", async (context) => {
  const fixture = await createFixture();
  context.after(() => rm(fixture.root, { recursive: true, force: true }));

  const result = runVerifier(fixture);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const report = JSON.parse(await readFile(fixture.output, "utf8"));
  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.equal(report.commitOrderVerified, true);
  assert.equal(report.p2A11.durationSeconds, 28_800);
  assert.equal(report.p2A11.fixtureCount, 100_000);
  assert.equal(report.p2A12.runUrl, fixture.matrix.workflow.runUrl);
  assert.equal(report.p2A12.artifacts.length, 3);
  assert.equal(report.p2A12.hostedJobs.length, 5);
  assert.equal(
    report.p2A12.hostedRunReceiptSha256,
    sha256(fixture.hostedRunBytes),
  );
  assert.deepEqual(inspectPhase2ExternalGatesReceipt(report), {
    accepted: true,
    failures: [],
  });
  const inspected = runInspector(fixture.output);
  assert.equal(inspected.status, 0, inspected.stderr || inspected.stdout);
  assert.deepEqual(JSON.parse(inspected.stdout), {
    accepted: true,
    failures: [],
  });

  const second = runVerifier(fixture);
  assert.equal(second.status, 0, second.stderr || second.stdout);
  assert.deepEqual(JSON.parse(second.stdout), report);
});

test("offline external receipt inspection rejects structural and semantic tampering", async (context) => {
  const fixture = await createFixture();
  context.after(() => rm(fixture.root, { recursive: true, force: true }));
  const result = runVerifier(fixture);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const report = JSON.parse(await readFile(fixture.output, "utf8"));
  const mutations = [
    (value) => {
      value.unexpected = true;
    },
    (value) => {
      value.evidenceAt = value.p2A11.completedAt;
    },
    (value) => {
      value.p2A11.summary.minimumInternalSampleCount += 1;
    },
    (value) => {
      value.p2A12.artifacts[1].executedTests.pop();
    },
    (value) => {
      value.p2A12.verificationEnvironment.githubSha = "b".repeat(40);
    },
    (value) => {
      value.p2A12.hostedJobs[0].conclusion = "failure";
    },
  ];
  for (const mutate of mutations) {
    const changed = structuredClone(report);
    mutate(changed);
    const inspection = inspectPhase2ExternalGatesReceipt(changed);
    assert.equal(inspection.accepted, false);
    assert.ok(inspection.failures.length > 0);
  }

  const tampered = structuredClone(report);
  tampered.p2A12.hostedJobs[0].url = "https://example.invalid/job/1";
  const tamperedPath = path.join(fixture.root, "tampered.json");
  await writeFile(tamperedPath, `${JSON.stringify(tampered, null, 2)}\n`);
  const inspected = runInspector(tamperedPath);
  assert.equal(inspected.status, 1);
  assert.equal(JSON.parse(inspected.stdout).accepted, false);
});

test("rejects a changed soak summary and an unverified commit order", async (context) => {
  const fixture = await createFixture();
  context.after(() => rm(fixture.root, { recursive: true, force: true }));
  const changed = structuredClone(fixture.resourceReport);
  changed.summary.scanPasses += 1;
  const changedBytes = Buffer.from(`${JSON.stringify(changed, null, 2)}\n`);

  const report = buildPhase2ExternalGatesReport({
    resourceBytes: changedBytes,
    resourceReport: changed,
    platformBundle: fixture.platformBundle,
    hostedRunBytes: fixture.hostedRunBytes,
    hostedRunReceipt: fixture.hostedRunReceipt,
    commitOrderVerified: false,
  });
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("does not equal its raw-sample replay"),
    ),
  );

  const changedHostedRun = structuredClone(fixture.hostedRunReceipt);
  changedHostedRun.jobs.at(-1).conclusion = "failure";
  const hostedRejected = buildPhase2ExternalGatesReport({
    resourceBytes: fixture.resourceBytes,
    resourceReport: fixture.resourceReport,
    platformBundle: fixture.platformBundle,
    hostedRunBytes: Buffer.from(JSON.stringify(changedHostedRun)),
    hostedRunReceipt: changedHostedRun,
    commitOrderVerified: true,
  });
  assert.equal(hostedRejected.accepted, false);
  assert.ok(
    hostedRejected.failures.some((failure) =>
      failure.includes("hosted job conclusion is not success"),
    ),
  );
  assert.ok(
    report.failures.includes(
      "P2 commits are not ordered soak <= hosted matrix <= current HEAD",
    ),
  );

  await writeFile(fixture.resourcePath, changedBytes);
  const result = runVerifier(fixture);
  assert.equal(result.status, 1);
  assert.equal(JSON.parse(result.stdout).accepted, false);
  await assert.rejects(readFile(fixture.output), /ENOENT/u);
});

async function createFixture() {
  const root = await mkdtemp(path.join(tmpdir(), "material-eagle-p2-gates-"));
  const platformArchive = path.join(root, "platform");
  const resourcePath = path.join(root, "p2-06-resource-soak.json");
  const hostedRunPath = path.join(root, "p2-a12-hosted-run.json");
  const output = path.join(root, "p2-external-gates.json");
  const commit = readCommit();
  await mkdir(platformArchive);

  const now = Date.now();
  const resourceCompletedAt = new Date(now - 10_000);
  const resourceStartedAt = new Date(
    resourceCompletedAt.getTime() -
      FORMAL_RESOURCE_STABILITY_OPTIONS.durationSeconds * 1_000,
  );
  const resourceReport = makeResourceReport({
    commit,
    startedAt: resourceStartedAt,
    completedAt: resourceCompletedAt,
  });
  assert.equal(
    resourceReport.accepted,
    true,
    resourceReport.failures.join("; "),
  );
  const resourceBytes = Buffer.from(
    `${JSON.stringify(resourceReport, null, 2)}\n`,
  );
  await writeFile(resourcePath, resourceBytes);

  const sourceCompletedAt = new Date(now - 2_000).toISOString();
  const sourceStartedAt = new Date(now - 62_000).toISOString();
  const sources = [];
  for (const platform of ["darwin", "linux", "win32"]) {
    const report = makeSourceReport({
      platform,
      commit,
      startedAt: sourceStartedAt,
      completedAt: sourceCompletedAt,
    });
    const bytes = Buffer.from(`${JSON.stringify(report, null, 2)}\n`);
    const artifactName = `p2-a12-source-${runnerOsFor(platform)}-${commit}-attempt-1`;
    const fileName = "p2-a12-platform-paths.json";
    await mkdir(path.join(platformArchive, artifactName));
    await writeFile(path.join(platformArchive, artifactName, fileName), bytes);
    sources.push({
      artifactName,
      fileName,
      sha256: sha256(bytes),
      report,
      bytes,
    });
  }

  const matrix = buildPlatformMatrixReport({
    sources,
    repositoryCommit: commit,
    nodeVersion: process.version,
    verifiedAt: new Date(now).toISOString(),
    workflowContext: workflowContext(commit),
  });
  assert.equal(matrix.accepted, true, matrix.failures.join("; "));
  const matrixBytes = Buffer.from(`${JSON.stringify(matrix, null, 2)}\n`);
  const matrixArtifactName = `p2-a12-matrix-${commit}-attempt-1`;
  await mkdir(path.join(platformArchive, matrixArtifactName));
  await writeFile(
    path.join(
      platformArchive,
      matrixArtifactName,
      "p2-08-platform-matrix.json",
    ),
    matrixBytes,
  );

  const platformBundle = {
    matrixArtifactName,
    matrixReport: matrix,
    matrixBytes,
    sources,
  };
  const hostedRun = makeHostedRun({
    commit,
    runUrl: matrix.workflow.runUrl,
    updatedAt: new Date(now + 1_000).toISOString(),
  });
  const hostedInspection = inspectP2HostedRun({
    run: hostedRun,
    requestedRunId: 123456,
    requestedAttempt: 1,
    expectedCommit: commit,
    repositorySlug: "owner/repository",
  });
  assert.equal(
    hostedInspection.accepted,
    true,
    hostedInspection.failures.join("; "),
  );
  const hostedRunReceipt = {
    ...buildP2HostedRunReceipt({
      inspection: hostedInspection,
      run: hostedRun,
      repositorySlug: "owner/repository",
      archive: makeArchiveReport(platformBundle),
    }),
    temporaryDownloadRemoved: true,
  };
  assert.equal(
    hostedRunReceipt.accepted,
    true,
    hostedRunReceipt.failures.join("; "),
  );
  const hostedRunBytes = Buffer.from(
    `${JSON.stringify(hostedRunReceipt, null, 2)}\n`,
  );
  await writeFile(hostedRunPath, hostedRunBytes);

  return {
    root,
    resourcePath,
    resourceReport,
    resourceBytes,
    platformArchive,
    hostedRunPath,
    hostedRunReceipt,
    hostedRunBytes,
    output,
    matrix,
    platformBundle,
  };
}

function makeResourceReport({ commit, startedAt, completedAt }) {
  const options = FORMAL_RESOURCE_STABILITY_OPTIONS;
  const internalSamples = [];
  const externalSamples = [];
  for (
    let elapsedMs = 0;
    elapsedMs <= options.durationSeconds * 1_000;
    elapsedMs += options.sampleIntervalSeconds * 1_000
  ) {
    internalSamples.push({
      status:
        elapsedMs === options.durationSeconds * 1_000 ? "complete" : "running",
      elapsedMs,
      sourceAssets: options.fixtureCount,
      scanPasses: Math.max(1, Math.floor(elapsedMs / 20_000)),
      watcherBatches: Math.max(1, Math.floor(elapsedMs / 5_000)),
      generatedEvents: Math.max(1, Math.floor(elapsedMs / 500)),
      thumbnailRequests: Math.max(1, Math.floor(elapsedMs / 50)),
      hashRequests: Math.max(1, Math.floor(elapsedMs / 50)),
      scheduler: {
        mode:
          elapsedMs >= 7_200_000 && elapsedMs < 14_400_000
            ? "background"
            : "foreground",
        foregroundLimit: 4,
        maxWaiters: 256,
        activeTotal: 1,
        waitingTotal: 0,
        peakActiveTotal: 2,
        peakWaitingTotal: 0,
        backgroundLimit: 2,
        scan: makeWorkSnapshot(Math.max(1, Math.floor(elapsedMs / 20_000))),
        hash: makeWorkSnapshot(Math.max(1, Math.floor(elapsedMs / 50))),
        decode: makeWorkSnapshot(Math.max(1, Math.floor(elapsedMs / 50))),
      },
      cache: {
        entryCount: Math.min(20_000, Math.floor(elapsedMs / 1_000)),
        maxEntries: 20_000,
        byteCount: Math.min(128 * 1024 * 1024, elapsedMs * 10),
        maxBytes: 1024 * 1024 * 1024,
      },
    });
    externalSamples.push({
      elapsedMs,
      rssKiB: 100_000 + Math.floor(elapsedMs / 3_600_000),
      cpuPercent: 25,
      threads: 4,
      handles: 10,
    });
  }
  return buildResourceStabilityReport({
    startedAt,
    completedAt,
    exit: { code: 0, signal: null },
    stderr: "",
    internalSamples,
    externalSamples,
    sampleParseErrors: [],
    monitorErrors: [],
    options,
    gitCommit: commit,
    environment: {
      platform: "darwin",
      architecture: "arm64",
      nodeVersion: process.version,
    },
  });
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

function makeSourceReport({ platform, commit, startedAt, completedAt }) {
  const expected = expectedPlatformPathTests(platform);
  return {
    schema: 1,
    accepted: true,
    failures: [],
    startedAt,
    completedAt,
    gitCommit: commit,
    command:
      "cargo test --locked -p asset-filesystem p2_platform -- --nocapture",
    environment: {
      platform,
      architecture: "x64",
      nodeVersion: process.version,
      rustc: "rustc phase-2-fixture",
      cargo: "cargo phase-2-fixture",
      ...workflowContext(commit),
      runnerOs: runnerOsFor(platform),
      runnerArch: "X64",
      runnerEnvironment: "github-hosted",
      gitCommit: commit,
    },
    requireWindowsSymlink: platform === "win32",
    expectedTests: expected,
    listedTests: expected,
    executedTests: expected,
    summary: {
      result: "ok",
      passed: expected.length,
      failed: 0,
      ignored: 0,
      measured: 0,
      filteredOut: 100,
    },
    processResults: {
      list: {
        status: 0,
        signal: null,
        error: null,
        stdout: expected.map((name) => `${name}: test`).join("\n"),
        stderr: "",
      },
      test: {
        status: 0,
        signal: null,
        error: null,
        stdout: [
          ...expected.map((name) => `test ${name} ... ok`),
          `test result: ok. ${String(expected.length)} passed; 0 failed; 0 ignored; 0 measured; 100 filtered out;`,
        ].join("\n"),
        stderr: "",
      },
    },
  };
}

function workflowContext(commit) {
  return {
    githubActions: "true",
    githubSha: commit,
    githubRunId: "123456",
    githubRunAttempt: "1",
    githubWorkflowRef: workflowRef,
    githubRepository: "owner/repository",
    githubServerUrl: "https://github.com",
    runnerOs: "Linux",
    runnerArch: "X64",
    runnerEnvironment: "github-hosted",
  };
}

function makeHostedRun({ commit, runUrl, updatedAt }) {
  const createdAt = new Date(Date.parse(updatedAt) - 70_000).toISOString();
  const startedAt = new Date(Date.parse(updatedAt) - 69_000).toISOString();
  return {
    attempt: 1,
    conclusion: "success",
    createdAt,
    databaseId: 123456,
    event: "workflow_dispatch",
    headBranch: "main",
    headSha: commit,
    jobs: REQUIRED_P2_HOSTED_JOBS.map((name, index) => ({
      databaseId: 900_000 + index,
      name,
      status: "completed",
      conclusion: "success",
      startedAt,
      completedAt: updatedAt,
      url: `${runUrl}/job/${String(900_000 + index)}`,
    })),
    startedAt,
    status: "completed",
    updatedAt,
    url: runUrl,
    workflowName: "CI",
  };
}

function makeArchiveReport(platformBundle) {
  const matrix = platformBundle.matrixReport;
  return {
    archived: true,
    gitCommit: matrix.gitCommit,
    githubRunAttempt: matrix.workflow.githubRunAttempt,
    runUrl: matrix.workflow.runUrl,
    files: platformMatrixBundleEntries(platformBundle).map((entry) => ({
      relativePath: entry.relativePath,
      sha256: sha256(entry.bytes),
      bytes: entry.bytes.length,
    })),
  };
}

function runVerifier(fixture) {
  return spawnSync(
    process.execPath,
    [
      verifier,
      "--resource",
      fixture.resourcePath,
      "--platform-archive",
      fixture.platformArchive,
      "--hosted-run",
      fixture.hostedRunPath,
      "--output",
      fixture.output,
    ],
    {
      cwd: repository,
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
    },
  );
}

function runInspector(receiptPath) {
  return spawnSync(process.execPath, [inspector, receiptPath], {
    cwd: repository,
    encoding: "utf8",
  });
}

function readCommit() {
  const result = spawnSync("git", ["rev-parse", "HEAD"], {
    cwd: repository,
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

function runnerOsFor(platform) {
  return { darwin: "macOS", linux: "Linux", win32: "Windows" }[platform];
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}
