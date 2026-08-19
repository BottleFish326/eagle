import { spawn } from "node:child_process";
import { execFile } from "node:child_process";
import {
  mkdtemp,
  mkdir,
  readdir,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { promisify } from "node:util";

import { buildResourceStabilityReport } from "./resource-stability-analysis.mjs";

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
const repositoryState = await readRepositoryState();
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

  const report = buildResourceStabilityReport({
    startedAt,
    exit,
    stderr: stderr.join(""),
    internalSamples,
    externalSamples,
    sampleParseErrors,
    options,
    gitCommit: repositoryState.gitCommit,
    environment: {
      platform: process.platform,
      architecture: process.arch,
      nodeVersion: process.version,
    },
  });
  await mkdir(path.dirname(options.output), { recursive: true });
  await writeReportAtomic(options.output, report);
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

async function readRepositoryState() {
  const [{ stdout: revision }, { stdout: status }] = await Promise.all([
    runFile("git", ["rev-parse", "HEAD"], { cwd: repository }),
    runFile("git", ["status", "--porcelain", "--untracked-files=normal"], {
      cwd: repository,
    }),
  ]);
  if (status.trim() !== "") {
    throw new Error(
      "resource stability evidence requires a clean Git worktree; commit or remove local changes first",
    );
  }
  return { gitCommit: revision.trim() };
}

async function writeReportAtomic(output, report) {
  const temporary = `${output}.tmp-${String(process.pid)}-${String(Date.now())}`;
  try {
    await writeFile(temporary, `${JSON.stringify(report, null, 2)}\n`, {
      flag: "wx",
    });
    await rename(temporary, output);
  } finally {
    await rm(temporary, { force: true });
  }
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
