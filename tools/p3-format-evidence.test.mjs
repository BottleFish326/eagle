import assert from "node:assert/strict";
import test from "node:test";

import { analyzeFormatResourceRuns } from "./format-resource-gate.mjs";
import { analyzeP3FormatEvidence } from "./p3-format-evidence.mjs";

const commit = "a".repeat(40);
const manifest = "b".repeat(64);
const platforms = [
  ["Linux", "linux", "linux", "X64", "x64"],
  ["macOS", "macos", "darwin", "ARM64", "arm64"],
  ["Windows", "windows", "win32", "X64", "x64"],
];

function context() {
  return {
    gitCommit: commit,
    runId: "12345",
    runAttempt: "2",
    workflowRef: "owner/eagle/.github/workflows/ci.yml@refs/heads/main",
    repository: "owner/eagle",
    serverUrl: "https://github.com",
    nodeVersion: "v24.19.0",
  };
}

function gateRun(profile, platform) {
  return {
    schema: 1,
    accepted: true,
    platform,
    providerProfile: profile,
    manifestSha256: manifest,
    fixtureCount: 43,
    checkedFixtureCount: 43,
    adversarialFixtureCount: 31,
    scanElapsedMs: 10,
    sourceBytes: 1_468_344,
    sourceDigestUnchanged: true,
    cancellation: { accepted: true },
    failures: [],
  };
}

function sources() {
  const evidence = [];
  for (const profile of ["core-only", "bundled-codecs"]) {
    for (const [
      label,
      gatePlatform,
      nodePlatform,
      architecture,
      nodeArch,
    ] of platforms) {
      const suffix = profile === "bundled-codecs" ? `-${architecture}` : "";
      const base = `${profile}-${label}${suffix}-${commit}-attempt-2`;
      const runs = [
        gateRun(profile, gatePlatform),
        gateRun(profile, gatePlatform),
        gateRun(profile, gatePlatform),
      ];
      const environment = {
        platform: nodePlatform,
        architecture: nodeArch,
        nodeVersion: "v24.19.0",
      };
      const resource = analyzeFormatResourceRuns({
        reports: runs,
        rssSamples: [
          { elapsedMs: 0, rssKiB: 100, iteration: 1 },
          { elapsedMs: 10, rssKiB: 120, iteration: 2 },
          { elapsedMs: 20, rssKiB: 110, iteration: 3 },
        ],
        repositoryState: { gitCommit: commit, dirty: false },
        providerProfile: profile,
        environment,
      });
      evidence.push({
        artifactName: `p3-a01-${base}`,
        fileName: `p3-a01-${profile}.json`,
        sha256: "c".repeat(64),
        report: runs[0],
      });
      evidence.push({
        artifactName: `p3-a02-${base}`,
        fileName: `p3-a02-${profile}-resources.json`,
        sha256: "d".repeat(64),
        report: resource,
      });
    }
  }
  return evidence;
}

test("accepts twelve commit-bound reports after replaying raw resource samples", () => {
  const report = analyzeP3FormatEvidence({
    sources: sources(),
    context: context(),
  });
  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.equal(report.profiles.length, 2);
  assert.equal(report.sources.length, 12);
});

test("rejects missing, cross-commit, and summary-tampered evidence", () => {
  const evidence = sources();
  evidence.shift();
  evidence[0].artifactName = evidence[0].artifactName.replace(
    commit,
    "e".repeat(40),
  );
  evidence.find((source) =>
    source.artifactName.startsWith("p3-a02-"),
  ).report.processTree.maxRssKiB = 1;
  const report = analyzeP3FormatEvidence({
    sources: evidence,
    context: context(),
  });
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) => failure.includes("missing evidence")),
  );
  assert.ok(
    report.failures.some((failure) => failure.includes("different commit")),
  );
  assert.ok(
    report.failures.some((failure) => failure.includes("raw-sample replay")),
  );
});

test("rejects a self-consistent resource report whose gate failed", () => {
  const evidence = sources();
  const resource = evidence.find((source) =>
    source.artifactName.startsWith("p3-a02-"),
  );
  resource.report = analyzeFormatResourceRuns({
    reports: resource.report.runs,
    rssSamples: resource.report.processTree.samples.filter(
      (sample) => sample.iteration !== 3,
    ),
    repositoryState: { gitCommit: commit, dirty: false },
    providerProfile: "core-only",
    environment: resource.report.environment,
  });
  const report = analyzeP3FormatEvidence({
    sources: evidence,
    context: context(),
  });
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("resource gate was not accepted"),
    ),
  );
});
