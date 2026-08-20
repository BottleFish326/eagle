import assert from "node:assert/strict";
import test from "node:test";

import { analyzeQueryPerformance } from "./query-performance-gate.mjs";

function fixture() {
  const timings = Array.from({ length: 200 }, (_, index) => 1_000_000 + index);
  return {
    productReport: {
      schema: 1,
      performance: {
        caseId: "combined-advanced",
        recordCount: 100_000,
        iterations: 200,
        resultCount: 20_312,
        indexBuildNanoseconds: 200_000_000,
        p50Nanoseconds: timings[99],
        p95Nanoseconds: timings[189],
        maxNanoseconds: timings[199],
        samplesNanoseconds: timings,
      },
    },
    rssSamples: Array.from({ length: 10 }, (_, index) => ({
      elapsedMs: index * 50,
      rssKiB: 20_000 + index * 100,
      processCount: 1,
    })),
    baselineRssKiB: 10_000,
    repositoryState: { gitCommit: "a".repeat(40), dirty: false },
    manifestSha256: "b".repeat(64),
    manifestDigestUnchanged: true,
    environment: { platform: "test" },
    options: {
      caseId: "combined-advanced",
      recordCount: 100_000,
      iterations: 200,
    },
  };
}

test("accepts replayed L query timings and bounded RSS growth", () => {
  const report = analyzeQueryPerformance(fixture());
  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.equal(report.latency.p95Nanoseconds, 1_000_189);
  assert.equal(report.processTree.deltaRssKiB, 10_900);
});

test("rejects summary drift, latency, resource, sample, and source failures", () => {
  const changed = fixture();
  changed.repositoryState.dirty = true;
  changed.manifestDigestUnchanged = false;
  changed.productReport.performance.p95Nanoseconds = 1;
  changed.productReport.performance.samplesNanoseconds.fill(100_000_001);
  changed.rssSamples = changed.rssSamples.slice(0, 1);
  changed.rssSamples[0].rssKiB = 2 * 1024 * 1024;
  const report = analyzeQueryPerformance(changed);
  assert.equal(report.accepted, false);
  for (const fragment of [
    "dirty",
    "changed",
    "replay",
    "p95",
    "sparse",
    "RSS",
  ]) {
    assert.ok(
      report.failures.some((failure) => failure.includes(fragment)),
      fragment,
    );
  }
});
