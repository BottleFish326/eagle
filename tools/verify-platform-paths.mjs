import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdir, rename, writeFile } from "node:fs/promises";
import path from "node:path";

import {
  inspectHostedEvidenceContext,
  inspectPlatformPathRun,
} from "./platform-path-evidence.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const outputPath = parseOutputPath(process.argv.slice(2));
const startedAt = new Date();
let report;

try {
  assertNode24();
  const repositoryState = readRepositoryState();
  const environment = readEnvironment(repositoryState.gitCommit);
  const command = [
    "cargo",
    "test",
    "--locked",
    "-p",
    "asset-filesystem",
    "p2_platform",
  ];
  const list = run(command[0], [...command.slice(1), "--", "--list"]);
  const test = run(command[0], [...command.slice(1), "--", "--nocapture"]);
  const inspection = inspectPlatformPathRun({
    platform: process.platform,
    listStatus: list.status,
    listOutput: `${list.stdout}\n${list.stderr}`,
    testStatus: test.status,
    testOutput: `${test.stdout}\n${test.stderr}`,
    requireWindowsSymlink:
      process.env.MATERIAL_EAGLE_REQUIRE_WINDOWS_SYMLINK === "1",
  });
  const contextFailures = inspectHostedEvidenceContext({
    platform: process.platform,
    gitCommit: repositoryState.gitCommit,
    environment,
  });
  report = {
    schema: 1,
    accepted: inspection.accepted && contextFailures.length === 0,
    failures: [...contextFailures, ...inspection.failures],
    startedAt: startedAt.toISOString(),
    completedAt: new Date().toISOString(),
    gitCommit: repositoryState.gitCommit,
    command: `${command.join(" ")} -- --nocapture`,
    environment,
    requireWindowsSymlink:
      process.env.MATERIAL_EAGLE_REQUIRE_WINDOWS_SYMLINK === "1",
    expectedTests: inspection.expectedTests,
    listedTests: inspection.listedTests,
    executedTests: inspection.executedTests,
    summary: inspection.summary,
    processResults: {
      list: serializeProcessResult(list),
      test: serializeProcessResult(test),
    },
  };
} catch (error) {
  report = {
    schema: 1,
    accepted: false,
    failures: [error instanceof Error ? error.message : String(error)],
    startedAt: startedAt.toISOString(),
    completedAt: new Date().toISOString(),
    gitCommit: safeGitCommit(),
    command:
      "cargo test --locked -p asset-filesystem p2_platform -- --nocapture",
    environment: safeEnvironment(),
  };
}

await writeJsonAtomic(outputPath, report);
console.log(JSON.stringify(report, null, 2));
if (!report.accepted) process.exitCode = 1;

function parseOutputPath(args) {
  if (args.length !== 2 || args[0] !== "--output") {
    throw new Error(
      "usage: node tools/verify-platform-paths.mjs --output <path>",
    );
  }
  return path.resolve(args[1]);
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24) {
    throw new Error(
      `P2-A12 evidence requires Node.js 24.x, received ${process.version}`,
    );
  }
}

function readRepositoryState() {
  const revision = run("git", ["rev-parse", "HEAD"]);
  if (revision.status !== 0)
    throw new Error(`cannot read Git commit: ${diagnostic(revision)}`);
  const status = run("git", [
    "status",
    "--porcelain",
    "--untracked-files=normal",
  ]);
  if (status.status !== 0)
    throw new Error(`cannot inspect Git worktree: ${diagnostic(status)}`);
  if (status.stdout.trim() !== "") {
    throw new Error("P2-A12 evidence requires a clean Git worktree");
  }
  return { gitCommit: revision.stdout.trim() };
}

function readEnvironment(gitCommit) {
  return {
    platform: process.platform,
    architecture: process.arch,
    nodeVersion: process.version,
    rustc: commandVersion("rustc", ["-Vv"]),
    cargo: commandVersion("cargo", ["-V"]),
    githubActions: process.env.GITHUB_ACTIONS ?? null,
    githubSha: process.env.GITHUB_SHA ?? null,
    githubRunId: process.env.GITHUB_RUN_ID ?? null,
    githubRunAttempt: process.env.GITHUB_RUN_ATTEMPT ?? null,
    githubWorkflowRef: process.env.GITHUB_WORKFLOW_REF ?? null,
    githubRepository: process.env.GITHUB_REPOSITORY ?? null,
    githubServerUrl: process.env.GITHUB_SERVER_URL ?? null,
    runnerOs: process.env.RUNNER_OS ?? null,
    runnerArch: process.env.RUNNER_ARCH ?? null,
    runnerEnvironment: process.env.MATERIAL_EAGLE_RUNNER_ENVIRONMENT ?? null,
    gitCommit,
  };
}

function safeGitCommit() {
  const result = run("git", ["rev-parse", "HEAD"]);
  return result.status === 0 ? result.stdout.trim() : null;
}

function safeEnvironment() {
  return {
    platform: process.platform,
    architecture: process.arch,
    nodeVersion: process.version,
    githubActions: process.env.GITHUB_ACTIONS ?? null,
    githubSha: process.env.GITHUB_SHA ?? null,
    githubRunId: process.env.GITHUB_RUN_ID ?? null,
    githubRunAttempt: process.env.GITHUB_RUN_ATTEMPT ?? null,
    githubWorkflowRef: process.env.GITHUB_WORKFLOW_REF ?? null,
    githubRepository: process.env.GITHUB_REPOSITORY ?? null,
    githubServerUrl: process.env.GITHUB_SERVER_URL ?? null,
    runnerOs: process.env.RUNNER_OS ?? null,
    runnerArch: process.env.RUNNER_ARCH ?? null,
    runnerEnvironment: process.env.MATERIAL_EAGLE_RUNNER_ENVIRONMENT ?? null,
  };
}

function commandVersion(command, args) {
  const result = run(command, args);
  if (result.status !== 0)
    throw new Error(`cannot read ${command} version: ${diagnostic(result)}`);
  return result.stdout.trim();
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: repository,
    encoding: "utf8",
    env: { ...process.env, CARGO_TERM_COLOR: "never" },
    maxBuffer: 16 * 1024 * 1024,
  });
  return {
    status: result.status ?? -1,
    signal: result.signal ?? null,
    error: result.error?.message ?? null,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
  };
}

function serializeProcessResult(result) {
  return {
    status: result.status,
    signal: result.signal,
    error: result.error,
    stdout: result.stdout.trim(),
    stderr: result.stderr.trim(),
  };
}

function diagnostic(result) {
  return (
    result.error ||
    result.stderr.trim() ||
    result.stdout.trim() ||
    `process exited with status ${String(result.status)}`
  );
}

async function writeJsonAtomic(destination, value) {
  await mkdir(path.dirname(destination), { recursive: true });
  const temporary = `${destination}.${randomUUID()}.tmp`;
  await writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`, {
    encoding: "utf8",
    flag: "wx",
  });
  await rename(temporary, destination);
}
