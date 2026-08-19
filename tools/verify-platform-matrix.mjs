import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  lstat,
  mkdir,
  readFile,
  readdir,
  rename,
  writeFile,
} from "node:fs/promises";
import path from "node:path";

import { buildPlatformMatrixReport } from "./platform-matrix-analysis.mjs";

const repository = path.resolve(import.meta.dirname, "..");
let outputPath;
let report;

try {
  const options = parseArguments(process.argv.slice(2));
  outputPath = options.outputPath;
  assertNode24();
  const gitCommit = readRepositoryState();
  const evidencePaths = await findEvidenceFiles(options.inputDirectory);
  const sources = [];
  for (const evidencePath of evidencePaths) {
    const bytes = await readBoundedFile(evidencePath);
    const relative = path.relative(options.inputDirectory, evidencePath);
    const components = relative.split(path.sep);
    sources.push({
      artifactName: components.length === 2 ? components[0] : null,
      fileName: path.basename(evidencePath),
      sha256: createHash("sha256").update(bytes).digest("hex"),
      report: JSON.parse(bytes.toString("utf8")),
    });
  }
  report = buildPlatformMatrixReport({
    sources,
    repositoryCommit: gitCommit,
    nodeVersion: process.version,
    verifiedAt: new Date().toISOString(),
    workflowContext: readWorkflowContext(),
  });
} catch (error) {
  report = {
    schema: 1,
    accepted: false,
    failures: [error instanceof Error ? error.message : String(error)],
    verifiedAt: new Date().toISOString(),
    gitCommit: safeGitCommit(),
    workflow: {
      githubRunId: process.env.GITHUB_RUN_ID ?? null,
      githubRunAttempt: process.env.GITHUB_RUN_ATTEMPT ?? null,
      githubWorkflowRef: process.env.GITHUB_WORKFLOW_REF ?? null,
    },
    verificationEnvironment: {
      nodeVersion: process.version,
      ...readWorkflowContext(),
    },
    artifacts: [],
  };
}

if (outputPath === undefined) {
  console.error(JSON.stringify(report, null, 2));
  process.exitCode = 1;
} else {
  await writeJsonAtomic(outputPath, report);
  console.log(JSON.stringify(report, null, 2));
  if (!report.accepted) process.exitCode = 1;
}

function parseArguments(args) {
  if (
    args.length !== 4 ||
    args[0] !== "--input-directory" ||
    args[2] !== "--output"
  ) {
    throw new Error(
      "usage: node tools/verify-platform-matrix.mjs --input-directory <path> --output <path>",
    );
  }
  return {
    inputDirectory: path.resolve(args[1]),
    outputPath: path.resolve(args[3]),
  };
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24) {
    throw new Error(
      `P2-A12 matrix verification requires Node.js 24.x, received ${process.version}`,
    );
  }
}

function readWorkflowContext() {
  return {
    githubActions: process.env.GITHUB_ACTIONS ?? null,
    githubSha: process.env.GITHUB_SHA ?? null,
    githubRunId: process.env.GITHUB_RUN_ID ?? null,
    githubRunAttempt: process.env.GITHUB_RUN_ATTEMPT ?? null,
    githubWorkflowRef: process.env.GITHUB_WORKFLOW_REF ?? null,
    runnerOs: process.env.RUNNER_OS ?? null,
    runnerArch: process.env.RUNNER_ARCH ?? null,
    runnerEnvironment: process.env.MATERIAL_EAGLE_RUNNER_ENVIRONMENT ?? null,
  };
}

function readRepositoryState() {
  const revision = run("git", ["rev-parse", "HEAD"]);
  if (revision.status !== 0)
    throw new Error(`cannot read Git commit: ${diagnostic(revision)}`);
  for (const args of [
    ["diff", "--quiet"],
    ["diff", "--cached", "--quiet"],
  ]) {
    const status = run("git", args);
    if (status.status !== 0)
      throw new Error(
        "P2-A12 matrix verification requires clean tracked files",
      );
  }
  return revision.stdout.trim();
}

async function findEvidenceFiles(inputDirectory) {
  const root = await lstat(inputDirectory);
  if (!root.isDirectory()) throw new Error("P2-A12 input is not a directory");
  const matches = [];
  await walk(inputDirectory, matches);
  matches.sort();
  if (matches.length !== 3) {
    throw new Error(
      `found ${String(matches.length)} P2-A12 source files, expected exactly 3`,
    );
  }
  return matches;
}

async function walk(directory, matches) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isSymbolicLink())
      throw new Error("P2-A12 input directory must not contain symbolic links");
    if (entry.isDirectory()) {
      await walk(entryPath, matches);
    } else if (entry.isFile() && entry.name === "p2-a12-platform-paths.json") {
      matches.push(entryPath);
    }
  }
}

async function readBoundedFile(filePath) {
  const bytes = await readFile(filePath);
  if (bytes.length > 4 * 1024 * 1024)
    throw new Error(`P2-A12 source exceeds 4 MiB: ${path.basename(filePath)}`);
  return bytes;
}

function safeGitCommit() {
  const result = run("git", ["rev-parse", "HEAD"]);
  return result.status === 0 ? result.stdout.trim() : null;
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: repository,
    encoding: "utf8",
    maxBuffer: 1024 * 1024,
  });
  return {
    status: result.status ?? -1,
    error: result.error?.message ?? null,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
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
