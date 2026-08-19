import { spawn } from "node:child_process";
import { execFile } from "node:child_process";
import { mkdtemp, mkdir, readdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";

const runFile = promisify(execFile);
const repository = path.resolve(import.meta.dirname, "..");
const defaults = {
  durationSeconds: 28_800,
  warmupSeconds: 60,
  fixtureCount: 100_000,
  sampleIntervalSeconds: 5,
  output: path.join(
    repository,
    "docs",
    "reports",
    "evidence",
    "p2-06-resource-soak.json",
  ),
};
const options = parseArguments(process.argv.slice(2));
let workspace;
let child;

try {
  workspace = await mkdtemp(path.join(os.tmpdir(), "material-eagle-p2-soak-"));
  const library = path.join(workspace, "library");
  const cache = path.join(workspace, "cache");
  await run("cargo", [
    "build",
    "--release",
    "-p",
    "fixture-generator",
    "-p",
    "resource-soak",
  ]);
  await run(path.join(repository, "target", "release", "fixture-generator"), [
    "generate",
    library,
    "--count",
    String(options.fixtureCount),
  ]);

  const internalSamples = [];
  const externalSamples = [];
  const sampleParseErrors = [];
  const stderr = [];
  const startedAt = new Date();
  const monotonicStarted = performance.now();
  child = spawn(
    path.join(repository, "target", "release", "resource-soak"),
    [
      library,
      cache,
      "--duration-seconds",
      String(options.durationSeconds),
      "--sample-interval-ms",
      String(options.sampleIntervalSeconds * 1_000),
      "--mode-interval-seconds",
      String(
        Math.max(1, Math.min(60, Math.floor(options.durationSeconds / 4))),
      ),
    ],
    { cwd: repository, stdio: ["ignore", "pipe", "pipe"] },
  );
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => stderr.push(chunk));
  consumeLines(child.stdout, (line) => {
    try {
      const sample = JSON.parse(line);
      internalSamples.push(sample);
      if (sample.status === "running") {
        console.log(
          [
            "Resource stability progress",
            `elapsedMs=${String(sample.elapsedMs)}`,
            `scans=${String(sample.scanPasses)}`,
            `events=${String(sample.generatedEvents)}`,
            `cacheEntries=${String(sample.cache.entryCount)}`,
            `active=${String(sample.scheduler.activeTotal)}`,
            `waiting=${String(sample.scheduler.waitingTotal)}`,
          ].join(" "),
        );
      }
    } catch (error) {
      sampleParseErrors.push(`${String(error)}: ${line.slice(0, 200)}`);
    }
  });
  const pendingExternalSamples = new Set();
  const captureExternalSample = () => {
    const task = sampleProcess(
      child.pid,
      performance.now() - monotonicStarted,
    ).then((sample) => {
      if (sample !== undefined) externalSamples.push(sample);
    });
    pendingExternalSamples.add(task);
    void task.finally(() => pendingExternalSamples.delete(task));
  };
  const externalTimer = setInterval(
    captureExternalSample,
    options.sampleIntervalSeconds * 1_000,
  );
  const firstExternal = await sampleProcess(child.pid, 0);
  if (firstExternal !== undefined) externalSamples.push(firstExternal);
  const exit = await waitForExit(child);
  clearInterval(externalTimer);
  await Promise.allSettled([...pendingExternalSamples]);
  const finalExternal = await sampleProcess(
    child.pid,
    performance.now() - monotonicStarted,
  );
  if (finalExternal !== undefined) externalSamples.push(finalExternal);

  const report = buildReport({
    startedAt,
    exit,
    stderr: stderr.join(""),
    internalSamples,
    externalSamples,
    sampleParseErrors,
    options,
  });
  await mkdir(path.dirname(options.output), { recursive: true });
  await writeFile(options.output, `${JSON.stringify(report, null, 2)}\n`);
  if (!report.accepted) {
    throw new Error(
      `resource stability rejected: ${report.failures.join("; ")}`,
    );
  }
  console.log(`Resource stability accepted ${JSON.stringify(report.summary)}`);
  console.log(`Evidence written to ${options.output}`);
} finally {
  if (child !== undefined && child.exitCode === null) child.kill("SIGTERM");
  if (workspace !== undefined) {
    await rm(workspace, { recursive: true, force: true, maxRetries: 3 });
  }
}

function buildReport({
  startedAt,
  exit,
  stderr,
  internalSamples,
  externalSamples,
  sampleParseErrors,
  options: runOptions,
}) {
  externalSamples.sort((left, right) => left.elapsedMs - right.elapsedMs);
  const measured = externalSamples.filter(
    (sample) => sample.elapsedMs >= runOptions.warmupSeconds * 1_000,
  );
  const resourceSamples = measured.length >= 2 ? measured : externalSamples;
  const first = resourceSamples.at(0);
  const last = resourceSamples.at(-1);
  const finalInternal = internalSamples.at(-1);
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
  const failures = [];
  if (exit.code !== 0)
    failures.push(`resource soak exited with code ${String(exit.code)}`);
  if (finalInternal?.status !== "complete")
    failures.push("resource soak did not emit a complete sample");
  if (sampleParseErrors.length > 0) {
    failures.push(
      `${String(sampleParseErrors.length)} internal samples were not valid JSON`,
    );
  }
  if (finalInternal?.sourceAssets !== runOptions.fixtureCount) {
    failures.push(
      `scanned ${String(finalInternal?.sourceAssets)} assets, expected ${String(runOptions.fixtureCount)}`,
    );
  }
  if (resourceSamples.length < 2)
    failures.push("fewer than two native process samples were captured");
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
  for (const sample of internalSamples) {
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
    !internalSamples.some((sample) => sample.scheduler.mode === "background")
  ) {
    failures.push("background resource mode was not observed");
  }
  return {
    schema: 1,
    command: `node tools/verify-resource-stability.mjs --duration-seconds ${String(runOptions.durationSeconds)} --fixture-count ${String(runOptions.fixtureCount)}`,
    startedAt: startedAt.toISOString(),
    completedAt: new Date().toISOString(),
    durationSeconds: runOptions.durationSeconds,
    warmupSeconds: runOptions.warmupSeconds,
    fixtureCount: runOptions.fixtureCount,
    accepted: failures.length === 0,
    failures,
    summary: {
      nativeSampleCount: resourceSamples.length,
      internalSampleCount: internalSamples.length,
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
    externalSamples,
    sampleParseErrors,
    stderr: stderr.trim(),
  };
}

async function sampleProcess(pid, elapsedMs) {
  if (pid === undefined) return undefined;
  try {
    const psArguments = ["-o", "rss=", "-o", "%cpu="];
    if (process.platform !== "darwin") psArguments.push("-o", "nlwp=");
    psArguments.push("-p", String(pid));
    const { stdout } = await runFile("ps", psArguments);
    const values = stdout.trim().split(/\s+/u).map(Number);
    const [rss, cpu] = values;
    const threads =
      process.platform === "darwin" ? await darwinThreadCount(pid) : values[2];
    return {
      elapsedMs: Math.round(elapsedMs),
      rssKiB: rss,
      cpuPercent: cpu,
      threads,
      handles: await handleCount(pid),
    };
  } catch {
    return undefined;
  }
}

async function darwinThreadCount(pid) {
  const { stdout } = await runFile("ps", ["-M", "-p", String(pid)]);
  return Math.max(0, stdout.trim().split("\n").length - 1);
}

async function handleCount(pid) {
  if (process.platform === "linux") {
    return (await readdir(`/proc/${String(pid)}/fd`)).length;
  }
  const { stdout } = await runFile("lsof", ["-n", "-P", "-p", String(pid)]);
  return Math.max(0, stdout.trim().split("\n").length - 1);
}

function consumeLines(stream, receive) {
  stream.setEncoding("utf8");
  let pending = "";
  stream.on("data", (chunk) => {
    pending += chunk;
    const lines = pending.split("\n");
    pending = lines.pop() ?? "";
    for (const line of lines) if (line.trim() !== "") receive(line);
  });
  stream.on("end", () => {
    if (pending.trim() !== "") receive(pending);
  });
}

function waitForExit(processHandle) {
  return new Promise((resolve, reject) => {
    processHandle.once("error", reject);
    processHandle.once("exit", (code, signal) => resolve({ code, signal }));
  });
}

async function run(command, args) {
  await runFile(command, args, {
    cwd: repository,
    maxBuffer: 16 * 1_024 * 1_024,
  });
}

function linearSlope(samples, key) {
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

function parseArguments(args) {
  const parsed = { ...defaults };
  for (let index = 0; index < args.length; index += 2) {
    const value = args[index + 1];
    if (value === undefined)
      throw new Error(`missing value for ${args[index]}`);
    switch (args[index]) {
      case "--duration-seconds":
        parsed.durationSeconds = positiveInteger(value, args[index]);
        break;
      case "--warmup-seconds":
        parsed.warmupSeconds = nonnegativeInteger(value, args[index]);
        break;
      case "--fixture-count":
        parsed.fixtureCount = positiveInteger(value, args[index]);
        break;
      case "--sample-interval-seconds":
        parsed.sampleIntervalSeconds = positiveInteger(value, args[index]);
        break;
      case "--output":
        parsed.output = path.resolve(value);
        break;
      default:
        throw new Error(`unknown argument ${args[index]}`);
    }
  }
  return parsed;
}

function positiveInteger(value, name) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0)
    throw new Error(`${name} must be a positive integer`);
  return parsed;
}

function nonnegativeInteger(value, name) {
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < 0)
    throw new Error(`${name} must be a nonnegative integer`);
  return parsed;
}
