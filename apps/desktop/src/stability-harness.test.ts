import { describe, expect, it } from "vitest";

import {
  type StabilityHarnessState,
  summarizeStabilityState,
} from "./stability-harness";

describe("phase one stability summary", () => {
  it("accepts a bounded complete resource curve", () => {
    const state: StabilityHarnessState = {
      schema: 1,
      datasetSize: 10_000,
      warmupMs: 60_000,
      durationMs: 1_800_000,
      status: "complete",
      samples: [
        sample(0, 100_000_000, 900, 10_000, 0),
        sample(900_000, 110_000_000, 800, 3_750, 20),
        sample(1_800_000, 118_000_000, 900, 10_000, 30),
      ],
      resultObservations: [
        {
          query: "favorite:true",
          expected: 3_750,
          observed: 3_750,
          elapsedMs: 10_000,
        },
      ],
      actionCount: 1_900,
      longTaskCount: 20,
      longTaskDurationMs: 5_000,
      errors: [],
    };

    expect(summarizeStabilityState(state).accepted).toBe(true);
  });

  it("rejects missing heap metrics and unstable filters", () => {
    const state: StabilityHarnessState = {
      schema: 1,
      datasetSize: 10_000,
      warmupMs: 0,
      durationMs: 30_000,
      status: "complete",
      samples: [sample(0, null, 90_000, 10_000, 0)],
      resultObservations: [
        {
          query: "color/blue",
          expected: 2_500,
          observed: 2_499,
          elapsedMs: 10_000,
        },
      ],
      actionCount: 30,
      longTaskCount: 0,
      longTaskDurationMs: 0,
      errors: [],
    };

    const summary = summarizeStabilityState(state);
    expect(summary.accepted).toBe(false);
    expect(summary.failures).toContain(
      "browser JavaScript heap metrics are unavailable",
    );
    expect(summary.failures).toContain(
      "one or more filter result counts were unstable",
    );
  });
});

function sample(
  elapsedMs: number,
  heap: number | null,
  domNodes: number,
  resultCount: number,
  lag: number,
) {
  return {
    elapsedMs,
    usedJsHeapBytes: heap,
    totalJsHeapBytes: heap === null ? null : heap * 2,
    domNodes,
    assetCards: Math.min(resultCount, 60),
    resultCount,
    eventLoopLagMs: lag,
    activeObjectUrls: Math.min(resultCount, 60),
    createdObjectUrls: Math.min(resultCount, 60),
    revokedObjectUrls: 0,
  };
}
