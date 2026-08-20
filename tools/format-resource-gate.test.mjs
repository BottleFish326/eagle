import assert from "node:assert/strict";
import test from "node:test";

import {
  aggregateProcessTreeRss,
  analyzeFormatResourceRuns,
} from "./format-resource-gate.mjs";

function acceptedReport() {
  return {
    accepted: true,
    providerProfile: "core-only",
    fixtureCount: 43,
    checkedFixtureCount: 43,
    adversarialFixtureCount: 31,
    manifestSha256: "a".repeat(64),
    sourceDigestUnchanged: true,
    cancellation: { accepted: true },
  };
}

test("aggregates only the selected process tree", () => {
  assert.deepEqual(
    aggregateProcessTreeRss(
      [
        { pid: 10, parentPid: 1, rssKiB: 100 },
        { pid: 11, parentPid: 10, rssKiB: 50 },
        { pid: 12, parentPid: 11, rssKiB: 25 },
        { pid: 20, parentPid: 1, rssKiB: 1000 },
      ],
      10,
    ),
    { rssKiB: 175, processCount: 3 },
  );
});

test("accepts repeated bounded adversarial reports", () => {
  const report = analyzeFormatResourceRuns({
    reports: [acceptedReport(), acceptedReport()],
    rssSamples: [
      { elapsedMs: 0, rssKiB: 100 },
      { elapsedMs: 20, rssKiB: 120 },
    ],
    maxRssKiB: 200,
    repositoryState: { gitCommit: "b".repeat(40), dirty: false },
    providerProfile: "core-only",
    environment: { platform: "test" },
  });
  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.equal(report.processTree.maxRssKiB, 120);
});

test("rejects drift, cancellation loss, sparse samples, and excess RSS", () => {
  const changed = acceptedReport();
  changed.sourceDigestUnchanged = false;
  changed.cancellation.accepted = false;
  const report = analyzeFormatResourceRuns({
    reports: [acceptedReport(), changed],
    rssSamples: [{ elapsedMs: 0, rssKiB: 300 }],
    maxRssKiB: 200,
    repositoryState: { gitCommit: "b".repeat(40), dirty: true },
    providerProfile: "core-only",
    environment: { platform: "test" },
  });
  assert.equal(report.accepted, false);
  assert.ok(report.failures.some((failure) => failure.includes("dirty")));
  assert.ok(report.failures.some((failure) => failure.includes("source")));
  assert.ok(
    report.failures.some((failure) => failure.includes("cancellation")),
  );
  assert.ok(report.failures.some((failure) => failure.includes("sparse")));
  assert.ok(report.failures.some((failure) => failure.includes("RSS")));
});
