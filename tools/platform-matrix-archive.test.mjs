import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { inspectPlatformMatrixArchive } from "./platform-matrix-archive.mjs";
import { buildPlatformMatrixReport } from "./platform-matrix-analysis.mjs";
import { expectedPlatformPathTests } from "./platform-path-evidence.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const archiver = path.join(
  repository,
  "tools",
  "archive-platform-matrix-evidence.mjs",
);
const workflowRef = "owner/repository/.github/workflows/ci.yml@refs/heads/main";

test("archiver replays and atomically preserves the exact four downloaded files", async (context) => {
  const fixture = await createBundle();
  context.after(() => rm(fixture.root, { recursive: true, force: true }));

  const first = runArchiver(fixture);
  assert.equal(first.status, 0, first.stderr || first.stdout);
  const firstSummary = JSON.parse(first.stdout);
  assert.equal(firstSummary.archived, true);
  assert.equal(firstSummary.alreadyPresent, false);
  assert.equal(firstSummary.gitCommit, fixture.commit);
  assert.equal(firstSummary.runUrl, fixture.matrix.workflow.runUrl);
  assert.equal(firstSummary.files.length, 4);

  for (const entry of fixture.files) {
    const archived = await readFile(
      path.join(fixture.output, entry.relativePath),
    );
    assert.ok(archived.equals(entry.bytes));
  }

  const second = runArchiver(fixture);
  assert.equal(second.status, 0, second.stderr || second.stdout);
  assert.equal(JSON.parse(second.stdout).alreadyPresent, true);
});

test("archive inspection rejects a changed matrix or changed source bytes", async (context) => {
  const fixture = await createBundle();
  context.after(() => rm(fixture.root, { recursive: true, force: true }));

  const changedMatrix = structuredClone(fixture.matrix);
  changedMatrix.workflow.runUrl =
    "https://github.com/other/repository/actions/runs/123456";
  const matrixInspection = inspectPlatformMatrixArchive({
    matrixArtifactName: fixture.matrixArtifactName,
    matrixReport: changedMatrix,
    sources: fixture.sources,
  });
  assert.equal(matrixInspection.accepted, false);
  assert.ok(
    matrixInspection.failures.includes(
      "downloaded matrix does not equal the offline replay",
    ),
  );

  const sourcePath = path.join(
    fixture.input,
    fixture.sources[0].artifactName,
    fixture.sources[0].fileName,
  );
  await writeFile(
    sourcePath,
    Buffer.concat([fixture.sources[0].bytes, Buffer.from("\n")]),
  );
  const result = runArchiver(fixture);
  assert.equal(result.status, 1);
  assert.match(
    result.stderr,
    /downloaded matrix does not equal the offline replay/u,
  );
  await assert.rejects(readFile(fixture.output), /EISDIR|ENOENT/u);
});

async function createBundle() {
  const root = await mkdtemp(
    path.join(tmpdir(), "material-eagle-p2-a12-archive-"),
  );
  const input = path.join(root, "download");
  const output = path.join(root, "archive");
  const commit = readCommit();
  await mkdir(input);

  const completedAt = new Date(Date.now() - 2_000).toISOString();
  const startedAt = new Date(Date.parse(completedAt) - 60_000).toISOString();
  const sources = [];
  const files = [];
  for (const platform of ["darwin", "linux", "win32"]) {
    const report = makeSourceReport({
      platform,
      commit,
      startedAt,
      completedAt,
    });
    const bytes = Buffer.from(`${JSON.stringify(report, null, 2)}\n`);
    const artifactName = `p2-a12-source-${runnerOsFor(platform)}-${commit}-attempt-1`;
    const fileName = "p2-a12-platform-paths.json";
    const relativePath = path.join(artifactName, fileName);
    await mkdir(path.join(input, artifactName));
    await writeFile(path.join(input, relativePath), bytes);
    sources.push({
      artifactName,
      fileName,
      sha256: sha256(bytes),
      report,
      bytes,
    });
    files.push({ relativePath, bytes });
  }

  const matrix = buildPlatformMatrixReport({
    sources,
    repositoryCommit: commit,
    nodeVersion: process.version,
    verifiedAt: new Date().toISOString(),
    workflowContext: workflowContext(commit),
  });
  assert.equal(matrix.accepted, true, matrix.failures.join("; "));
  const matrixBytes = Buffer.from(`${JSON.stringify(matrix, null, 2)}\n`);
  const matrixArtifactName = `p2-a12-matrix-${commit}-attempt-1`;
  const matrixRelativePath = path.join(
    matrixArtifactName,
    "p2-08-platform-matrix.json",
  );
  await mkdir(path.join(input, matrixArtifactName));
  await writeFile(path.join(input, matrixRelativePath), matrixBytes);
  files.push({ relativePath: matrixRelativePath, bytes: matrixBytes });

  return {
    root,
    input,
    output,
    commit,
    matrix,
    matrixArtifactName,
    sources,
    files,
  };
}

function makeSourceReport({ platform, commit, startedAt, completedAt }) {
  const expected = expectedPlatformPathTests(platform);
  const environment = {
    platform,
    architecture: "x64",
    nodeVersion: process.version,
    rustc: "rustc archive-fixture",
    cargo: "cargo archive-fixture",
    ...workflowContext(commit),
    runnerOs: runnerOsFor(platform),
    runnerArch: "X64",
    runnerEnvironment: "github-hosted",
    gitCommit: commit,
  };
  return {
    schema: 1,
    accepted: true,
    failures: [],
    startedAt,
    completedAt,
    gitCommit: commit,
    command:
      "cargo test --locked -p asset-filesystem p2_platform -- --nocapture",
    environment,
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

function runArchiver(fixture) {
  return spawnSync(
    process.execPath,
    [
      archiver,
      "--input-directory",
      fixture.input,
      "--output-directory",
      fixture.output,
    ],
    {
      cwd: repository,
      encoding: "utf8",
      maxBuffer: 4 * 1024 * 1024,
    },
  );
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
