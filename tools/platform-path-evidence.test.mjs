import assert from "node:assert/strict";
import test from "node:test";

import {
  expectedPlatformPathTests,
  inspectHostedEvidenceContext,
  inspectPlatformPathRun,
  parseExecutedTests,
  parseListedTests,
  parseTestSummary,
} from "./platform-path-evidence.mjs";

for (const [platform, count] of [
  ["darwin", 10],
  ["linux", 12],
  ["win32", 9],
]) {
  test(`accepts the exact ${platform} native suite`, () => {
    const expected = expectedPlatformPathTests(platform);
    assert.equal(expected.length, count);
    const result = inspectPlatformPathRun({
      platform,
      listStatus: 0,
      listOutput: listedOutput(expected),
      testStatus: 0,
      testOutput: executedOutput(expected),
      requireWindowsSymlink: platform === "win32",
    });
    assert.equal(result.accepted, true);
    assert.deepEqual(result.failures, []);
    assert.deepEqual(result.listedTests, expected);
    assert.deepEqual(result.executedTests, expected);
    assert.equal(result.summary?.passed, count);
  });
}

test("parsers ignore unrelated Cargo lines and preserve the exact suite", () => {
  const expected = expectedPlatformPathTests("darwin");
  const list = `Finished test profile\n${listedOutput(expected)}\n0 tests, 0 benchmarks`;
  const executed = `running ${String(expected.length)} tests\n${executedOutput(expected)}\nDoc-tests asset_filesystem`;
  assert.deepEqual(parseListedTests(list), expected);
  assert.deepEqual(parseExecutedTests(executed), expected);
  assert.equal(parseTestSummary(executed)?.result, "ok");
});

test("rejects a missing listed or executed native test", () => {
  const expected = expectedPlatformPathTests("linux");
  const result = inspectPlatformPathRun({
    platform: "linux",
    listStatus: 0,
    listOutput: listedOutput(expected.slice(1)),
    testStatus: 0,
    testOutput: executedOutput(expected.slice(0, -1)),
    requireWindowsSymlink: false,
  });
  assert.equal(result.accepted, false);
  assert.ok(
    result.failures.some((failure) =>
      failure.startsWith("listed tests missing"),
    ),
  );
  assert.ok(
    result.failures.some((failure) =>
      failure.startsWith("executed tests missing"),
    ),
  );
});

test("rejects Windows evidence that can skip its symlink fixture", () => {
  const expected = expectedPlatformPathTests("win32");
  const result = inspectPlatformPathRun({
    platform: "win32",
    listStatus: 0,
    listOutput: listedOutput(expected),
    testStatus: 0,
    testOutput: `${executedOutput(expected)}\nnative Windows symlink fixture skipped`,
    requireWindowsSymlink: false,
  });
  assert.equal(result.accepted, false);
  assert.ok(
    result.failures.includes(
      "Windows evidence did not require the native symlink fixture",
    ),
  );
  assert.ok(
    result.failures.includes("native Windows symlink fixture was skipped"),
  );
});

test("rejects nonzero, ignored, failed, or missing-summary runs", () => {
  const expected = expectedPlatformPathTests("darwin");
  const failed = inspectPlatformPathRun({
    platform: "darwin",
    listStatus: 2,
    listOutput: listedOutput(expected),
    testStatus: 101,
    testOutput: [
      ...expected.map((name) => `test ${name} ... ok`),
      `test result: FAILED. ${String(expected.length)} passed; 1 failed; 1 ignored; 0 measured; 100 filtered out;`,
    ].join("\n"),
    requireWindowsSymlink: false,
  });
  assert.equal(failed.accepted, false);
  assert.ok(
    failed.failures.some((failure) => failure.includes("listing exited")),
  );
  assert.ok(
    failed.failures.some((failure) => failure.includes("suite exited")),
  );
  assert.ok(
    failed.failures.some((failure) => failure.includes("exact 10-test pass")),
  );

  const missingSummary = inspectPlatformPathRun({
    platform: "darwin",
    listStatus: 0,
    listOutput: listedOutput(expected),
    testStatus: 0,
    testOutput: expected.map((name) => `test ${name} ... ok`).join("\n"),
    requireWindowsSymlink: false,
  });
  assert.ok(missingSummary.failures.includes("cargo test summary was missing"));
});

test("rejects unsupported evidence platforms", () => {
  assert.throws(
    () => expectedPlatformPathTests("freebsd"),
    /unsupported P2-A12 evidence platform/u,
  );
});

test("accepts only a commit-bound GitHub-hosted runner context", () => {
  const gitCommit = "a".repeat(40);
  const accepted = inspectHostedEvidenceContext({
    platform: "linux",
    gitCommit,
    environment: {
      githubActions: "true",
      githubSha: gitCommit,
      runnerOs: "Linux",
      runnerArch: "X64",
      runnerEnvironment: "github-hosted",
      githubRepository: "owner/repository",
      githubServerUrl: "https://github.com",
    },
  });
  assert.deepEqual(accepted, []);

  const rejected = inspectHostedEvidenceContext({
    platform: "win32",
    gitCommit,
    environment: {
      githubActions: null,
      githubSha: "b".repeat(40),
      runnerOs: "Linux",
      runnerArch: null,
      runnerEnvironment: "self-hosted",
      githubRepository: null,
      githubServerUrl: null,
    },
  });
  assert.equal(rejected.length, 7);
  assert.ok(rejected.some((failure) => failure.includes("GitHub Actions")));
  assert.ok(rejected.some((failure) => failure.includes("GITHUB_SHA")));
  assert.ok(rejected.some((failure) => failure.includes("RUNNER_OS")));
  assert.ok(rejected.some((failure) => failure.includes("RUNNER_ARCH")));
  assert.ok(rejected.some((failure) => failure.includes("GitHub-hosted")));
  assert.ok(rejected.some((failure) => failure.includes("GITHUB_REPOSITORY")));
  assert.ok(rejected.some((failure) => failure.includes("github.com")));
});

function listedOutput(tests) {
  return tests.map((name) => `${name}: test`).join("\n");
}

function executedOutput(tests) {
  return [
    ...tests.map((name) => `test ${name} ... ok`),
    `test result: ok. ${String(tests.length)} passed; 0 failed; 0 ignored; 0 measured; 100 filtered out;`,
  ].join("\n");
}
