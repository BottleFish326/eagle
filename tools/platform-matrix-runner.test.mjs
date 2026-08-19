import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { expectedPlatformPathTests } from "./platform-path-evidence.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const verifier = path.join(repository, "tools", "verify-platform-matrix.mjs");
const workflowRef = "owner/repository/.github/workflows/ci.yml@refs/heads/main";

test("CLI discovers, hashes, replays, and atomically consolidates three artifacts", async (context) => {
  const fixture = await createFixture();
  context.after(() => rm(fixture.root, { recursive: true, force: true }));
  const result = runVerifier(fixture);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const report = JSON.parse(await readFile(fixture.output, "utf8"));
  assert.equal(report.accepted, true);
  assert.deepEqual(report.failures, []);
  assert.equal(report.gitCommit, fixture.commit);
  assert.equal(report.workflow.githubRunId, "123456");
  assert.equal(report.verificationEnvironment.runnerOs, "Linux");
  assert.deepEqual(
    report.artifacts.map((artifact) => artifact.platform),
    ["darwin", "linux", "win32"],
  );
  for (const artifact of report.artifacts) {
    const bytes = await readFile(
      path.join(fixture.input, artifact.artifactName, artifact.fileName),
    );
    assert.equal(
      artifact.sha256,
      createHash("sha256").update(bytes).digest("hex"),
    );
    assert.equal("processResults" in artifact, false);
  }
});

test("CLI writes a rejected verdict when the artifact set is incomplete", async (context) => {
  const fixture = await createFixture(["darwin", "linux"]);
  context.after(() => rm(fixture.root, { recursive: true, force: true }));
  const result = runVerifier(fixture);
  assert.equal(result.status, 1);

  const report = JSON.parse(await readFile(fixture.output, "utf8"));
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.includes("found 2 P2-A12 source files, expected exactly 3"),
  );
  assert.equal(report.verificationEnvironment.githubRunAttempt, "1");
});

async function createFixture(platforms = ["darwin", "linux", "win32"]) {
  const root = await mkdtemp(path.join(tmpdir(), "material-eagle-p2-a12-"));
  const input = path.join(root, "input");
  const output = path.join(root, "p2-08-platform-matrix.json");
  const commit = readCommit();
  await mkdir(input);
  for (const platform of platforms) {
    const runnerOs = runnerOsFor(platform);
    const artifactName = `p2-a12-source-${runnerOs}-${commit}-attempt-1`;
    const directory = path.join(input, artifactName);
    await mkdir(directory);
    await writeFile(
      path.join(directory, "p2-a12-platform-paths.json"),
      `${JSON.stringify(makeReport(platform, commit), null, 2)}\n`,
      "utf8",
    );
  }
  return { root, input, output, commit };
}

function makeReport(platform, commit) {
  const runnerOs = runnerOsFor(platform);
  const expected = expectedPlatformPathTests(platform);
  const completedAt = new Date(Date.now() - 1_000).toISOString();
  const startedAt = new Date(Date.parse(completedAt) - 60_000).toISOString();
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
      rustc: "rustc integration-fixture",
      cargo: "cargo integration-fixture",
      githubActions: "true",
      githubSha: commit,
      githubRunId: "123456",
      githubRunAttempt: "1",
      githubWorkflowRef: workflowRef,
      runnerOs,
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

function runVerifier(fixture) {
  return spawnSync(
    process.execPath,
    [verifier, "--input-directory", fixture.input, "--output", fixture.output],
    {
      cwd: repository,
      encoding: "utf8",
      env: {
        ...process.env,
        GITHUB_ACTIONS: "true",
        GITHUB_SHA: fixture.commit,
        GITHUB_RUN_ID: "123456",
        GITHUB_RUN_ATTEMPT: "1",
        GITHUB_WORKFLOW_REF: workflowRef,
        RUNNER_OS: "Linux",
        RUNNER_ARCH: "X64",
        MATERIAL_EAGLE_RUNNER_ENVIRONMENT: "github-hosted",
      },
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
