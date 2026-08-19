import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
  link,
  lstat,
  mkdir,
  readFile,
  unlink,
  writeFile,
} from "node:fs/promises";
import path from "node:path";

import { buildPhase2ExternalGatesReport } from "./phase-2-external-gates.mjs";
import { readPlatformMatrixBundle } from "./platform-matrix-bundle.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const defaults = {
  resourcePath: path.join(
    repository,
    "docs",
    "reports",
    "evidence",
    "p2-06-resource-soak.json",
  ),
  platformArchive: path.join(
    repository,
    "docs",
    "reports",
    "evidence",
    "p2-a12-platform-evidence",
  ),
  outputPath: path.join(
    repository,
    "docs",
    "reports",
    "evidence",
    "p2-external-gates.json",
  ),
};

try {
  const options = parseArguments(process.argv.slice(2));
  assertNode24();
  const head = readRepositoryHead();
  const resourceBytes = await readBoundedFile(
    options.resourcePath,
    32 * 1024 * 1024,
  );
  const resourceReport = JSON.parse(resourceBytes.toString("utf8"));
  const platformBundle = await readPlatformMatrixBundle(
    options.platformArchive,
  );
  const resourceCommit = resourceReport.gitCommit;
  const matrixCommit = platformBundle.matrixReport?.gitCommit;
  const commitOrderVerified =
    isCommit(resourceCommit) &&
    isCommit(matrixCommit) &&
    isAncestor(resourceCommit, matrixCommit) &&
    isAncestor(matrixCommit, head);
  const report = buildPhase2ExternalGatesReport({
    resourceBytes,
    resourceReport,
    platformBundle,
    commitOrderVerified,
  });
  console.log(JSON.stringify(report, null, 2));
  if (report.accepted) {
    await writeExclusiveOrIdentical(options.outputPath, report);
  } else {
    process.exitCode = 1;
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

function parseArguments(args) {
  if (args.length === 0) return { ...defaults };
  if (
    args.length === 6 &&
    args[0] === "--resource" &&
    args[2] === "--platform-archive" &&
    args[4] === "--output"
  ) {
    return {
      resourcePath: path.resolve(args[1]),
      platformArchive: path.resolve(args[3]),
      outputPath: path.resolve(args[5]),
    };
  }
  throw new Error(
    "usage: node tools/verify-phase-2-external-gates.mjs [--resource <json> --platform-archive <directory> --output <json>]",
  );
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `phase 2 external gates require Node.js 24.x, received ${process.version}`,
    );
}

function readRepositoryHead() {
  const revision = run("git", ["rev-parse", "HEAD"]);
  if (revision.status !== 0)
    throw new Error(`cannot read Git commit: ${diagnostic(revision)}`);
  for (const args of [
    ["diff", "--quiet"],
    ["diff", "--cached", "--quiet"],
  ]) {
    if (run("git", args).status !== 0)
      throw new Error("phase 2 external gates require clean tracked files");
  }
  return revision.stdout.trim();
}

function isAncestor(ancestor, descendant) {
  return (
    run("git", ["merge-base", "--is-ancestor", ancestor, descendant]).status ===
    0
  );
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}

async function readBoundedFile(filePath, limit) {
  const stats = await lstat(filePath);
  if (!stats.isFile()) throw new Error("P2-A11 evidence is not a regular file");
  if (stats.size > limit) throw new Error("P2-A11 evidence exceeds 32 MiB");
  return readFile(filePath);
}

async function writeExclusiveOrIdentical(destination, value) {
  const bytes = Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
  try {
    const existing = await readFile(destination);
    if (!existing.equals(bytes))
      throw new Error("existing phase 2 external gate evidence differs");
    return;
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }

  await mkdir(path.dirname(destination), { recursive: true });
  const temporary = path.join(
    path.dirname(destination),
    `.${path.basename(destination)}.${randomUUID()}.tmp`,
  );
  await writeFile(temporary, bytes, { flag: "wx" });
  try {
    await link(temporary, destination);
  } finally {
    await unlink(temporary).catch(() => {});
  }
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
