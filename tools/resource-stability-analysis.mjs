const COVERAGE_RATIO = 0.75;

export function buildResourceStabilityReport({
  startedAt,
  exit,
  stderr,
  internalSamples,
  externalSamples,
  sampleParseErrors,
  options: runOptions,
  gitCommit,
  environment,
}) {
  const validInternalSamples = internalSamples.filter(isInternalSample);
  const validExternalSamples = externalSamples
    .filter(isExternalSample)
    .toSorted((left, right) => left.elapsedMs - right.elapsedMs);
  const invalidInternalSampleCount =
    internalSamples.length - validInternalSamples.length;
  const invalidExternalSampleCount =
    externalSamples.length - validExternalSamples.length;
  const measurementStartSeconds =
    runOptions.warmupSeconds < runOptions.durationSeconds
      ? runOptions.warmupSeconds
      : 0;
  const measured = validExternalSamples.filter(
    (sample) => sample.elapsedMs >= measurementStartSeconds * 1_000,
  );
  const resourceSamples =
    measured.length >= 2 ? measured : validExternalSamples;
  const first = resourceSamples.at(0);
  const last = resourceSamples.at(-1);
  const finalInternal = validInternalSamples.at(-1);
  const rssGrowthKiB =
    first === undefined || last === undefined
      ? null
      : last.rssKiB - first.rssKiB;
  const rssSlopeKiBPerMinute = linearSlope(resourceSamples, "rssKiB");
  const handleGrowth =
    first === undefined || last === undefined
      ? null
      : last.handles - first.handles;
  const threadBaseline = first?.threads ?? null;
  const maxThreads = maximum(resourceSamples, "threads");
  const maxHandles = maximum(resourceSamples, "handles");
  const minHandles = minimum(resourceSamples, "handles");
  const maxCpuPercent = maximum(resourceSamples, "cpuPercent");
  const minimumInternalSamples = minimumCoverageSamples(
    runOptions.durationSeconds,
    runOptions.sampleIntervalSeconds,
  );
  const minimumExternalSamples = minimumCoverageSamples(
    runOptions.durationSeconds - measurementStartSeconds,
    runOptions.sampleIntervalSeconds,
  );
  const failures = [];

  if (!/^[0-9a-f]{40,64}$/u.test(gitCommit)) {
    failures.push("resource soak Git commit was missing or invalid");
  }
  if (!/^v24\./u.test(environment?.nodeVersion ?? "")) {
    failures.push(
      "resource soak must run on the repository Node.js 24 runtime",
    );
  }
  if (exit?.code !== 0) {
    failures.push(`resource soak exited with code ${String(exit?.code)}`);
  }
  if (exit?.signal !== null && exit?.signal !== undefined) {
    failures.push(`resource soak exited after signal ${String(exit.signal)}`);
  }
  if (finalInternal?.status !== "complete") {
    failures.push("resource soak did not emit a complete sample");
  }
  if (
    finalInternal !== undefined &&
    finalInternal.elapsedMs < runOptions.durationSeconds * 1_000
  ) {
    failures.push(
      `complete sample elapsed ${String(finalInternal.elapsedMs)} ms is shorter than requested ${String(runOptions.durationSeconds * 1_000)} ms`,
    );
  }
  if (!hasMonotonicElapsed(validInternalSamples)) {
    failures.push("internal sample elapsed times were not monotonic");
  }
  if (sampleParseErrors.length > 0) {
    failures.push(
      `${String(sampleParseErrors.length)} internal samples were not valid JSON`,
    );
  }
  if (invalidInternalSampleCount > 0) {
    failures.push(
      `${String(invalidInternalSampleCount)} internal samples had an invalid shape`,
    );
  }
  if (invalidExternalSampleCount > 0) {
    failures.push(
      `${String(invalidExternalSampleCount)} native process samples had an invalid shape`,
    );
  }
  if (finalInternal?.sourceAssets !== runOptions.fixtureCount) {
    failures.push(
      `scanned ${String(finalInternal?.sourceAssets)} assets, expected ${String(runOptions.fixtureCount)}`,
    );
  }
  if (validInternalSamples.length < minimumInternalSamples) {
    failures.push(
      `captured ${String(validInternalSamples.length)} internal samples, expected at least ${String(minimumInternalSamples)}`,
    );
  }
  if (resourceSamples.length < minimumExternalSamples) {
    failures.push(
      `captured ${String(resourceSamples.length)} native process samples, expected at least ${String(minimumExternalSamples)}`,
    );
  }
  const lastExternalDeadline =
    runOptions.durationSeconds * 1_000 -
    runOptions.sampleIntervalSeconds * 2_000;
  if (
    last !== undefined &&
    lastExternalDeadline > 0 &&
    last.elapsedMs < lastExternalDeadline
  ) {
    failures.push(
      `last native process sample at ${String(last.elapsedMs)} ms did not cover the requested duration`,
    );
  }
  if (rssGrowthKiB !== null && rssGrowthKiB > 256 * 1_024) {
    failures.push(`RSS growth ${String(rssGrowthKiB)} KiB exceeds 262144 KiB`);
  }
  if (rssSlopeKiBPerMinute !== null && rssSlopeKiBPerMinute > 8 * 1_024) {
    failures.push(
      `RSS slope ${String(rssSlopeKiBPerMinute)} KiB/min exceeds 8192 KiB/min`,
    );
  }
  if (handleGrowth !== null && handleGrowth > 64) {
    failures.push(`handle growth ${String(handleGrowth)} exceeds 64`);
  }
  if (
    maxHandles !== null &&
    minHandles !== null &&
    maxHandles - minHandles > 128
  ) {
    failures.push(
      `handle range ${String(maxHandles - minHandles)} exceeds 128`,
    );
  }
  if (
    maxThreads !== null &&
    threadBaseline !== null &&
    maxThreads > threadBaseline + 16
  ) {
    failures.push(`thread peak ${String(maxThreads)} exceeds baseline + 16`);
  }
  const foregroundLimit = finalInternal?.scheduler.foregroundLimit;
  if (
    maxCpuPercent !== null &&
    foregroundLimit !== undefined &&
    maxCpuPercent > foregroundLimit * 100 + 50
  ) {
    failures.push(
      `CPU peak ${String(maxCpuPercent)} exceeds scheduler capacity envelope`,
    );
  }
  for (const sample of validInternalSamples) {
    if (sample.scheduler.activeTotal > sample.scheduler.foregroundLimit) {
      failures.push("scheduler active work exceeded the foreground bound");
      break;
    }
    if (sample.scheduler.waitingTotal > sample.scheduler.maxWaiters) {
      failures.push("scheduler waiters exceeded the bounded queue");
      break;
    }
    if (
      sample.cache.entryCount > sample.cache.maxEntries ||
      sample.cache.byteCount > sample.cache.maxBytes
    ) {
      failures.push("thumbnail cache exceeded its configured bound");
      break;
    }
  }
  if (
    (finalInternal?.generatedEvents ?? 0) === 0 ||
    (finalInternal?.watcherBatches ?? 0) === 0
  ) {
    failures.push("filesystem event activity was not observed");
  }
  if (
    (finalInternal?.thumbnailRequests ?? 0) === 0 ||
    (finalInternal?.hashRequests ?? 0) === 0
  ) {
    failures.push("decode or hash activity was not observed");
  }
  if (
    runOptions.durationSeconds >= 240 &&
    !validInternalSamples.some(
      (sample) => sample.scheduler.mode === "background",
    )
  ) {
    failures.push("background resource mode was not observed");
  }

  return {
    schema: 1,
    command: [
      "node tools/verify-resource-stability.mjs",
      `--duration-seconds ${String(runOptions.durationSeconds)}`,
      `--warmup-seconds ${String(runOptions.warmupSeconds)}`,
      `--fixture-count ${String(runOptions.fixtureCount)}`,
      `--sample-interval-seconds ${String(runOptions.sampleIntervalSeconds)}`,
    ].join(" "),
    startedAt: startedAt.toISOString(),
    completedAt: new Date().toISOString(),
    durationSeconds: runOptions.durationSeconds,
    warmupSeconds: runOptions.warmupSeconds,
    fixtureCount: runOptions.fixtureCount,
    sampleIntervalSeconds: runOptions.sampleIntervalSeconds,
    gitCommit,
    environment,
    exit,
    accepted: failures.length === 0,
    failures,
    summary: {
      nativeSampleCount: resourceSamples.length,
      minimumNativeSampleCount: minimumExternalSamples,
      internalSampleCount: validInternalSamples.length,
      minimumInternalSampleCount: minimumInternalSamples,
      invalidInternalSampleCount,
      invalidExternalSampleCount,
      rssGrowthKiB,
      rssSlopeKiBPerMinute: round(rssSlopeKiBPerMinute, 2),
      maxRssKiB: maximum(resourceSamples, "rssKiB"),
      handleGrowth,
      maxHandles,
      threadBaseline,
      maxThreads,
      maxCpuPercent,
      scanPasses: finalInternal?.scanPasses ?? 0,
      generatedEvents: finalInternal?.generatedEvents ?? 0,
      watcherBatches: finalInternal?.watcherBatches ?? 0,
      thumbnailRequests: finalInternal?.thumbnailRequests ?? 0,
      hashRequests: finalInternal?.hashRequests ?? 0,
      cacheEntries: finalInternal?.cache.entryCount ?? 0,
      scheduler: finalInternal?.scheduler ?? null,
    },
    internalSamples,
    externalSamples: validExternalSamples,
    sampleParseErrors,
    stderr: stderr.trim(),
  };
}

export function assertResourceStabilityRuntime(nodeVersion) {
  const major = Number.parseInt(nodeVersion.split(".", 1)[0], 10);
  if (major !== 24) {
    throw new Error(
      `resource stability evidence requires Node.js 24.x, received ${nodeVersion}`,
    );
  }
}

export function linearSlope(samples, key) {
  if (samples.length < 2) return null;
  const xMean =
    samples.reduce((sum, sample) => sum + sample.elapsedMs, 0) / samples.length;
  const yMean =
    samples.reduce((sum, sample) => sum + sample[key], 0) / samples.length;
  let numerator = 0;
  let denominator = 0;
  for (const sample of samples) {
    const x = sample.elapsedMs - xMean;
    numerator += x * (sample[key] - yMean);
    denominator += x * x;
  }
  return denominator === 0 ? 0 : (numerator / denominator) * 60_000;
}

function isInternalSample(sample) {
  return (
    isRecord(sample) &&
    ["running", "complete"].includes(sample.status) &&
    isNonnegativeNumber(sample.elapsedMs) &&
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
    isNonnegativeInteger(sample.scheduler.foregroundLimit) &&
    isNonnegativeInteger(sample.scheduler.maxWaiters) &&
    isRecord(sample.cache) &&
    isNonnegativeInteger(sample.cache.entryCount) &&
    isNonnegativeInteger(sample.cache.maxEntries) &&
    isNonnegativeInteger(sample.cache.byteCount) &&
    isNonnegativeInteger(sample.cache.maxBytes)
  );
}

function isExternalSample(sample) {
  return (
    isRecord(sample) &&
    isNonnegativeNumber(sample.elapsedMs) &&
    isNonnegativeNumber(sample.rssKiB) &&
    isNonnegativeNumber(sample.cpuPercent) &&
    isNonnegativeInteger(sample.threads) &&
    isNonnegativeInteger(sample.handles)
  );
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonnegativeNumber(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function isNonnegativeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

function hasMonotonicElapsed(samples) {
  return samples.every(
    (sample, index) =>
      index === 0 || sample.elapsedMs >= samples[index - 1].elapsedMs,
  );
}

function minimumCoverageSamples(durationSeconds, intervalSeconds) {
  const expected =
    Math.floor(Math.max(0, durationSeconds) / intervalSeconds) + 1;
  return Math.max(2, Math.floor(expected * COVERAGE_RATIO));
}

function maximum(samples, key) {
  return samples.length === 0
    ? null
    : Math.max(...samples.map((sample) => sample[key]));
}

function minimum(samples, key) {
  return samples.length === 0
    ? null
    : Math.min(...samples.map((sample) => sample[key]));
}

function round(value, digits) {
  if (value === null) return null;
  const factor = 10 ** digits;
  return Math.round(value * factor) / factor;
}
