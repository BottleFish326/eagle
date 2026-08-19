import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";

import {
  buildP2HostedRunReceipt,
  collectP2HostedEvidence,
  inspectP2HostedRun,
  p2HostedArtifactPatterns,
} from "./p2-hosted-evidence.mjs";
import { inspectP2HostedRunReceipt } from "./p2-hosted-run-receipt.mjs";
import { platformMatrixBundleEntries } from "./platform-matrix-bundle.mjs";

const commit = "a".repeat(40);
const repositorySlug = "owner/material-eagle";

test("accepts exact completed manual-run metadata and binds artifact patterns", () => {
  const run = acceptedRun();
  const inspection = inspectP2HostedRun({
    run,
    requestedRunId: 123,
    requestedAttempt: 2,
    expectedCommit: commit,
    repositorySlug,
  });
  assert.equal(inspection.accepted, true, inspection.failures.join("; "));
  assert.deepEqual(p2HostedArtifactPatterns(commit, 2), [
    `p2-a12-source-*-${commit}-attempt-2`,
    `p2-a12-matrix-${commit}-attempt-2`,
  ]);
});

test("rejects mismatched, incomplete, or non-manual workflow metadata", () => {
  const run = {
    ...acceptedRun(),
    databaseId: 999,
    attempt: 1,
    status: "in_progress",
    conclusion: "",
    event: "push",
    headBranch: "feature/path",
    headSha: "b".repeat(40),
    workflowName: "Other",
    url: "https://github.com/other/repository/actions/runs/999",
  };
  const inspection = inspectP2HostedRun({
    run,
    requestedRunId: 123,
    requestedAttempt: 2,
    expectedCommit: commit,
    repositorySlug,
  });
  assert.equal(inspection.accepted, false);
  assert.ok(inspection.failures.length >= 9);
});

test("downloads, archives, then removes only a successfully processed bundle", async () => {
  const calls = [];
  const run = acceptedRun();
  const inspection = inspectP2HostedRun({
    run,
    requestedRunId: 123,
    requestedAttempt: 2,
    expectedCommit: commit,
    repositorySlug,
  });
  const result = await collectP2HostedEvidence({
    inspection,
    run,
    repositorySlug,
    patterns: p2HostedArtifactPatterns(commit, 2),
    downloadDirectory: "/tmp/p2-hosted-test",
    downloadArtifacts: async (input) => calls.push(["download", input]),
    archiveEvidence: async (input) => {
      calls.push(["archive", input]);
      return archiveReport();
    },
    removeDownloadDirectory: async (input) => calls.push(["remove", input]),
    publishEvidence: async (input) => calls.push(["publish", input]),
  });

  assert.equal(result.accepted, true, result.failures.join("; "));
  assert.equal(result.temporaryDownloadRemoved, true);
  assert.equal(result.jobs.length, 5);
  assert.equal(result.archive.files.length, 4);
  assert.deepEqual(
    calls.map(([name]) => name),
    ["download", "archive", "remove", "publish"],
  );
});

test("does not download a rejected run or remove evidence after archive failure", async () => {
  let downloaded = false;
  await assert.rejects(
    collectP2HostedEvidence({
      inspection: { accepted: false, failures: ["not successful"] },
      run: acceptedRun(),
      repositorySlug,
      patterns: [],
      downloadDirectory: "/tmp/p2-hosted-test",
      downloadArtifacts: async () => {
        downloaded = true;
      },
      archiveEvidence: async () => ({}),
      removeDownloadDirectory: async () => {},
      publishEvidence: async () => {},
    }),
    /hosted run rejected: not successful/u,
  );
  assert.equal(downloaded, false);

  let removed = false;
  await assert.rejects(
    collectP2HostedEvidence({
      inspection: { accepted: true, failures: [] },
      run: acceptedRun(),
      repositorySlug,
      patterns: p2HostedArtifactPatterns(commit, 2),
      downloadDirectory: "/tmp/p2-hosted-test",
      downloadArtifacts: async () => {},
      archiveEvidence: async () => {
        throw new Error("archive rejected");
      },
      removeDownloadDirectory: async () => {
        removed = true;
      },
      publishEvidence: async () => {},
    }),
    /archive rejected/u,
  );
  assert.equal(removed, false);

  await assert.rejects(
    collectP2HostedEvidence({
      inspection: { accepted: true, failures: [] },
      run: acceptedRun(),
      repositorySlug,
      patterns: ["wrong"],
      downloadDirectory: "/tmp/p2-hosted-test",
      downloadArtifacts: async () => {},
      archiveEvidence: async () => ({ archived: true }),
      removeDownloadDirectory: async () => {},
      publishEvidence: async () => {},
    }),
    /artifact patterns do not match/u,
  );
});

test("rejects an archive receipt that does not bind the hosted run", () => {
  const run = acceptedRun();
  const inspection = inspectP2HostedRun({
    run,
    requestedRunId: 123,
    requestedAttempt: 2,
    expectedCommit: commit,
    repositorySlug,
  });
  const archive = archiveReport();
  archive.gitCommit = "b".repeat(40);
  archive.files[1].relativePath = archive.files[0].relativePath;
  const receipt = buildP2HostedRunReceipt({
    inspection,
    run,
    repositorySlug,
    archive,
  });
  assert.equal(receipt.accepted, false);
  assert.ok(
    receipt.failures.includes("archive commit does not match hosted run"),
  );
  assert.ok(
    receipt.failures.includes(
      "platform evidence archive file paths are not unique",
    ),
  );
});

test("replays a hosted receipt from archived bytes and rejects tampering", () => {
  const run = acceptedRun();
  const inspection = inspectP2HostedRun({
    run,
    requestedRunId: 123,
    requestedAttempt: 2,
    expectedCommit: commit,
    repositorySlug,
  });
  const bundle = platformBundle();
  const receipt = {
    ...buildP2HostedRunReceipt({
      inspection,
      run,
      repositorySlug,
      archive: archiveReportForBundle(bundle),
    }),
    temporaryDownloadRemoved: true,
  };
  const accepted = inspectP2HostedRunReceipt(receipt, bundle);
  assert.equal(accepted.accepted, true, accepted.failures.join("; "));

  const changedReceipt = structuredClone(receipt);
  changedReceipt.jobs[0].conclusion = "failure";
  assert.equal(
    inspectP2HostedRunReceipt(changedReceipt, bundle).accepted,
    false,
  );
  const changedBundle = structuredClone(bundle);
  changedBundle.sources[0].bytes = Buffer.from("changed source");
  assert.equal(
    inspectP2HostedRunReceipt(receipt, changedBundle).accepted,
    false,
  );
});

function acceptedRun() {
  const url = `https://github.com/${repositorySlug}/actions/runs/123`;
  return {
    attempt: 2,
    conclusion: "success",
    databaseId: 123,
    event: "workflow_dispatch",
    headBranch: "main",
    headSha: commit,
    status: "completed",
    createdAt: "2026-08-20T00:00:00.000Z",
    startedAt: "2026-08-20T00:00:01.000Z",
    updatedAt: "2026-08-20T00:10:00.000Z",
    url,
    workflowName: "CI",
    jobs: [
      "Path compatibility (ubuntu-24.04)",
      "Path compatibility (macos-15)",
      "Path compatibility (windows-2025)",
      "Consolidate path compatibility evidence",
      "Format, test, and build",
    ].map((name, index) => ({
      databaseId: 1_000 + index,
      name,
      status: "completed",
      conclusion: "success",
      startedAt: "2026-08-20T00:00:02Z",
      completedAt: "2026-08-20T00:09:59Z",
      url: `${url}/job/${String(1_000 + index)}`,
    })),
  };
}

function archiveReport() {
  return {
    archived: true,
    alreadyPresent: false,
    gitCommit: commit,
    githubRunAttempt: "2",
    runUrl: `https://github.com/${repositorySlug}/actions/runs/123`,
    files: ["Linux", "macOS", "Windows", "matrix"].map((label, index) => ({
      relativePath: `p2-a12-${label}/evidence-${String(index)}.json`,
      sha256: String(index).repeat(64),
      bytes: 100 + index,
    })),
  };
}

function platformBundle() {
  const runUrl = `https://github.com/${repositorySlug}/actions/runs/123`;
  return {
    matrixArtifactName: `p2-a12-matrix-${commit}-attempt-2`,
    matrixReport: {
      gitCommit: commit,
      workflow: {
        githubRunId: "123",
        githubRunAttempt: "2",
        runUrl,
      },
      verificationEnvironment: { githubRepository: repositorySlug },
    },
    matrixBytes: Buffer.from("matrix evidence"),
    sources: ["Linux", "macOS", "Windows"].map((runner) => ({
      artifactName: `p2-a12-source-${runner}-${commit}-attempt-2`,
      fileName: "p2-a12-platform-paths.json",
      bytes: Buffer.from(`${runner} evidence`),
    })),
  };
}

function archiveReportForBundle(bundle) {
  return {
    archived: true,
    gitCommit: commit,
    githubRunAttempt: "2",
    runUrl: `https://github.com/${repositorySlug}/actions/runs/123`,
    files: platformMatrixBundleEntries(bundle).map((entry) => ({
      relativePath: entry.relativePath,
      sha256: createHash("sha256").update(entry.bytes).digest("hex"),
      bytes: entry.bytes.length,
    })),
  };
}
