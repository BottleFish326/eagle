import assert from "node:assert/strict";
import test from "node:test";

import { expectedPlatformPathTests } from "./platform-path-evidence.mjs";
import { buildPlatformMatrixReport } from "./platform-matrix-analysis.mjs";

const COMMIT = "a".repeat(40);
const VERIFIED_AT = "2026-08-20T00:10:00.000Z";

test("accepts one replayed native report per hosted platform", () => {
  const report = buildPlatformMatrixReport(validInput());
  assert.equal(report.accepted, true);
  assert.deepEqual(report.failures, []);
  assert.deepEqual(
    report.artifacts.map((artifact) => artifact.platform),
    ["darwin", "linux", "win32"],
  );
  assert.equal(report.artifacts[0].expectedTests.length, 10);
  assert.equal(report.artifacts[1].expectedTests.length, 12);
  assert.equal(report.artifacts[2].expectedTests.length, 9);
  assert.deepEqual(report.workflow, {
    githubRunId: "123456",
    githubRunAttempt: "1",
    githubWorkflowRef:
      "owner/repository/.github/workflows/ci.yml@refs/heads/main",
  });
  assert.equal(report.verificationEnvironment.runnerOs, "Linux");
  assert.equal("processResults" in report.artifacts[0], false);
});

test("rejects a missing platform and a duplicate replacement", () => {
  const input = validInput();
  input.sources.pop();
  input.sources.push(makeSource("darwin"));
  const report = buildPlatformMatrixReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) => failure.includes("darwin appears 2")),
  );
  assert.ok(
    report.failures.some((failure) => failure.includes("linux appears 0")),
  );
});

test("rejects reports from different commits or workflow runs", () => {
  const input = validInput();
  input.sources[0].report.gitCommit = "b".repeat(40);
  input.sources[0].report.environment.gitCommit = "b".repeat(40);
  input.sources[1].report.environment.githubRunId = "999999";
  const report = buildPlatformMatrixReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) => failure.includes("repository commit")),
  );
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("one nonempty githubRunId"),
    ),
  );
});

test("rejects sources that do not match the verification job context", () => {
  const input = validInput();
  input.workflowContext.githubRunId = "654321";
  input.workflowContext.githubSha = "b".repeat(40);
  input.workflowContext.runnerEnvironment = "self-hosted";
  const report = buildPlatformMatrixReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("GITHUB_SHA does not match HEAD"),
    ),
  );
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("must use a GitHub-hosted runner"),
    ),
  );
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("githubRunId does not match verification job"),
    ),
  );
});

test("replays raw output and rejects stored or execution tampering", () => {
  const input = validInput();
  input.sources[0].report.listedTests = [];
  input.sources[1].report.processResults.test.stdout = executedOutput(
    expectedPlatformPathTests("linux").slice(1),
  );
  const report = buildPlatformMatrixReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("listedTests does not match"),
    ),
  );
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("replay: executed tests missing"),
    ),
  );
});

test("rejects a nonzero process even when the source claims acceptance", () => {
  const input = validInput();
  input.sources[1].report.processResults.test.status = 101;
  const report = buildPlatformMatrixReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) => failure.includes("test exited nonzero")),
  );
  assert.ok(
    report.failures.some((failure) => failure.includes("suite exited")),
  );
});

test("rejects self-hosted evidence and a skipped Windows symlink requirement", () => {
  const input = validInput();
  input.sources[1].report.environment.runnerEnvironment = "self-hosted";
  input.sources[0].report.requireWindowsSymlink = false;
  const report = buildPlatformMatrixReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) => failure.includes("GitHub-hosted")),
  );
  assert.ok(
    report.failures.some((failure) => failure.includes("symlink requirement")),
  );
});

test("rejects false source verdicts and nonempty source failures", () => {
  const input = validInput();
  input.sources[0].report.accepted = false;
  input.sources[0].report.failures = ["claimed failure"];
  const report = buildPlatformMatrixReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) => failure.includes("not accepted")),
  );
  assert.ok(report.failures.some((failure) => failure.includes("not empty")));
});

test("rejects an altered command or incomplete toolchain identity", () => {
  const input = validInput();
  input.sources[0].report.command = "cargo test";
  input.sources[1].report.environment.rustc = "";
  input.sources[2].report.environment.cargo = null;
  input.sources[2].report.environment.architecture = "";
  const report = buildPlatformMatrixReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("formal P2-A12 command"),
    ),
  );
  assert.ok(
    report.failures.some((failure) => failure.includes("environment rustc")),
  );
  assert.ok(
    report.failures.some((failure) => failure.includes("environment cargo")),
  );
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("environment architecture"),
    ),
  );
});

test("rejects wrong Node versions, timestamps, digests, and artifact names", () => {
  const input = validInput();
  input.nodeVersion = "v25.0.0";
  input.sources[0].report.environment.nodeVersion = "v22.0.0";
  input.sources[1].sha256 = "invalid";
  input.sources[2].artifactName = "../escape";
  input.sources[2].report.completedAt = "2026-08-20T00:20:00.000Z";
  const report = buildPlatformMatrixReport(input);
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) => failure.includes("requires Node.js 24")),
  );
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("did not run with Node.js 24"),
    ),
  );
  assert.ok(
    report.failures.some((failure) => failure.includes("digest is invalid")),
  );
  assert.ok(
    report.failures.some((failure) => failure.includes("safe basename")),
  );
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("after matrix verification"),
    ),
  );
});

function validInput() {
  return {
    sources: [makeSource("win32"), makeSource("darwin"), makeSource("linux")],
    repositoryCommit: COMMIT,
    nodeVersion: "v24.19.0",
    verifiedAt: VERIFIED_AT,
    workflowContext: {
      githubActions: "true",
      githubSha: COMMIT,
      githubRunId: "123456",
      githubRunAttempt: "1",
      githubWorkflowRef:
        "owner/repository/.github/workflows/ci.yml@refs/heads/main",
      runnerOs: "Linux",
      runnerArch: "X64",
      runnerEnvironment: "github-hosted",
    },
  };
}

function makeSource(platform) {
  const runnerOs = { darwin: "macOS", linux: "Linux", win32: "Windows" }[
    platform
  ];
  const expected = expectedPlatformPathTests(platform);
  const startedAt = "2026-08-20T00:00:00.000Z";
  const completedAt = "2026-08-20T00:05:00.000Z";
  return {
    artifactName: `p2-a12-source-${runnerOs}-${COMMIT}-attempt-1`,
    fileName: "p2-a12-platform-paths.json",
    sha256: platform.charCodeAt(0).toString(16).padStart(2, "0").repeat(32),
    report: {
      schema: 1,
      accepted: true,
      failures: [],
      startedAt,
      completedAt,
      gitCommit: COMMIT,
      command:
        "cargo test --locked -p asset-filesystem p2_platform -- --nocapture",
      environment: {
        platform,
        architecture: "x64",
        nodeVersion: "v24.19.0",
        rustc: "rustc 1.89.0",
        cargo: "cargo 1.89.0",
        githubActions: "true",
        githubSha: COMMIT,
        githubRunId: "123456",
        githubRunAttempt: "1",
        githubWorkflowRef:
          "owner/repository/.github/workflows/ci.yml@refs/heads/main",
        runnerOs,
        runnerArch: "X64",
        runnerEnvironment: "github-hosted",
        gitCommit: COMMIT,
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
          stdout: listedOutput(expected),
          stderr: "",
        },
        test: {
          status: 0,
          signal: null,
          error: null,
          stdout: executedOutput(expected),
          stderr: "",
        },
      },
    },
  };
}

function listedOutput(tests) {
  return tests.map((name) => `${name}: test`).join("\n");
}

function executedOutput(tests) {
  return [
    ...tests.map((name) => `test ${name} ... ok`),
    `test result: ok. ${String(tests.length)} passed; 0 failed; 0 ignored; 0 measured; 100 filtered out;`,
  ].join("\n");
}
