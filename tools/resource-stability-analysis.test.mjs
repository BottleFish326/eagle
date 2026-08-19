import assert from "node:assert/strict";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  assertResourceStabilityRuntime,
  buildResourceStabilityReport,
  linearSlope,
} from "./resource-stability-analysis.mjs";
import { inspectResourceStabilityReport } from "./resource-stability-report.mjs";
import {
  createResourceStabilityCheckpoint,
  writeJsonAtomic,
} from "./resource-stability-checkpoint.mjs";
import { inspectResourceStabilityCheckpoint } from "./resource-stability-checkpoint-inspection.mjs";

const options = {
  durationSeconds: 100,
  warmupSeconds: 10,
  fixtureCount: 10,
  sampleIntervalSeconds: 5,
  checkpointIntervalSeconds: 10,
};

test("accepts a complete bounded run with representative sample coverage", () => {
  const report = buildResourceStabilityReport(acceptedInput());

  assert.equal(report.accepted, true);
  assert.deepEqual(report.failures, []);
  assert.equal(report.exit.code, 0);
  assert.ok(
    report.summary.internalSampleCount >=
      report.summary.minimumInternalSampleCount,
  );
  assert.ok(
    report.summary.nativeSampleCount >= report.summary.minimumNativeSampleCount,
  );
});

test("replays every raw sample before accepting a final report", () => {
  const report = buildResourceStabilityReport(acceptedInput());
  const inspection = inspectResourceStabilityReport(report, {
    expectedOptions: options,
  });
  assert.equal(inspection.accepted, true);
  assert.deepEqual(inspection.failures, []);
  assert.deepEqual(inspection.replayedReport, report);
});

test("rejects a stored summary or formal option that does not match replay", () => {
  const report = buildResourceStabilityReport(acceptedInput());
  report.summary.scanPasses += 1;
  report.durationSeconds = 99;
  const inspection = inspectResourceStabilityReport(report, {
    expectedOptions: options,
  });
  assert.equal(inspection.accepted, false);
  assert.ok(
    inspection.failures.some((failure) =>
      failure.includes("durationSeconds is 99, expected 100"),
    ),
  );
  assert.ok(
    inspection.failures.includes(
      "resource stability report does not equal its raw-sample replay",
    ),
  );
});

test("rejects a complete sample that does not cover the requested duration", () => {
  const input = acceptedInput();
  input.internalSamples = input.internalSamples.slice(0, 11);
  input.internalSamples.at(-1).status = "complete";

  const report = buildResourceStabilityReport(input);

  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("shorter than requested"),
    ),
  );
});

test("rejects sparse native monitoring even when endpoint values look bounded", () => {
  const input = acceptedInput();
  input.externalSamples = [
    input.externalSamples.at(0),
    input.externalSamples.at(-1),
  ];

  const report = buildResourceStabilityReport(input);

  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("native process samples, expected at least"),
    ),
  );
});

test("rejects untraceable revision, signaled exit, invalid shape, and overflow", () => {
  const input = acceptedInput();
  input.gitCommit = "dirty";
  input.environment.nodeVersion = "v25.9.0";
  input.exit = { code: null, signal: "SIGTERM" };
  input.internalSamples[3].scheduler.waitingTotal = "unknown";
  input.internalSamples[4].cache.entryCount =
    input.internalSamples[4].cache.maxEntries + 1;

  const report = buildResourceStabilityReport(input);

  assert.equal(report.accepted, false);
  assert.ok(report.failures.some((failure) => failure.includes("Git commit")));
  assert.ok(report.failures.some((failure) => failure.includes("Node.js 24")));
  assert.ok(report.failures.some((failure) => failure.includes("SIGTERM")));
  assert.ok(
    report.failures.some((failure) => failure.includes("invalid shape")),
  );
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("thumbnail cache exceeded"),
    ),
  );
});

test("requires the repository Node.js major before starting expensive work", () => {
  assert.doesNotThrow(() => assertResourceStabilityRuntime("24.19.0"));
  assert.throws(
    () => assertResourceStabilityRuntime("25.9.0"),
    /requires Node\.js 24\.x/u,
  );
});

test("atomically replaces checkpoint JSON without leaving temporary files", async () => {
  const directory = await mkdtemp(
    path.join(os.tmpdir(), "material-eagle-checkpoint-test-"),
  );
  const output = path.join(directory, "evidence.partial");
  try {
    await writeJsonAtomic(output, { sequence: 1 });
    await writeJsonAtomic(output, { sequence: 2 });

    assert.deepEqual(JSON.parse(await readFile(output, "utf8")), {
      sequence: 2,
    });
    assert.deepEqual(await readdir(directory), ["evidence.partial"]);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("accepts healthy partial evidence and rejects bounded-resource violations", () => {
  const input = acceptedInput();
  const checkpoint = createResourceStabilityCheckpoint({
    startedAt: input.startedAt,
    gitCommit: input.gitCommit,
    environment: input.environment,
    options: input.options,
    childPid: 123,
    internalSamples: input.internalSamples,
    externalSamples: input.externalSamples,
    sampleParseErrors: [],
    monitorErrors: [],
    stderr: "",
  });
  const healthy = inspectResourceStabilityCheckpoint(checkpoint);
  assert.equal(healthy.healthy, true);
  assert.deepEqual(healthy.failures, []);
  assert.equal(healthy.summary.targetDurationMs, 100_000);
  assert.equal(healthy.summary.coveredDurationMs, 100_000);
  assert.equal(healthy.summary.remainingDurationMs, 0);
  assert.equal(healthy.summary.progressPercent, 100);

  checkpoint.externalSamples.pop();
  const slowerNativeStream = inspectResourceStabilityCheckpoint(checkpoint);
  assert.equal(slowerNativeStream.healthy, true);
  assert.equal(slowerNativeStream.summary.coveredDurationMs, 95_000);
  assert.equal(slowerNativeStream.summary.remainingDurationMs, 5_000);
  assert.equal(slowerNativeStream.summary.progressPercent, 95);

  checkpoint.internalSamples[4].cache.entryCount =
    checkpoint.internalSamples[4].cache.maxEntries + 1;
  checkpoint.monitorErrors.push("checkpoint write failed");
  const unhealthy = inspectResourceStabilityCheckpoint(checkpoint);
  assert.equal(unhealthy.healthy, false);
  assert.ok(
    unhealthy.failures.some((failure) => failure.includes("cache exceeded")),
  );
  assert.ok(
    unhealthy.failures.some((failure) => failure.includes("monitor errors")),
  );
});

test("binds a partial checkpoint to the requested formal options", () => {
  const input = acceptedInput();
  const checkpoint = createResourceStabilityCheckpoint({
    startedAt: input.startedAt,
    gitCommit: input.gitCommit,
    environment: input.environment,
    options: input.options,
    childPid: 123,
    internalSamples: input.internalSamples,
    externalSamples: input.externalSamples,
    sampleParseErrors: [],
    monitorErrors: [],
    stderr: "",
  });

  checkpoint.options.durationSeconds = 99;
  const inspection = inspectResourceStabilityCheckpoint(checkpoint, {
    expectedOptions: options,
  });
  assert.equal(inspection.healthy, false);
  assert.ok(
    inspection.failures.includes(
      "checkpoint durationSeconds is 99, expected 100",
    ),
  );
});

test("reports a least-squares rate per minute", () => {
  assert.equal(
    linearSlope(
      [
        { elapsedMs: 0, rssKiB: 100 },
        { elapsedMs: 5_000, rssKiB: 105 },
        { elapsedMs: 10_000, rssKiB: 110 },
      ],
      "rssKiB",
    ),
    60,
  );
});

function acceptedInput() {
  const internalSamples = [];
  for (let elapsedMs = 0; elapsedMs <= 100_000; elapsedMs += 5_000) {
    internalSamples.push({
      status: elapsedMs === 100_000 ? "complete" : "running",
      elapsedMs,
      sourceAssets: 10,
      scanPasses: Math.max(1, Math.floor(elapsedMs / 10_000)),
      watcherBatches: Math.max(1, Math.floor(elapsedMs / 1_000)),
      generatedEvents: Math.max(1, Math.floor(elapsedMs / 250)),
      thumbnailRequests: Math.max(1, Math.floor(elapsedMs / 25)),
      hashRequests: Math.max(1, Math.floor(elapsedMs / 25)),
      scheduler: {
        mode:
          elapsedMs >= 25_000 && elapsedMs < 50_000
            ? "background"
            : "foreground",
        foregroundLimit: 4,
        maxWaiters: 256,
        activeTotal: 1,
        waitingTotal: 0,
        peakActiveTotal: 2,
        peakWaitingTotal: 0,
      },
      cache: {
        entryCount: Math.min(2_000, Math.floor(elapsedMs / 100)),
        maxEntries: 20_000,
        byteCount: Math.min(64 * 1024 * 1024, elapsedMs * 100),
        maxBytes: 1024 * 1024 * 1024,
      },
    });
  }
  const externalSamples = [];
  for (let elapsedMs = 0; elapsedMs <= 100_000; elapsedMs += 5_000) {
    externalSamples.push({
      elapsedMs,
      rssKiB: 100_000 + Math.floor(elapsedMs / 10_000),
      cpuPercent: 25,
      threads: 4,
      handles: 10,
    });
  }
  return {
    startedAt: new Date("2026-08-19T00:00:00.000Z"),
    exit: { code: 0, signal: null },
    stderr: "",
    internalSamples,
    externalSamples,
    sampleParseErrors: [],
    monitorErrors: [],
    options: { ...options },
    gitCommit: "a".repeat(40),
    environment: {
      platform: "test",
      architecture: "test",
      nodeVersion: "v24.0.0",
    },
  };
}
