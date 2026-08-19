import { rename, rm, writeFile } from "node:fs/promises";

export function createResourceStabilityCheckpoint({
  startedAt,
  gitCommit,
  environment,
  options,
  childPid,
  internalSamples,
  externalSamples,
  sampleParseErrors,
  monitorErrors,
  stderr,
}) {
  return {
    schema: 1,
    status: "running",
    gitCommit,
    environment,
    startedAt: startedAt.toISOString(),
    updatedAt: new Date().toISOString(),
    options: {
      durationSeconds: options.durationSeconds,
      warmupSeconds: options.warmupSeconds,
      fixtureCount: options.fixtureCount,
      sampleIntervalSeconds: options.sampleIntervalSeconds,
      checkpointIntervalSeconds: options.checkpointIntervalSeconds,
    },
    childPid,
    internalSamples,
    externalSamples,
    sampleParseErrors,
    monitorErrors,
    stderr: stderr.trim(),
  };
}

export async function writeJsonAtomic(output, value) {
  const temporary = `${output}.tmp-${String(process.pid)}-${String(Date.now())}`;
  try {
    await writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`, {
      flag: "wx",
    });
    await rename(temporary, output);
  } finally {
    await rm(temporary, { force: true });
  }
}

export function inspectResourceStabilityCheckpoint(checkpoint) {
  const failures = [];
  if (!isRecord(checkpoint) || checkpoint.schema !== 1) {
    return unhealthy("checkpoint root or schema is invalid");
  }
  if (checkpoint.status !== "running") {
    failures.push("checkpoint status is not running");
  }
  if (!/^[0-9a-f]{40,64}$/u.test(checkpoint.gitCommit ?? "")) {
    failures.push("checkpoint Git commit is missing or invalid");
  }
  if (!/^v24\./u.test(checkpoint.environment?.nodeVersion ?? "")) {
    failures.push("checkpoint was not produced by Node.js 24");
  }
  const intervalSeconds = checkpoint.options?.sampleIntervalSeconds;
  const fixtureCount = checkpoint.options?.fixtureCount;
  if (!isPositiveInteger(intervalSeconds) || !isPositiveInteger(fixtureCount)) {
    failures.push("checkpoint sampling options are invalid");
  }
  const internalSamples = Array.isArray(checkpoint.internalSamples)
    ? checkpoint.internalSamples
    : [];
  const externalSamples = Array.isArray(checkpoint.externalSamples)
    ? checkpoint.externalSamples
    : [];
  if (internalSamples.length === 0)
    failures.push("checkpoint has no internal samples");
  if (externalSamples.length === 0)
    failures.push("checkpoint has no native samples");
  if (!hasMonotonicElapsed(internalSamples)) {
    failures.push("checkpoint internal sample time is not monotonic");
  }
  if (!hasMonotonicElapsed(externalSamples)) {
    failures.push("checkpoint native sample time is not monotonic");
  }
  const invalidInternal = internalSamples.filter(
    (sample) => !isCheckpointInternalSample(sample),
  ).length;
  const invalidExternal = externalSamples.filter(
    (sample) => !isCheckpointExternalSample(sample),
  ).length;
  if (invalidInternal > 0) {
    failures.push(
      `${String(invalidInternal)} checkpoint internal samples are invalid`,
    );
  }
  if (invalidExternal > 0) {
    failures.push(
      `${String(invalidExternal)} checkpoint native samples are invalid`,
    );
  }
  const validInternal = internalSamples.filter(isCheckpointInternalSample);
  const validExternal = externalSamples.filter(isCheckpointExternalSample);
  const lastInternal = validInternal.at(-1);
  const lastExternal = validExternal.at(-1);
  if (isPositiveInteger(intervalSeconds)) {
    requirePartialCoverage(
      failures,
      "internal",
      validInternal.length,
      lastInternal?.elapsedMs,
      intervalSeconds,
    );
    requirePartialCoverage(
      failures,
      "native",
      validExternal.length,
      lastExternal?.elapsedMs,
      intervalSeconds,
    );
  }
  for (const sample of validInternal) {
    if (sample.sourceAssets !== fixtureCount) {
      failures.push("checkpoint source asset count changed");
      break;
    }
    if (sample.scheduler.activeTotal > sample.scheduler.foregroundLimit) {
      failures.push("checkpoint scheduler active work exceeded its bound");
      break;
    }
    if (sample.scheduler.waitingTotal > sample.scheduler.maxWaiters) {
      failures.push("checkpoint scheduler waiting work exceeded its bound");
      break;
    }
    if (
      sample.cache.entryCount > sample.cache.maxEntries ||
      sample.cache.byteCount > sample.cache.maxBytes
    ) {
      failures.push("checkpoint thumbnail cache exceeded its bound");
      break;
    }
  }
  if (!hasMonotonicCounters(validInternal)) {
    failures.push("checkpoint activity counters moved backwards");
  }
  if ((checkpoint.sampleParseErrors?.length ?? 0) > 0) {
    failures.push("checkpoint contains internal sample parse errors");
  }
  if ((checkpoint.monitorErrors?.length ?? 0) > 0) {
    failures.push("checkpoint contains monitor errors");
  }
  if (
    (lastInternal?.elapsedMs ?? 0) >= 240_000 &&
    !validInternal.some((sample) => sample.scheduler.mode === "background")
  ) {
    failures.push("checkpoint has not observed background resource mode");
  }
  if (
    lastInternal !== undefined &&
    (lastInternal.scanPasses === 0 ||
      lastInternal.generatedEvents === 0 ||
      lastInternal.watcherBatches === 0 ||
      lastInternal.thumbnailRequests === 0 ||
      lastInternal.hashRequests === 0)
  ) {
    failures.push(
      "checkpoint is missing required scan/event/hash/decode activity",
    );
  }

  return {
    healthy: failures.length === 0,
    failures,
    summary: {
      updatedAt: checkpoint.updatedAt ?? null,
      gitCommit: checkpoint.gitCommit ?? null,
      internalSampleCount: validInternal.length,
      nativeSampleCount: validExternal.length,
      latestInternalElapsedMs: lastInternal?.elapsedMs ?? null,
      latestNativeElapsedMs: lastExternal?.elapsedMs ?? null,
      modes: [
        ...new Set(validInternal.map((sample) => sample.scheduler.mode)),
      ].sort(),
      scanPasses: lastInternal?.scanPasses ?? 0,
      generatedEvents: lastInternal?.generatedEvents ?? 0,
      cacheEntries: lastInternal?.cache.entryCount ?? 0,
      cacheBytes: lastInternal?.cache.byteCount ?? 0,
      peakActive: maximum(
        validInternal.map((sample) => sample.scheduler.peakActiveTotal),
      ),
      peakWaiting: maximum(
        validInternal.map((sample) => sample.scheduler.peakWaitingTotal),
      ),
    },
  };
}

function unhealthy(message) {
  return { healthy: false, failures: [message], summary: null };
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isFiniteNonnegative(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function isNonnegativeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

function isPositiveInteger(value) {
  return Number.isSafeInteger(value) && value > 0;
}

function isCheckpointInternalSample(sample) {
  return (
    isRecord(sample) &&
    ["running", "complete"].includes(sample.status) &&
    isFiniteNonnegative(sample.elapsedMs) &&
    isNonnegativeInteger(sample.sourceAssets) &&
    isNonnegativeInteger(sample.scanPasses) &&
    isNonnegativeInteger(sample.watcherBatches) &&
    isNonnegativeInteger(sample.generatedEvents) &&
    isNonnegativeInteger(sample.thumbnailRequests) &&
    isNonnegativeInteger(sample.hashRequests) &&
    isRecord(sample.scheduler) &&
    typeof sample.scheduler.mode === "string" &&
    isNonnegativeInteger(sample.scheduler.activeTotal) &&
    isNonnegativeInteger(sample.scheduler.waitingTotal) &&
    isNonnegativeInteger(sample.scheduler.peakActiveTotal) &&
    isNonnegativeInteger(sample.scheduler.peakWaitingTotal) &&
    isNonnegativeInteger(sample.scheduler.foregroundLimit) &&
    isNonnegativeInteger(sample.scheduler.maxWaiters) &&
    isRecord(sample.cache) &&
    isNonnegativeInteger(sample.cache.entryCount) &&
    isNonnegativeInteger(sample.cache.maxEntries) &&
    isNonnegativeInteger(sample.cache.byteCount) &&
    isNonnegativeInteger(sample.cache.maxBytes)
  );
}

function isCheckpointExternalSample(sample) {
  return (
    isRecord(sample) &&
    isFiniteNonnegative(sample.elapsedMs) &&
    isFiniteNonnegative(sample.rssKiB) &&
    isFiniteNonnegative(sample.cpuPercent) &&
    isNonnegativeInteger(sample.threads) &&
    isNonnegativeInteger(sample.handles)
  );
}

function hasMonotonicElapsed(samples) {
  return samples.every(
    (sample, index) =>
      index === 0 ||
      (isFiniteNonnegative(sample?.elapsedMs) &&
        sample.elapsedMs >= samples[index - 1]?.elapsedMs),
  );
}

function requirePartialCoverage(
  failures,
  label,
  count,
  elapsedMs,
  intervalSeconds,
) {
  if (!isFiniteNonnegative(elapsedMs)) return;
  const expected = Math.floor(elapsedMs / (intervalSeconds * 1_000)) + 1;
  const minimum = Math.max(2, Math.floor(expected * 0.75));
  if (count < minimum) {
    failures.push(
      `checkpoint captured ${String(count)} ${label} samples, expected at least ${String(minimum)}`,
    );
  }
}

function hasMonotonicCounters(samples) {
  const keys = [
    "scanPasses",
    "watcherBatches",
    "generatedEvents",
    "thumbnailRequests",
    "hashRequests",
  ];
  return samples.every(
    (sample, index) =>
      index === 0 ||
      keys.every((key) => sample[key] >= samples[index - 1][key]),
  );
}

function maximum(values) {
  return values.length === 0 ? null : Math.max(...values);
}
