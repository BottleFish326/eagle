import assert from "node:assert/strict";
import test from "node:test";

import {
  buildP2HostedReadiness,
  githubRepositorySlug,
} from "./p2-hosted-readiness.mjs";

const commit = "a".repeat(40);

test("accepts one clean published main commit and emits commit-bound commands", () => {
  const report = buildP2HostedReadiness({
    ghAvailable: true,
    ghAuthenticated: true,
    remoteUrl: "git@github.com:owner/material-eagle.git",
    repositorySlug: "owner/material-eagle",
    defaultBranch: "main",
    branch: "main",
    upstream: "origin/main",
    currentCommit: commit,
    remoteCommit: commit,
    cleanTracked: true,
    workflowDispatchConfigured: true,
  });

  assert.equal(report.ready, true, report.failures.join("; "));
  assert.equal(report.github.originRepository, "owner/material-eagle");
  assert.match(report.commands[0], /gh workflow run ci\.yml --ref main/u);
  assert.match(report.commands[1], new RegExp(commit, "u"));
  assert.match(report.commands[1], /--json attempt,databaseId/u);
  assert.match(report.commands[3], /--run-id <run-id> --attempt <attempt>/u);
  assert.equal(report.commands.length, 4);
});

test("rejects missing tooling, mismatched publication, and a dirty workflow", () => {
  const report = buildP2HostedReadiness({
    ghAvailable: false,
    ghAuthenticated: false,
    remoteUrl: "https://gitlab.com/owner/material-eagle.git",
    repositorySlug: null,
    defaultBranch: null,
    branch: "feature/path",
    upstream: null,
    currentCommit: commit,
    remoteCommit: "b".repeat(40),
    cleanTracked: false,
    workflowDispatchConfigured: false,
  });

  assert.equal(report.ready, false);
  assert.ok(report.failures.includes("GitHub CLI is not installed"));
  assert.ok(
    report.failures.includes(
      "origin is not a supported github.com repository URL",
    ),
  );
  assert.ok(report.failures.includes("current branch is not main"));
  assert.ok(report.failures.includes("GitHub default branch is not main"));
  assert.ok(report.failures.includes("main does not track origin/main"));
  assert.ok(
    report.failures.includes("current HEAD is not published at origin/main"),
  );
  assert.ok(report.failures.includes("tracked files are not clean"));
  assert.ok(
    report.failures.includes("CI workflow does not expose workflow_dispatch"),
  );
  assert.deepEqual(report.commands, []);
});

test("normalizes supported GitHub remote URL forms", () => {
  for (const value of [
    "https://github.com/owner/repository.git",
    "http://github.com/owner/repository",
    "git@github.com:owner/repository.git",
    "ssh://git@github.com/owner/repository/",
  ]) {
    assert.equal(githubRepositorySlug(value), "owner/repository");
  }
  assert.equal(
    githubRepositorySlug("https://example.com/owner/repository"),
    null,
  );
  assert.equal(githubRepositorySlug("https://github.com/owner"), null);
});
