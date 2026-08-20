import { execFile, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

import {
  aggregateProcessTreeRss,
  parseWindowsProcessRows,
} from "./format-resource-gate.mjs";
import { writeJsonAtomic } from "./resource-stability-checkpoint.mjs";

const runFile = promisify(execFile);
const repository = path.resolve(import.meta.dirname, "..");
const MAX_OUTPUT_BYTES = 16 * 1024 * 1024;
const DEFAULT_RECORDS = 100_000;
const DEFAULT_ITERATIONS = 200;
const DEFAULT_CASE = "combined-advanced";
const MAX_P95_NANOSECONDS = 100_000_000;
const MAX_RSS_KIB = 1024 * 1024;

export function analyzeQueryPerformance({
  productReport,
  rssSamples,
  baselineRssKiB,
  repositoryState,
  manifestSha256,
  manifestDigestUnchanged,
  environment,
  options,
}) {
  const failures = [];
  const performance = productReport.performance;
  if (repositoryState.dirty)
    failures.push("repository was dirty before the gate");
  if (!manifestDigestUnchanged)
    failures.push("query manifest changed during the gate");
  if (productReport.schema !== 1)
    failures.push("product report schema is not 1");
  if (performance === null || typeof performance !== "object") {
    failures.push("product performance report is missing");
  }
  if (performance?.caseId !== options.caseId)
    failures.push("performance case does not match");
  if (performance?.recordCount !== options.recordCount)
    failures.push("performance record count does not match");
  if (performance?.iterations !== options.iterations)
    failures.push("performance iteration count does not match");
  const timings = Array.isArray(performance?.samplesNanoseconds)
    ? performance.samplesNanoseconds.filter(
        (sample) => Number.isSafeInteger(sample) && sample >= 0,
      )
    : [];
  if (timings.length !== options.iterations) {
    failures.push("raw performance timing sample count does not match");
  }
  const sorted = [...timings].sort((left, right) => left - right);
  const replayedP50 = percentile(sorted, 50);
  const replayedP95 = percentile(sorted, 95);
  const replayedMax = sorted.at(-1) ?? 0;
  if (
    replayedP50 !== performance?.p50Nanoseconds ||
    replayedP95 !== performance?.p95Nanoseconds ||
    replayedMax !== performance?.maxNanoseconds
  ) {
    failures.push("stored latency summary does not replay from raw samples");
  }
  if (replayedP95 > MAX_P95_NANOSECONDS) {
    failures.push("query p95 exceeds 100 ms");
  }
  const samples = rssSamples.filter(
    (sample) =>
      Number.isFinite(sample.elapsedMs) &&
      sample.elapsedMs >= 0 &&
      Number.isFinite(sample.rssKiB) &&
      sample.rssKiB >= 0 &&
      Number.isInteger(sample.processCount) &&
      sample.processCount > 0,
  );
  if (samples.length < 10)
    failures.push("process-tree RSS sampling was too sparse");
  if (!Number.isFinite(baselineRssKiB) || baselineRssKiB <= 0) {
    failures.push("process-tree baseline RSS is missing");
  }
  const maxRssKiB = samples.reduce(
    (maximum, sample) => Math.max(maximum, sample.rssKiB),
    baselineRssKiB,
  );
  if (maxRssKiB > MAX_RSS_KIB) failures.push("process-tree RSS exceeds 1 GiB");
  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    gitCommit: repositoryState.gitCommit,
    manifestSha256,
    manifestDigestUnchanged,
    environment,
    options,
    resultCount: performance?.resultCount ?? null,
    indexBuildNanoseconds: performance?.indexBuildNanoseconds ?? null,
    latency: {
      sampleCount: timings.length,
      p50Nanoseconds: replayedP50,
      p95Nanoseconds: replayedP95,
      maxNanoseconds: replayedMax,
      limitP95Nanoseconds: MAX_P95_NANOSECONDS,
      samplesNanoseconds: timings,
    },
    processTree: {
      baselineRssKiB,
      sampleCount: samples.length,
      maxRssKiB,
      deltaRssKiB: Math.max(0, maxRssKiB - baselineRssKiB),
      limitRssKiB: MAX_RSS_KIB,
      samples,
    },
  };
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  if (Number(process.versions.node.split(".")[0]) !== 24) {
    throw new Error("query performance gate requires Node.js 24");
  }
  const repositoryState = await readRepositoryState();
  const before = await readFile(options.manifest);
  const manifestSha256 = createHash("sha256").update(before).digest("hex");
  await runFile("cargo", ["build", "--release", "-p", "query-gate"], {
    cwd: repository,
  });
  const executable = path.join(
    repository,
    "target",
    "release",
    process.platform === "win32" ? "query-gate.exe" : "query-gate",
  );
  const run = await runProductPerformance({ executable, options });
  const after = await readFile(options.manifest);
  const report = analyzeQueryPerformance({
    productReport: run.productReport,
    rssSamples: run.rssSamples,
    baselineRssKiB: run.baselineRssKiB,
    repositoryState,
    manifestSha256,
    manifestDigestUnchanged: before.equals(after),
    environment: {
      platform: process.platform,
      architecture: process.arch,
      nodeVersion: process.version,
      cpus: os.cpus().length,
      totalMemoryBytes: os.totalmem(),
    },
    options: {
      recordCount: options.recordCount,
      iterations: options.iterations,
      caseId: options.caseId,
    },
  });
  if (options.output !== undefined)
    await writeJsonAtomic(options.output, report);
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  if (!report.accepted) process.exitCode = 1;
}

async function runProductPerformance({ executable, options }) {
  const child = spawn(
    executable,
    [
      "--manifest",
      options.manifest,
      "--performance-records",
      String(options.recordCount),
      "--performance-iterations",
      String(options.iterations),
      "--performance-case",
      options.caseId,
      "--wait-for-sampler",
    ],
    { cwd: repository, stdio: ["pipe", "pipe", "pipe"] },
  );
  const stdout = [];
  let outputBytes = 0;
  child.stdout.on("data", (chunk) => {
    outputBytes += chunk.length;
    if (outputBytes > MAX_OUTPUT_BYTES) child.kill();
    else stdout.push(chunk);
  });
  let stderr = "";
  let readyResolve;
  let readyReject;
  const ready = new Promise((resolve, reject) => {
    readyResolve = resolve;
    readyReject = reject;
  });
  child.stderr.on("data", (chunk) => {
    stderr = `${stderr}${chunk.toString("utf8")}`.slice(-4000);
    if (stderr.includes("QUERY_GATE_READY")) readyResolve();
  });
  child.once("error", readyReject);
  const exitPromise = waitForExit(child);
  await withTimeout(
    ready,
    10_000,
    "query gate did not become ready for sampling",
  );
  const baseline = await sampleProcessTree(child.pid);
  if (baseline.processCount === 0)
    throw new Error("query gate baseline process was not sampled");
  const rssSamples = [];
  const started = performance.now();
  let running = true;
  const sampling = (async () => {
    while (running) {
      try {
        const tree = await sampleProcessTree(child.pid);
        if (tree.processCount > 0) {
          rssSamples.push({
            elapsedMs: Math.round(performance.now() - started),
            rssKiB: tree.rssKiB,
            processCount: tree.processCount,
          });
        }
      } catch {}
      await delay(process.platform === "win32" ? 250 : 50);
    }
  })();
  child.stdin.end("START\n");
  const exit = await exitPromise;
  running = false;
  await sampling;
  if (outputBytes > MAX_OUTPUT_BYTES)
    throw new Error("query gate output exceeded 16 MiB");
  if (exit.code !== 0 || exit.signal !== null) {
    throw new Error(
      `query gate failed with code=${String(exit.code)} signal=${String(exit.signal)}: ${stderr}`,
    );
  }
  return {
    productReport: JSON.parse(Buffer.concat(stdout).toString("utf8")),
    baselineRssKiB: baseline.rssKiB,
    rssSamples,
  };
}

async function sampleProcessTree(rootPid) {
  const rows =
    process.platform === "win32"
      ? await windowsProcessRows()
      : await unixProcessRows();
  return aggregateProcessTreeRss(rows, rootPid);
}

async function unixProcessRows() {
  const { stdout } = await runFile("ps", ["-axo", "pid=,ppid=,rss="]);
  return stdout
    .trim()
    .split("\n")
    .map((line) => line.trim().split(/\s+/u).map(Number))
    .filter((values) => values.length === 3 && values.every(Number.isFinite))
    .map(([pid, parentPid, rssKiB]) => ({ pid, parentPid, rssKiB }));
}

async function windowsProcessRows() {
  const { stdout } = await runFile("powershell.exe", [
    "-NoProfile",
    "-NonInteractive",
    "-Command",
    "Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,WorkingSetSize | ConvertTo-Json -Compress",
  ]);
  return parseWindowsProcessRows(JSON.parse(stdout));
}

async function readRepositoryState() {
  const [{ stdout: revision }, { stdout: status }] = await Promise.all([
    runFile("git", ["rev-parse", "HEAD"], { cwd: repository }),
    runFile("git", ["status", "--porcelain"], { cwd: repository }),
  ]);
  return { gitCommit: revision.trim(), dirty: status.trim() !== "" };
}

function percentile(sorted, requested) {
  const rank = Math.max(0, Math.ceil((sorted.length * requested) / 100) - 1);
  return sorted[rank] ?? 0;
}

function parseArguments(arguments_) {
  const options = {
    recordCount: DEFAULT_RECORDS,
    iterations: DEFAULT_ITERATIONS,
    caseId: DEFAULT_CASE,
  };
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === "--manifest")
      options.manifest = path.resolve(arguments_[++index]);
    else if (argument === "--output")
      options.output = path.resolve(arguments_[++index]);
    else if (argument === "--records")
      options.recordCount = Number(arguments_[++index]);
    else if (argument === "--iterations")
      options.iterations = Number(arguments_[++index]);
    else if (argument === "--case") options.caseId = arguments_[++index];
    else throw new Error(`unknown argument: ${String(argument)}`);
  }
  if (options.manifest === undefined) throw new Error("--manifest is required");
  if (
    !Number.isInteger(options.recordCount) ||
    options.recordCount !== DEFAULT_RECORDS
  ) {
    throw new Error(`--records must be ${String(DEFAULT_RECORDS)}`);
  }
  if (
    !Number.isInteger(options.iterations) ||
    options.iterations !== DEFAULT_ITERATIONS
  ) {
    throw new Error(`--iterations must be ${String(DEFAULT_ITERATIONS)}`);
  }
  if (options.caseId !== DEFAULT_CASE)
    throw new Error(`--case must be ${DEFAULT_CASE}`);
  return options;
}

function waitForExit(child) {
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal }));
  });
}

async function withTimeout(promise, milliseconds, message) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), milliseconds);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  await main();
}
