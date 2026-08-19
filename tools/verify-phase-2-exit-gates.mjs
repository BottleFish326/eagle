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

import {
  buildPhase2ExitGatesReport,
  inspectPhase2ExitGatesReceipt,
} from "./phase-2-exit-gates.mjs";
import { buildPhase2ExternalGatesReport } from "./phase-2-external-gates.mjs";
import { readPlatformMatrixBundle } from "./platform-matrix-bundle.mjs";
import {
  buildSoakBaselineAudit,
  FORMAL_SOAK_BASELINE_COMMIT,
  FORMAL_SOAK_LOADED_PATHS,
  FORMAL_SOAK_PRODUCT_SCOPES,
} from "./soak-baseline-audit.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const evidenceDirectory = path.join(repository, "docs", "reports", "evidence");
const paths = {
  resource: path.join(evidenceDirectory, "p2-06-resource-soak.json"),
  platformArchive: path.join(evidenceDirectory, "p2-a12-platform-evidence"),
  hostedRun: path.join(evidenceDirectory, "p2-a12-hosted-run.json"),
  external: path.join(evidenceDirectory, "p2-external-gates.json"),
  localFaults: path.join(evidenceDirectory, "p2-local-fault-gates.json"),
  output: path.join(evidenceDirectory, "p2-phase-2-exit.json"),
};
const repositoryPaths = {
  external: "docs/reports/evidence/p2-external-gates.json",
  localFaults: "docs/reports/evidence/p2-local-fault-gates.json",
};

try {
  assertNode24();
  assertNoArguments(process.argv.slice(2));
  await assertOutputAbsent(paths.output);
  const candidateCommit = readCleanRepositoryHead();

  const resourceBytes = await readBoundedFile(
    paths.resource,
    32 * 1024 * 1024,
    "P2-A11 evidence",
  );
  const resourceReport = parseJson(resourceBytes, "P2-A11 evidence");
  const platformBundle = await readPlatformMatrixBundle(paths.platformArchive);
  const hostedRunBytes = await readBoundedFile(
    paths.hostedRun,
    1024 * 1024,
    "P2-A12 hosted run evidence",
  );
  const hostedRunReceipt = parseJson(
    hostedRunBytes,
    "P2-A12 hosted run evidence",
  );
  const externalBytes = await readBoundedFile(
    paths.external,
    1024 * 1024,
    "phase 2 external gate evidence",
  );
  const externalReport = parseJson(
    externalBytes,
    "phase 2 external gate evidence",
  );
  const localFaultBytes = await readBoundedFile(
    paths.localFaults,
    1024 * 1024,
    "phase 2 local fault evidence",
  );
  const localFaultReceipt = parseJson(
    localFaultBytes,
    "phase 2 local fault evidence",
  );

  const resourceCommit = resourceReport?.gitCommit;
  const matrixCommit = platformBundle.matrixReport?.gitCommit;
  const localCommit = localFaultReceipt?.gitCommit;
  const externalCommitOrder =
    isCommit(resourceCommit) &&
    isCommit(matrixCommit) &&
    isAncestor(resourceCommit, matrixCommit) &&
    isAncestor(matrixCommit, candidateCommit);
  const externalReplay = buildPhase2ExternalGatesReport({
    resourceBytes,
    resourceReport,
    platformBundle,
    hostedRunBytes,
    hostedRunReceipt,
    commitOrderVerified: externalCommitOrder,
  });
  const commitOrderVerified =
    externalCommitOrder &&
    isCommit(localCommit) &&
    isAncestor(matrixCommit, localCommit) &&
    isAncestor(localCommit, candidateCommit);
  const soakBaselineAudit = buildCurrentSoakBaselineAudit(candidateCommit);
  const localCandidateDriftPaths = isCommit(localCommit)
    ? changedPaths(localCommit, []).filter(
        (entry) => !isAllowedAfterLocal(entry),
      )
    : [];

  const report = buildPhase2ExitGatesReport({
    externalBytes,
    externalReport,
    externalReplay,
    localFaultBytes,
    localFaultReceipt,
    candidateCommit,
    workingTreeClean: true,
    commitOrderVerified,
    externalEvidenceInLocalCandidate:
      isCommit(localCommit) &&
      gitBlobEquals(localCommit, repositoryPaths.external, externalBytes),
    localEvidenceCommitted: gitBlobEquals(
      candidateCommit,
      repositoryPaths.localFaults,
      localFaultBytes,
    ),
    soakBaselineAudit,
    localCandidateDriftPaths,
  });
  if (!report.accepted) {
    console.log(JSON.stringify(report, null, 2));
    process.exitCode = 1;
  } else {
    const receiptInspection = inspectPhase2ExitGatesReceipt(report);
    if (!receiptInspection.accepted)
      throw new Error(
        `phase 2 exit receipt self-check failed: ${receiptInspection.failures.join("; ")}`,
      );
    await writeExclusive(paths.output, report);
    console.log(JSON.stringify(report, null, 2));
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `phase 2 exit gates require Node.js 24.x, received ${process.version}`,
    );
}

function assertNoArguments(args) {
  if (args.length !== 0)
    throw new Error("usage: node tools/verify-phase-2-exit-gates.mjs");
}

async function assertOutputAbsent(destination) {
  try {
    await lstat(destination);
    throw new Error(
      "phase 2 exit evidence already exists and will not be overwritten",
    );
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
}

function readCleanRepositoryHead() {
  const head = git(["rev-parse", "HEAD"]).trim();
  const status = git(["status", "--porcelain=v1", "--untracked-files=all"]);
  if (status !== "")
    throw new Error(
      "phase 2 exit gates require a completely clean working tree",
    );
  return head;
}

function buildCurrentSoakBaselineAudit(candidateCommit) {
  return buildSoakBaselineAudit({
    baselineCommit: FORMAL_SOAK_BASELINE_COMMIT,
    currentCommit: candidateCommit,
    descendantOfBaseline: isAncestor(
      FORMAL_SOAK_BASELINE_COMMIT,
      candidateCommit,
    ),
    loadedChangedPaths: changedPaths(
      FORMAL_SOAK_BASELINE_COMMIT,
      FORMAL_SOAK_LOADED_PATHS,
    ),
    productChangedPaths: changedPaths(
      FORMAL_SOAK_BASELINE_COMMIT,
      FORMAL_SOAK_PRODUCT_SCOPES,
    ),
  });
}

function changedPaths(baseline, scopes) {
  const args = ["diff", "--name-only", "--no-renames", "-z", baseline];
  if (scopes.length > 0) args.push("--", ...scopes);
  return git(args).split("\0").filter(Boolean).toSorted();
}

function isAllowedAfterLocal(filePath) {
  return (
    filePath === "README.md" ||
    filePath === "docs" ||
    filePath.startsWith("docs/")
  );
}

function isAncestor(ancestor, descendant) {
  return gitStatus(["merge-base", "--is-ancestor", ancestor, descendant]);
}

function gitBlobEquals(commit, filePath, expected) {
  const result = runGit(["show", `${commit}:${filePath}`], null);
  return (
    result.status === 0 &&
    Buffer.isBuffer(result.stdout) &&
    result.stdout.equals(expected)
  );
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}

async function readBoundedFile(filePath, limit, label) {
  const stats = await lstat(filePath);
  if (!stats.isFile()) throw new Error(`${label} is not a regular file`);
  if (stats.size > limit) throw new Error(`${label} exceeds its size limit`);
  return readFile(filePath);
}

function parseJson(bytes, label) {
  try {
    return JSON.parse(bytes.toString("utf8"));
  } catch (error) {
    throw new Error(
      `${label} is not valid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
}

async function writeExclusive(destination, value) {
  await mkdir(path.dirname(destination), { recursive: true });
  const bytes = Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
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

function git(args) {
  const result = runGit(args, "utf8");
  if (result.status !== 0)
    throw new Error(`git ${args[0]} failed: ${diagnostic(result)}`);
  return result.stdout;
}

function gitStatus(args) {
  return runGit(args, "utf8").status === 0;
}

function runGit(args, encoding) {
  const result = spawnSync("git", args, {
    cwd: repository,
    encoding,
    maxBuffer: 32 * 1024 * 1024,
  });
  return {
    status: result.status ?? -1,
    error: result.error?.message ?? null,
    stdout: result.stdout ?? (encoding === null ? Buffer.alloc(0) : ""),
    stderr: result.stderr ?? (encoding === null ? Buffer.alloc(0) : ""),
  };
}

function diagnostic(result) {
  const stderr = Buffer.isBuffer(result.stderr)
    ? result.stderr.toString("utf8").trim()
    : result.stderr.trim();
  const stdout = Buffer.isBuffer(result.stdout)
    ? result.stdout.toString("utf8").trim()
    : result.stdout.trim();
  return (
    result.error ||
    stderr ||
    stdout ||
    `process exited with status ${String(result.status)}`
  );
}
