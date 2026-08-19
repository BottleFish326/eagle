const COMMON_TESTS = [
  "library::tests::p2_platform_rejects_configuration_that_enables_symlink_traversal",
  "library::tests::p2_platform_rejects_duplicate_and_overlapping_roots",
  "platform::tests::p2_platform_linux_keys_preserve_case_and_unicode_spelling",
  "platform::tests::p2_platform_macos_normalizes_unicode_without_assuming_volume_case_mode",
  "platform::tests::p2_platform_windows_keys_are_case_insensitive_and_unicode_normalized",
  "platform::tests::p2_platform_windows_portability_diagnostics_cover_names_characters_and_length",
  "scanner::tests::p2_platform_scans_native_unicode_path_without_rewriting_it",
];

const UNIX_TESTS = [
  "scanner::tests::p2_platform_disconnect_during_scan_is_non_authoritative",
  "scanner::tests::p2_platform_permission_revocation_during_scan_is_non_authoritative",
  "scanner::tests::p2_platform_symlink_loop_is_skipped_once_with_an_explicit_diagnostic",
];

const LINUX_TESTS = [
  "scanner::tests::p2_platform_linux_moved_mount_root_is_non_authoritative",
  "scanner::tests::p2_platform_linux_scans_case_distinct_files_as_separate_assets",
];

const WINDOWS_TESTS = [
  "scanner::tests::p2_platform_windows_long_path_scans_and_atomically_updates_sidecar",
  "scanner::tests::p2_platform_windows_symlink_loop_is_skipped_when_native_creation_is_available",
];

export function expectedPlatformPathTests(platform) {
  switch (platform) {
    case "darwin":
      return [...COMMON_TESTS, ...UNIX_TESTS].toSorted();
    case "linux":
      return [...COMMON_TESTS, ...UNIX_TESTS, ...LINUX_TESTS].toSorted();
    case "win32":
      return [...COMMON_TESTS, ...WINDOWS_TESTS].toSorted();
    default:
      throw new Error(`unsupported P2-A12 evidence platform ${platform}`);
  }
}

export function parseListedTests(output) {
  return uniqueSorted(
    output
      .split(/\r?\n/u)
      .map((line) => /^(.*): test$/u.exec(line.trim())?.[1])
      .filter((name) => name !== undefined && name.includes("p2_platform")),
  );
}

export function parseExecutedTests(output) {
  return uniqueSorted(
    output
      .split(/\r?\n/u)
      .map((line) => /^test (.*) \.\.\. ok$/u.exec(line.trim())?.[1])
      .filter((name) => name !== undefined && name.includes("p2_platform")),
  );
}

export function parseTestSummary(output) {
  const summaries = [
    ...output.matchAll(
      /test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;/gu,
    ),
  ];
  const match = summaries.at(-1);
  if (match === undefined) return null;
  return {
    result: match[1],
    passed: Number(match[2]),
    failed: Number(match[3]),
    ignored: Number(match[4]),
    measured: Number(match[5]),
    filteredOut: Number(match[6]),
  };
}

export function inspectPlatformPathRun({
  platform,
  listStatus,
  listOutput,
  testStatus,
  testOutput,
  requireWindowsSymlink,
}) {
  const expectedTests = expectedPlatformPathTests(platform);
  const listedTests = parseListedTests(listOutput);
  const executedTests = parseExecutedTests(testOutput);
  const summary = parseTestSummary(testOutput);
  const failures = [];

  if (listStatus !== 0)
    failures.push(`test listing exited with status ${String(listStatus)}`);
  if (testStatus !== 0)
    failures.push(
      `platform path suite exited with status ${String(testStatus)}`,
    );
  compareTests("listed", listedTests, expectedTests, failures);
  compareTests("executed", executedTests, expectedTests, failures);
  if (summary === null) {
    failures.push("cargo test summary was missing");
  } else if (
    summary.result !== "ok" ||
    summary.passed !== expectedTests.length ||
    summary.failed !== 0 ||
    summary.ignored !== 0 ||
    summary.measured !== 0
  ) {
    failures.push(
      `cargo test summary was not an exact ${String(expectedTests.length)}-test pass`,
    );
  }
  if (platform === "win32" && !requireWindowsSymlink) {
    failures.push(
      "Windows evidence did not require the native symlink fixture",
    );
  }
  if (testOutput.includes("native Windows symlink fixture skipped")) {
    failures.push("native Windows symlink fixture was skipped");
  }

  return {
    accepted: failures.length === 0,
    failures,
    expectedTests,
    listedTests,
    executedTests,
    summary,
  };
}

export function inspectHostedEvidenceContext({
  platform,
  gitCommit,
  environment,
}) {
  const failures = [];
  if (environment.githubActions !== "true")
    failures.push("P2-A12 formal evidence must run in GitHub Actions");
  if (environment.githubSha !== gitCommit)
    failures.push("GITHUB_SHA does not match the tested Git commit");
  const expectedRunnerOs = {
    darwin: "macOS",
    linux: "Linux",
    win32: "Windows",
  }[platform];
  if (expectedRunnerOs === undefined) {
    failures.push(`unsupported P2-A12 hosted platform ${platform}`);
  } else if (environment.runnerOs !== expectedRunnerOs) {
    failures.push(
      `RUNNER_OS ${String(environment.runnerOs)} does not match ${expectedRunnerOs}`,
    );
  }
  if (
    typeof environment.runnerArch !== "string" ||
    environment.runnerArch === ""
  )
    failures.push("RUNNER_ARCH is missing");
  if (environment.runnerEnvironment !== "github-hosted")
    failures.push("P2-A12 formal evidence must use a GitHub-hosted runner");
  if (!/^[^/\s]+\/[^/\s]+$/u.test(environment.githubRepository ?? ""))
    failures.push("GITHUB_REPOSITORY is missing or invalid");
  if (environment.githubServerUrl !== "https://github.com")
    failures.push("P2-A12 formal evidence must originate from github.com");
  return failures;
}

function compareTests(label, actual, expected, failures) {
  if (actual.length !== expected.length) {
    failures.push(
      `${label} ${String(actual.length)} tests, expected ${String(expected.length)}`,
    );
  }
  const actualSet = new Set(actual);
  const expectedSet = new Set(expected);
  const missing = expected.filter((name) => !actualSet.has(name));
  const unexpected = actual.filter((name) => !expectedSet.has(name));
  if (missing.length > 0)
    failures.push(`${label} tests missing: ${missing.join(", ")}`);
  if (unexpected.length > 0)
    failures.push(`${label} tests unexpected: ${unexpected.join(", ")}`);
}

function uniqueSorted(values) {
  return [...new Set(values)].toSorted();
}
