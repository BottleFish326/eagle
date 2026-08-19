import { randomUUID } from "node:crypto";
import {
  link,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  unlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  collectP2HostedEvidence,
  inspectP2HostedRun,
  p2HostedArtifactPatterns,
} from "./p2-hosted-evidence.mjs";
import {
  collectP2HostedReadinessInputs,
  runHostedCommand,
} from "./p2-hosted-environment.mjs";
import { buildP2HostedReadiness } from "./p2-hosted-readiness.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const receiptPath = path.join(
  repository,
  "docs",
  "reports",
  "evidence",
  "p2-a12-hosted-run.json",
);
let downloadDirectory = null;
let temporaryDownloadRemoved = false;

try {
  assertNode24();
  const options = parseArguments(process.argv.slice(2));
  const readiness = buildP2HostedReadiness(
    await collectP2HostedReadinessInputs(repository),
  );
  if (!readiness.ready)
    throw new Error(
      `P2-A12 hosted environment is not ready: ${readiness.failures.join("; ")}`,
    );

  const run = readHostedRun({
    runId: options.runId,
    attempt: options.attempt,
    repositorySlug: readiness.github.repository,
  });
  const inspection = inspectP2HostedRun({
    run,
    requestedRunId: options.runId,
    requestedAttempt: options.attempt,
    expectedCommit: readiness.git.currentCommit,
    repositorySlug: readiness.github.repository,
  });
  if (!inspection.accepted)
    throw new Error(
      `P2-A12 hosted run rejected: ${inspection.failures.join("; ")}`,
    );

  const patterns = p2HostedArtifactPatterns(
    readiness.git.currentCommit,
    options.attempt,
  );
  downloadDirectory = await mkdtemp(
    path.join(tmpdir(), "material-eagle-p2-a12-"),
  );
  const result = await collectP2HostedEvidence({
    inspection,
    run,
    repositorySlug: readiness.github.repository,
    patterns,
    downloadDirectory,
    downloadArtifacts: async ({
      patterns: selected,
      downloadDirectory: target,
    }) => {
      const args = ["run", "download", String(options.runId)];
      for (const pattern of selected) args.push("-p", pattern);
      args.push("-D", target, "-R", readiness.github.repository);
      requireCommand("gh", args, 60_000);
    },
    archiveEvidence: async ({ downloadDirectory: source }) => {
      const output = requireCommand(
        process.execPath,
        [
          path.join(
            repository,
            "tools",
            "archive-platform-matrix-evidence.mjs",
          ),
          "--input-directory",
          source,
        ],
        60_000,
      );
      return JSON.parse(output);
    },
    removeDownloadDirectory: async (target) => {
      await rm(target, { recursive: true });
      temporaryDownloadRemoved = true;
    },
    publishEvidence: async (receipt) => {
      await writeExclusiveOrIdentical(receiptPath, receipt);
    },
  });
  console.log(JSON.stringify(result, null, 2));
} catch (error) {
  const preserved =
    downloadDirectory !== null && !temporaryDownloadRemoved
      ? `; temporary download preserved at ${downloadDirectory}`
      : "";
  console.error(
    `${error instanceof Error ? error.message : String(error)}${preserved}`,
  );
  process.exitCode = 1;
}

function parseArguments(args) {
  if (args.length === 4 && args[0] === "--run-id" && args[2] === "--attempt") {
    return {
      runId: positiveInteger(args[1], "run ID"),
      attempt: positiveInteger(args[3], "run attempt"),
    };
  }
  throw new Error(
    "usage: node tools/collect-p2-hosted-evidence.mjs --run-id <positive-integer> --attempt <positive-integer>",
  );
}

function positiveInteger(value, label) {
  if (!/^[1-9][0-9]*$/u.test(value))
    throw new Error(`${label} must be a positive integer`);
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed))
    throw new Error(`${label} exceeds the safe integer range`);
  return parsed;
}

function readHostedRun({ runId, attempt, repositorySlug }) {
  const output = requireCommand("gh", [
    "run",
    "view",
    String(runId),
    "--attempt",
    String(attempt),
    "--json",
    "attempt,conclusion,createdAt,databaseId,event,headBranch,headSha,jobs,startedAt,status,updatedAt,url,workflowName",
    "-R",
    repositorySlug,
  ]);
  try {
    return JSON.parse(output);
  } catch {
    throw new Error("GitHub CLI returned invalid hosted run JSON");
  }
}

async function writeExclusiveOrIdentical(destination, value) {
  const bytes = Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
  try {
    const stats = await lstat(destination);
    if (!stats.isFile())
      throw new Error("existing hosted run evidence is not a regular file");
    const existing = await readFile(destination);
    if (!existing.equals(bytes))
      throw new Error("existing hosted run evidence differs");
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

function requireCommand(command, args, timeout) {
  const result = runHostedCommand(repository, command, args, timeout);
  if (result.status !== 0)
    throw new Error(
      result.error ||
        result.stderr.trim() ||
        result.stdout.trim() ||
        `${command} exited with status ${String(result.status)}`,
    );
  return result.stdout.trim();
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `P2 hosted evidence collection requires Node.js 24.x, received ${process.version}`,
    );
}
