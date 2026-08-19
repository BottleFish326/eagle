import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  lstat,
  mkdir,
  readFile,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import path from "node:path";

import { inspectPlatformMatrixArchive } from "./platform-matrix-archive.mjs";
import {
  platformMatrixBundleEntries,
  readPlatformMatrixBundle,
  walkEvidenceTree,
} from "./platform-matrix-bundle.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const defaultOutputDirectory = path.join(
  repository,
  "docs",
  "reports",
  "evidence",
  "p2-a12-platform-evidence",
);

try {
  const options = parseArguments(process.argv.slice(2));
  assertNode24();
  const repositoryState = readRepositoryState();
  assertSeparateTrees(options.inputDirectory, options.outputDirectory);
  const bundle = await readPlatformMatrixBundle(options.inputDirectory);
  const inspection = inspectPlatformMatrixArchive(bundle);
  if (!inspection.accepted)
    throw new Error(
      `P2-A12 archive rejected: ${inspection.failures.join("; ")}`,
    );
  assertTestedCommitIsAncestor(
    inspection.replayedReport.gitCommit,
    repositoryState.gitCommit,
  );
  const entries = platformMatrixBundleEntries(bundle);
  const result = await archiveAtomically(options.outputDirectory, entries);
  console.log(
    JSON.stringify(
      {
        schema: 1,
        archived: true,
        alreadyPresent: result.alreadyPresent,
        outputDirectory: options.outputDirectory,
        gitCommit: inspection.replayedReport.gitCommit,
        githubRunAttempt: inspection.replayedReport.workflow.githubRunAttempt,
        runUrl: inspection.replayedReport.workflow.runUrl,
        files: entries.map((entry) => ({
          relativePath: entry.relativePath,
          sha256: sha256(entry.bytes),
          bytes: entry.bytes.length,
        })),
      },
      null,
      2,
    ),
  );
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

function parseArguments(args) {
  if (args.length === 2 && args[0] === "--input-directory") {
    return {
      inputDirectory: path.resolve(args[1]),
      outputDirectory: defaultOutputDirectory,
    };
  }
  if (
    args.length === 4 &&
    args[0] === "--input-directory" &&
    args[2] === "--output-directory"
  ) {
    return {
      inputDirectory: path.resolve(args[1]),
      outputDirectory: path.resolve(args[3]),
    };
  }
  throw new Error(
    "usage: node tools/archive-platform-matrix-evidence.mjs --input-directory <path> [--output-directory <path>]",
  );
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `P2-A12 evidence archival requires Node.js 24.x, received ${process.version}`,
    );
}

function readRepositoryState() {
  const revision = run("git", ["rev-parse", "HEAD"]);
  if (revision.status !== 0)
    throw new Error(`cannot read Git commit: ${diagnostic(revision)}`);
  for (const args of [
    ["diff", "--quiet"],
    ["diff", "--cached", "--quiet"],
  ]) {
    if (run("git", args).status !== 0)
      throw new Error("P2-A12 archival requires clean tracked files");
  }
  return { gitCommit: revision.stdout.trim() };
}

function assertTestedCommitIsAncestor(testedCommit, currentCommit) {
  if (!/^[0-9a-f]{40,64}$/u.test(testedCommit ?? ""))
    throw new Error("tested Git commit is invalid");
  const object = run("git", ["cat-file", "-e", `${testedCommit}^{commit}`]);
  if (object.status !== 0)
    throw new Error("tested Git commit is not present in the local repository");
  const ancestor = run("git", [
    "merge-base",
    "--is-ancestor",
    testedCommit,
    currentCommit,
  ]);
  if (ancestor.status !== 0)
    throw new Error("tested Git commit is not an ancestor of the current HEAD");
}

function assertSeparateTrees(inputDirectory, outputDirectory) {
  if (
    inputDirectory === outputDirectory ||
    isInside(inputDirectory, outputDirectory) ||
    isInside(outputDirectory, inputDirectory)
  )
    throw new Error("input and output directories must be separate trees");
}

function isInside(candidate, parent) {
  const relative = path.relative(parent, candidate);
  return (
    relative !== "" &&
    !relative.startsWith(`..${path.sep}`) &&
    relative !== ".."
  );
}

async function archiveAtomically(outputDirectory, entries) {
  try {
    const existing = await lstat(outputDirectory);
    if (!existing.isDirectory())
      throw new Error("P2-A12 archive destination is not a directory");
    await assertExistingArchive(outputDirectory, entries);
    return { alreadyPresent: true };
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }

  await mkdir(path.dirname(outputDirectory), { recursive: true });
  const stagingDirectory = path.join(
    path.dirname(outputDirectory),
    `.${path.basename(outputDirectory)}.${randomUUID()}.tmp`,
  );
  await mkdir(stagingDirectory);
  try {
    for (const entry of entries) {
      const destination = path.join(stagingDirectory, entry.relativePath);
      await mkdir(path.dirname(destination), { recursive: true });
      await writeFile(destination, entry.bytes, { flag: "wx" });
    }
    await rename(stagingDirectory, outputDirectory);
  } catch (error) {
    await rm(stagingDirectory, { recursive: true, force: true });
    throw error;
  }
  return { alreadyPresent: false };
}

async function assertExistingArchive(outputDirectory, entries) {
  const files = [];
  await walkEvidenceTree(outputDirectory, files);
  const actual = files
    .map((file) => path.relative(outputDirectory, file))
    .toSorted();
  const expected = entries.map((entry) => entry.relativePath).toSorted();
  if (JSON.stringify(actual) !== JSON.stringify(expected))
    throw new Error("existing P2-A12 archive has a different file set");
  for (const entry of entries) {
    const bytes = await readFile(
      path.join(outputDirectory, entry.relativePath),
    );
    if (!bytes.equals(entry.bytes))
      throw new Error(
        `existing P2-A12 archive differs at ${entry.relativePath}`,
      );
  }
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
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
