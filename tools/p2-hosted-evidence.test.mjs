import assert from "node:assert/strict";
import test from "node:test";

import {
  collectP2HostedEvidence,
  inspectP2HostedRun,
  p2HostedArtifactPatterns,
} from "./p2-hosted-evidence.mjs";

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
  assert.equal(inspection.failures.length, 9);
});

test("downloads, archives, then removes only a successfully processed bundle", async () => {
  const calls = [];
  const run = acceptedRun();
  const result = await collectP2HostedEvidence({
    inspection: { accepted: true, failures: [] },
    run,
    patterns: p2HostedArtifactPatterns(commit, 2),
    downloadDirectory: "/tmp/p2-hosted-test",
    downloadArtifacts: async (input) => calls.push(["download", input]),
    archiveEvidence: async (input) => {
      calls.push(["archive", input]);
      return { archived: true, alreadyPresent: false };
    },
    removeDownloadDirectory: async (input) => calls.push(["remove", input]),
  });

  assert.equal(result.collected, true);
  assert.equal(result.temporaryDownloadRemoved, true);
  assert.deepEqual(
    calls.map(([name]) => name),
    ["download", "archive", "remove"],
  );
});

test("does not download a rejected run or remove evidence after archive failure", async () => {
  let downloaded = false;
  await assert.rejects(
    collectP2HostedEvidence({
      inspection: { accepted: false, failures: ["not successful"] },
      run: acceptedRun(),
      patterns: [],
      downloadDirectory: "/tmp/p2-hosted-test",
      downloadArtifacts: async () => {
        downloaded = true;
      },
      archiveEvidence: async () => ({}),
      removeDownloadDirectory: async () => {},
    }),
    /hosted run rejected: not successful/u,
  );
  assert.equal(downloaded, false);

  let removed = false;
  await assert.rejects(
    collectP2HostedEvidence({
      inspection: { accepted: true, failures: [] },
      run: acceptedRun(),
      patterns: p2HostedArtifactPatterns(commit, 2),
      downloadDirectory: "/tmp/p2-hosted-test",
      downloadArtifacts: async () => {},
      archiveEvidence: async () => {
        throw new Error("archive rejected");
      },
      removeDownloadDirectory: async () => {
        removed = true;
      },
    }),
    /archive rejected/u,
  );
  assert.equal(removed, false);

  await assert.rejects(
    collectP2HostedEvidence({
      inspection: { accepted: true, failures: [] },
      run: acceptedRun(),
      patterns: ["wrong"],
      downloadDirectory: "/tmp/p2-hosted-test",
      downloadArtifacts: async () => {},
      archiveEvidence: async () => ({ archived: true }),
      removeDownloadDirectory: async () => {},
    }),
    /artifact patterns do not match/u,
  );
});

function acceptedRun() {
  return {
    attempt: 2,
    conclusion: "success",
    databaseId: 123,
    event: "workflow_dispatch",
    headBranch: "main",
    headSha: commit,
    status: "completed",
    url: `https://github.com/${repositorySlug}/actions/runs/123`,
    workflowName: "CI",
  };
}
