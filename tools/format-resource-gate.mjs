import { execFile, spawn } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";

import { writeJsonAtomic } from "./resource-stability-checkpoint.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const MAX_OUTPUT_BYTES = 1024 * 1024;
const DEFAULT_MAX_RSS_KIB = 512 * 1024;
const runFile = promisify(execFile);

export function analyzeFormatResourceRuns({
  reports,
  rssSamples,
  maxRssKiB = DEFAULT_MAX_RSS_KIB,
  repositoryState,
  providerProfile,
  environment,
}) {
  const failures = [];
  if (repositoryState.dirty)
    failures.push("repository was dirty before the gate");
  if (reports.length === 0) failures.push("no format gate run completed");
  const manifestDigests = new Set();
  for (const [index, report] of reports.entries()) {
    const label = `run ${String(index + 1)}`;
    if (report.accepted !== true) failures.push(`${label} was rejected`);
    if (report.fixtureCount !== 43 || report.checkedFixtureCount !== 43) {
      failures.push(`${label} did not check all 43 fixtures`);
    }
    if (report.adversarialFixtureCount !== 31) {
      failures.push(`${label} did not check all 31 adversarial fixtures`);
    }
    if (report.sourceDigestUnchanged !== true) {
      failures.push(`${label} changed source fixture bytes`);
    }
    if (report.cancellation?.accepted !== true) {
      failures.push(`${label} did not prove cooperative scan cancellation`);
    }
    if (report.providerProfile !== providerProfile) {
      failures.push(`${label} provider profile does not match the request`);
    }
    if (typeof report.manifestSha256 === "string") {
      manifestDigests.add(report.manifestSha256);
    }
  }
  if (manifestDigests.size !== 1) {
    failures.push("runs did not share exactly one manifest SHA-256");
  }
  const samples = rssSamples.filter(
    (sample) =>
      Number.isFinite(sample.rssKiB) &&
      sample.rssKiB >= 0 &&
      Number.isFinite(sample.elapsedMs) &&
      sample.elapsedMs >= 0,
  );
  if (samples.length < reports.length) {
    failures.push("process-tree RSS sampling was too sparse");
  }
  const observedMaxRssKiB = samples.reduce(
    (maximum, sample) => Math.max(maximum, sample.rssKiB),
    0,
  );
  if (observedMaxRssKiB > maxRssKiB) {
    failures.push(
      `process-tree RSS ${String(observedMaxRssKiB)} KiB exceeds ${String(maxRssKiB)} KiB`,
    );
  }
  return {
    schema: 1,
    accepted: failures.length === 0,
    gitCommit: repositoryState.gitCommit,
    environment,
    providerProfile,
    iterations: reports.length,
    fixtureCount: reports[0]?.fixtureCount ?? 0,
    adversarialFixtureCount: reports[0]?.adversarialFixtureCount ?? 0,
    manifestSha256: [...manifestDigests][0] ?? null,
    sourceDigestUnchangedEveryRun: reports.every(
      (report) => report.sourceDigestUnchanged === true,
    ),
    cancellationAcceptedEveryRun: reports.every(
      (report) => report.cancellation?.accepted === true,
    ),
    runs: reports,
    processTree: {
      sampleCount: samples.length,
      maxRssKiB: observedMaxRssKiB,
      limitRssKiB: maxRssKiB,
      samples,
    },
    failures,
  };
}

export function aggregateProcessTreeRss(rows, rootPid) {
  const included = new Set([rootPid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const row of rows) {
      if (included.has(row.parentPid) && !included.has(row.pid)) {
        included.add(row.pid);
        changed = true;
      }
    }
  }
  let rssKiB = 0;
  let processCount = 0;
  for (const row of rows) {
    if (!included.has(row.pid) || !Number.isFinite(row.rssKiB)) continue;
    rssKiB += row.rssKiB;
    processCount += 1;
  }
  return { rssKiB, processCount };
}

export function parseWindowsProcessRows(parsed) {
  return (Array.isArray(parsed) ? parsed : [parsed])
    .filter((row) => row !== null && typeof row === "object")
    .map((row) => ({
      pid: Number(row.ProcessId),
      parentPid: Number(row.ParentProcessId),
      rssKiB: Math.ceil(Number(row.WorkingSetSize) / 1024),
    }))
    .filter((row) =>
      [row.pid, row.parentPid, row.rssKiB].every(Number.isFinite),
    );
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  const repositoryState = await readRepositoryState();
  await runFile("cargo", ["build", "--release", "-p", "format-gate"], {
    cwd: repository,
  });
  const executable = path.join(
    repository,
    "target",
    "release",
    process.platform === "win32" ? "format-gate.exe" : "format-gate",
  );
  const reports = [];
  const rssSamples = [];
  for (let index = 0; index < options.iterations; index += 1) {
    const result = await runFormatGate({ executable, options });
    reports.push(result.report);
    rssSamples.push(
      ...result.rssSamples.map((sample) => ({
        ...sample,
        iteration: index + 1,
      })),
    );
  }
  const report = analyzeFormatResourceRuns({
    reports,
    rssSamples,
    maxRssKiB: options.maxRssKiB,
    repositoryState,
    providerProfile: options.providerProfile,
    environment: {
      platform: process.platform,
      architecture: process.arch,
      nodeVersion: process.version,
      cpus: os.cpus().length,
      totalMemoryBytes: os.totalmem(),
    },
  });
  const serialized = `${JSON.stringify(report, null, 2)}\n`;
  if (options.output !== undefined) {
    await writeJsonAtomic(options.output, report);
  }
  process.stdout.write(serialized);
  if (!report.accepted) process.exitCode = 1;
}

function parseArguments(arguments_) {
  const options = {
    providerProfile: "core-only",
    workerBundle: undefined,
    iterations: 3,
    maxRssKiB: DEFAULT_MAX_RSS_KIB,
    output: undefined,
  };
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    const value = arguments_[index + 1];
    if (argument === "--provider-profile") options.providerProfile = value;
    else if (argument === "--worker-bundle") options.workerBundle = value;
    else if (argument === "--iterations") options.iterations = Number(value);
    else if (argument === "--max-rss-kib") options.maxRssKiB = Number(value);
    else if (argument === "--output") options.output = path.resolve(value);
    else throw new Error(`unknown argument: ${argument}`);
    index += 1;
  }
  if (!Number.isSafeInteger(options.iterations) || options.iterations < 2) {
    throw new Error("--iterations must be an integer of at least 2");
  }
  if (!Number.isSafeInteger(options.maxRssKiB) || options.maxRssKiB <= 0) {
    throw new Error("--max-rss-kib must be a positive integer");
  }
  if (
    options.providerProfile === "core-only" &&
    options.workerBundle !== undefined
  ) {
    throw new Error("core-only profile cannot use --worker-bundle");
  }
  if (
    options.providerProfile === "bundled-codecs" &&
    options.workerBundle === undefined
  ) {
    throw new Error("bundled-codecs profile requires --worker-bundle");
  }
  if (!new Set(["core-only", "bundled-codecs"]).has(options.providerProfile)) {
    throw new Error(`unsupported provider profile: ${options.providerProfile}`);
  }
  return options;
}

async function runFormatGate({ executable, options }) {
  const arguments_ = [
    "--manifest",
    path.join(repository, "fixtures", "formats", "manifest.json"),
    "--provider-profile",
    options.providerProfile,
  ];
  if (options.workerBundle !== undefined) {
    arguments_.push("--worker-bundle", path.resolve(options.workerBundle));
  }
  const windowsSampler =
    process.platform === "win32"
      ? await startWindowsProcessSampler()
      : undefined;
  const child = spawn(executable, arguments_, {
    cwd: repository,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const started = performance.now();
  const stdout = [];
  const stderr = [];
  let outputBytes = 0;
  const capture = (target) => (chunk) => {
    outputBytes += chunk.length;
    if (outputBytes > MAX_OUTPUT_BYTES) {
      child.kill();
      return;
    }
    target.push(chunk);
  };
  child.stdout.on("data", capture(stdout));
  child.stderr.on("data", capture(stderr));
  const rssSamples = [];
  const pendingSamples = new Set();
  const sample = () => {
    const pending = sampleProcessTree(child.pid)
      .then((tree) => {
        if (tree.processCount > 0) {
          rssSamples.push({
            elapsedMs: Math.round(performance.now() - started),
            rssKiB: tree.rssKiB,
            processCount: tree.processCount,
          });
        }
      })
      .catch(() => {});
    pendingSamples.add(pending);
    void pending.finally(() => pendingSamples.delete(pending));
  };
  let timer;
  if (windowsSampler === undefined) {
    sample();
    timer = setInterval(sample, 20);
  } else {
    windowsSampler.observe(child.pid, started, rssSamples);
  }
  let exit;
  try {
    exit = await waitForExit(child);
  } finally {
    if (timer !== undefined) clearInterval(timer);
    await Promise.allSettled([...pendingSamples]);
    if (windowsSampler !== undefined) await windowsSampler.finish();
  }
  if (outputBytes > MAX_OUTPUT_BYTES) {
    throw new Error("format gate output exceeded 1 MiB");
  }
  const diagnostic = Buffer.concat(stderr).toString("utf8").slice(-2000);
  if (exit.code !== 0 || exit.signal !== null) {
    throw new Error(
      `format gate failed with code=${String(exit.code)} signal=${String(exit.signal)}: ${diagnostic}`,
    );
  }
  let report;
  try {
    report = JSON.parse(Buffer.concat(stdout).toString("utf8"));
  } catch (error) {
    throw new Error(
      `format gate JSON is invalid: ${String(error)}: ${diagnostic}`,
    );
  }
  return { report, rssSamples };
}

async function sampleProcessTree(rootPid) {
  const rows = await unixProcessRows();
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

async function startWindowsProcessSampler() {
  const command = [
    "$ErrorActionPreference = 'Stop'",
    "$null = @(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,WorkingSetSize)",
    "[Console]::Out.WriteLine('READY')",
    "[Console]::Out.Flush()",
    "$rootProcessId = [int][Console]::In.ReadLine()",
    "$seenRoot = $false",
    "while ($true) {",
    "$rows = @(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,WorkingSetSize)",
    "$rootPresent = @($rows | Where-Object { $_.ProcessId -eq $rootProcessId }).Count -gt 0",
    "if ($rootPresent) { $seenRoot = $true }",
    "[Console]::Out.WriteLine(($rows | ConvertTo-Json -Compress -Depth 2))",
    "[Console]::Out.Flush()",
    "if ($seenRoot -and -not $rootPresent) { break }",
    "Start-Sleep -Milliseconds 10",
    "}",
    "[Console]::Out.WriteLine('DONE')",
    "[Console]::Out.Flush()",
  ].join("\n");
  const sampler = spawn(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", command],
    { stdio: ["pipe", "pipe", "pipe"] },
  );
  let buffer = "";
  let diagnostic = "";
  let rootPid;
  let started;
  let targetSamples;
  let readyResolve;
  let readyReject;
  let doneResolve;
  const ready = new Promise((resolve, reject) => {
    readyResolve = resolve;
    readyReject = reject;
  });
  const done = new Promise((resolve) => {
    doneResolve = resolve;
  });
  sampler.stderr.on("data", (chunk) => {
    diagnostic = `${diagnostic}${chunk.toString("utf8")}`.slice(-2000);
  });
  sampler.stdout.on("data", (chunk) => {
    buffer += chunk.toString("utf8");
    if (buffer.length > MAX_OUTPUT_BYTES) {
      diagnostic = "Windows process sampler output line exceeded 1 MiB";
      sampler.kill();
      readyReject(new Error(diagnostic));
      return;
    }
    let newline;
    while ((newline = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, newline).trim();
      buffer = buffer.slice(newline + 1);
      if (line === "READY") {
        readyResolve();
      } else if (line === "DONE") {
        doneResolve();
      } else if (line !== "" && rootPid !== undefined) {
        try {
          const tree = aggregateProcessTreeRss(
            parseWindowsProcessRows(JSON.parse(line)),
            rootPid,
          );
          if (tree.processCount > 0) {
            targetSamples.push({
              elapsedMs: Math.round(performance.now() - started),
              rssKiB: tree.rssKiB,
              processCount: tree.processCount,
            });
          }
        } catch (error) {
          diagnostic = `invalid sampler JSON: ${String(error)}`;
        }
      }
    }
  });
  sampler.once("error", (error) => {
    readyReject(error);
    doneResolve();
  });
  sampler.once("exit", (code, signal) => {
    if (rootPid === undefined) {
      readyReject(
        new Error(
          `Windows process sampler exited before READY: code=${String(code)} signal=${String(signal)} ${diagnostic}`,
        ),
      );
    }
    doneResolve();
  });
  await withTimeout(
    ready,
    5_000,
    "Windows process sampler did not become ready",
  );
  return {
    observe(processId, observedStarted, samples) {
      rootPid = processId;
      started = observedStarted;
      targetSamples = samples;
      sampler.stdin.end(`${String(processId)}\n`);
    },
    async finish() {
      const completed = await settlesWithin(done, 2_000);
      if (!completed) {
        sampler.kill();
        await settlesWithin(done, 2_000);
      }
      if (diagnostic !== "") {
        throw new Error(`Windows process sampler failed: ${diagnostic}`);
      }
    },
  };
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

async function settlesWithin(promise, milliseconds) {
  let timer;
  try {
    return await Promise.race([
      promise.then(() => true),
      new Promise((resolve) => {
        timer = setTimeout(() => resolve(false), milliseconds);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function waitForExit(child) {
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal }));
  });
}

async function readRepositoryState() {
  const [{ stdout: revision }, { stdout: status }] = await Promise.all([
    runFile("git", ["rev-parse", "HEAD"], { cwd: repository }),
    runFile("git", ["status", "--porcelain"], { cwd: repository }),
  ]);
  return {
    gitCommit: revision.trim(),
    dirty: status.trim() !== "",
  };
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  await main();
}
