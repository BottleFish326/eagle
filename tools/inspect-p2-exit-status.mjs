import { spawnSync } from "node:child_process";
import { isDeepStrictEqual } from "node:util";
import { lstat, readFile } from "node:fs/promises";
import path from "node:path";

import { collectP2HostedReadinessInputs } from "./p2-hosted-environment.mjs";
import { buildP2HostedReadiness } from "./p2-hosted-readiness.mjs";
import { inspectP2HostedRunReceipt } from "./p2-hosted-run-receipt.mjs";
import { inspectP2LocalFaultGatesReceipt } from "./p2-local-fault-gates.mjs";
import {
  inspectDefectRegister,
  inspectP2DataSafetyAuditReceipt,
  P2_DATA_SAFETY_REPORTS,
} from "./p2-data-safety-audit.mjs";
import { buildP2ExitStatus } from "./p2-exit-status.mjs";
import { inspectPhase2ExitGatesReceipt } from "./phase-2-exit-gates.mjs";
import { buildPhase2ExternalGatesReport } from "./phase-2-external-gates.mjs";
import { inspectPlatformMatrixArchive } from "./platform-matrix-archive.mjs";
import { readPlatformMatrixBundle } from "./platform-matrix-bundle.mjs";
import { inspectResourceStabilityCheckpoint } from "./resource-stability-checkpoint-inspection.mjs";
import {
  FORMAL_RESOURCE_STABILITY_OPTIONS,
  inspectResourceStabilityReport,
} from "./resource-stability-report.mjs";
import {
  buildSoakBaselineAudit,
  FORMAL_SOAK_LOADED_PATHS,
  FORMAL_SOAK_PRODUCT_SCOPES,
} from "./soak-baseline-audit.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const evidenceDirectory = path.join(repository, "docs", "reports", "evidence");
const files = {
  resource: path.join(evidenceDirectory, "p2-06-resource-soak.json"),
  resourcePartial: path.join(
    evidenceDirectory,
    "p2-06-resource-soak.json.partial",
  ),
  platformArchive: path.join(evidenceDirectory, "p2-a12-platform-evidence"),
  hostedRun: path.join(evidenceDirectory, "p2-a12-hosted-run.json"),
  external: path.join(evidenceDirectory, "p2-external-gates.json"),
  localFaults: path.join(evidenceDirectory, "p2-local-fault-gates.json"),
  dataSafety: path.join(evidenceDirectory, "p2-data-safety-audit.json"),
  defectRegister: path.join(repository, "docs", "defects.json"),
  finalExit: path.join(evidenceDirectory, "p2-phase-2-exit.json"),
};
const repositoryFiles = {
  localFaults: "docs/reports/evidence/p2-local-fault-gates.json",
  dataSafety: "docs/reports/evidence/p2-data-safety-audit.json",
  defectRegister: "docs/defects.json",
  finalExit: "docs/reports/evidence/p2-phase-2-exit.json",
};
const invalidStages = new Set([
  "evidence-conflict",
  "final-exit-invalid",
  "soak-failed",
  "external-gates-invalid",
  "local-faults-invalid",
  "data-safety-invalid",
]);

try {
  assertNode24();
  assertNoArguments(process.argv.slice(2));
  const git = collectGitState();
  const hostedReadiness = buildP2HostedReadiness(
    await collectP2HostedReadinessInputs(repository),
  );
  const resource = await collectResourceState(git.currentCommit);
  const platform = await collectPlatformState();
  const hostedRun = await collectHostedRunState(platform);
  const externalGates = await collectExternalState({
    git,
    resource,
    platform,
    hostedRun,
  });
  const localFaults = await collectLocalFaultState(git.currentCommit);
  const dataSafetyReadiness = await collectDataSafetyReadiness({
    gitState: git,
    externalGates,
    localFaults,
  });
  const dataSafety = await collectDataSafetyState(git.currentCommit);
  const finalExit = await collectFinalExitState(git.currentCommit);
  const report = buildP2ExitStatus({
    git,
    soak: resource.gate,
    hostedReadiness,
    externalGates,
    localFaults,
    dataSafetyReadiness,
    dataSafety,
    finalExit,
  });
  console.log(JSON.stringify(report, null, 2));
  if (invalidStages.has(report.stage)) process.exitCode = 1;
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `P2 exit status requires Node.js 24.x, received ${process.version}`,
    );
}

function assertNoArguments(args) {
  if (args.length !== 0)
    throw new Error("usage: node tools/inspect-p2-exit-status.mjs");
}

function collectGitState() {
  const currentCommit = git(["rev-parse", "HEAD"]).trim();
  return {
    currentCommit,
    cleanTracked:
      gitStatus(["diff", "--quiet"]) &&
      gitStatus(["diff", "--cached", "--quiet"]),
    cleanAll: git(["status", "--porcelain=v1", "--untracked-files=all"]) === "",
  };
}

async function collectResourceState(currentCommit) {
  const finalEvidence = await readOptionalJson(
    files.resource,
    32 * 1024 * 1024,
  );
  const partialEvidence = await readOptionalJson(
    files.resourcePartial,
    32 * 1024 * 1024,
  );
  if (finalEvidence.exists && partialEvidence.exists)
    return {
      gate: {
        state: "failed",
        failures: ["final and partial P2-A11 evidence both exist"],
        summary: null,
      },
      finalEvidence,
    };
  if (finalEvidence.exists) {
    if (finalEvidence.error !== null)
      return failedResource(finalEvidence.error, finalEvidence);
    const inspection = inspectResourceStabilityReport(finalEvidence.value, {
      expectedOptions: FORMAL_RESOURCE_STABILITY_OPTIONS,
    });
    const baselineAudit = collectSoakBaselineAudit(
      finalEvidence.value?.gitCommit,
      currentCommit,
    );
    const failures = [...inspection.failures, ...baselineAudit.failures];
    return {
      gate: {
        state:
          inspection.accepted && baselineAudit.accepted ? "passed" : "failed",
        failures,
        summary: inspection.replayedReport?.summary ?? null,
      },
      finalEvidence,
      replayedReport: inspection.replayedReport,
      baselineAudit,
    };
  }
  if (partialEvidence.exists) {
    if (partialEvidence.error !== null)
      return failedResource(partialEvidence.error, finalEvidence);
    const inspection = inspectResourceStabilityCheckpoint(
      partialEvidence.value,
      { expectedOptions: FORMAL_RESOURCE_STABILITY_OPTIONS },
    );
    const baselineAudit = collectSoakBaselineAudit(
      partialEvidence.value?.gitCommit,
      currentCommit,
    );
    const failures = [...inspection.failures, ...baselineAudit.failures];
    return {
      gate: {
        state:
          inspection.healthy && baselineAudit.accepted ? "running" : "failed",
        failures,
        summary: inspection.summary,
      },
      finalEvidence,
      baselineAudit,
    };
  }
  return {
    gate: { state: "missing", failures: [], summary: null },
    finalEvidence,
  };
}

function collectSoakBaselineAudit(baselineCommit, currentCommit) {
  const baselineExists =
    isCommit(baselineCommit) &&
    gitStatus(["cat-file", "-e", `${baselineCommit}^{commit}`]);
  return buildSoakBaselineAudit({
    baselineCommit,
    currentCommit,
    descendantOfBaseline:
      baselineExists && isAncestor(baselineCommit, currentCommit),
    loadedChangedPaths: baselineExists
      ? changedPaths(baselineCommit, FORMAL_SOAK_LOADED_PATHS)
      : [],
    productChangedPaths: baselineExists
      ? changedPaths(baselineCommit, FORMAL_SOAK_PRODUCT_SCOPES)
      : [],
  });
}

function changedPaths(baselineCommit, scopes) {
  const tracked = gitNullDelimited([
    "diff",
    "--name-only",
    "--no-renames",
    "-z",
    baselineCommit,
    "--",
    ...scopes,
  ]);
  const untracked = gitNullDelimited([
    "ls-files",
    "--others",
    "--exclude-standard",
    "-z",
    "--",
    ...scopes,
  ]);
  return [...new Set([...tracked, ...untracked])].toSorted();
}

function gitNullDelimited(args) {
  return git(args)
    .split("\0")
    .filter((entry) => entry.length > 0);
}

function failedResource(message, finalEvidence) {
  return {
    gate: { state: "failed", failures: [message], summary: null },
    finalEvidence,
  };
}

async function collectPlatformState() {
  const directory = await inspectOptionalDirectory(files.platformArchive);
  if (!directory.exists)
    return { exists: false, accepted: false, failures: [], bundle: null };
  if (directory.error !== null)
    return {
      exists: true,
      accepted: false,
      failures: [directory.error],
      bundle: null,
    };
  try {
    const bundle = await readPlatformMatrixBundle(files.platformArchive);
    const inspection = inspectPlatformMatrixArchive(bundle);
    return {
      exists: true,
      accepted: inspection.accepted,
      failures: inspection.failures,
      bundle,
    };
  } catch (error) {
    return {
      exists: true,
      accepted: false,
      failures: [error instanceof Error ? error.message : String(error)],
      bundle: null,
    };
  }
}

async function collectHostedRunState(platform) {
  const evidence = await readOptionalJson(files.hostedRun, 1024 * 1024);
  if (!evidence.exists)
    return { exists: false, accepted: false, failures: [], evidence };
  if (evidence.error !== null)
    return {
      exists: true,
      accepted: false,
      failures: [evidence.error],
      evidence,
    };
  if (platform.bundle === null)
    return {
      exists: true,
      accepted: false,
      failures: ["hosted run receipt has no readable platform archive"],
      evidence,
    };
  const inspection = inspectP2HostedRunReceipt(evidence.value, platform.bundle);
  return {
    exists: true,
    accepted: inspection.accepted,
    failures: inspection.failures,
    evidence,
  };
}

async function collectExternalState({ git, resource, platform, hostedRun }) {
  const evidence = await readOptionalJson(files.external, 1024 * 1024);
  const dependenciesAccepted =
    resource.gate.state === "passed" &&
    resource.finalEvidence?.bytes !== null &&
    platform.accepted &&
    platform.bundle !== null &&
    hostedRun.accepted &&
    hostedRun.evidence?.bytes !== null;

  if (!evidence.exists) {
    const existingFailures = [
      ...(platform.exists && !platform.accepted ? platform.failures : []),
      ...(hostedRun.exists && !hostedRun.accepted ? hostedRun.failures : []),
    ];
    if (existingFailures.length > 0)
      return { state: "invalid", failures: existingFailures, summary: null };
    return {
      state: dependenciesAccepted ? "ready" : "missing",
      failures: [],
      summary: null,
    };
  }
  if (evidence.error !== null)
    return { state: "invalid", failures: [evidence.error], summary: null };
  if (!dependenciesAccepted)
    return {
      state: "invalid",
      failures: [
        "external gate receipt is missing one or more accepted inputs",
      ],
      summary: null,
    };

  const resourceReport = resource.finalEvidence.value;
  const matrixCommit = platform.bundle.matrixReport?.gitCommit;
  const resourceCommit = resourceReport?.gitCommit;
  const commitOrderVerified =
    isCommit(resourceCommit) &&
    isCommit(matrixCommit) &&
    isAncestor(resourceCommit, matrixCommit) &&
    isAncestor(matrixCommit, git.currentCommit);
  const replay = buildPhase2ExternalGatesReport({
    resourceBytes: resource.finalEvidence.bytes,
    resourceReport,
    platformBundle: platform.bundle,
    hostedRunBytes: hostedRun.evidence.bytes,
    hostedRunReceipt: hostedRun.evidence.value,
    commitOrderVerified,
  });
  const accepted = replay.accepted && isDeepStrictEqual(evidence.value, replay);
  return {
    state: accepted ? "accepted" : "invalid",
    failures: accepted
      ? []
      : [
          ...replay.failures,
          ...(isDeepStrictEqual(evidence.value, replay)
            ? []
            : ["stored external gate receipt does not equal its replay"]),
        ],
    summary: accepted
      ? {
          evidenceAt: replay.evidenceAt,
          p2A11Commit: replay.p2A11.gitCommit,
          p2A12Commit: replay.p2A12.gitCommit,
          runUrl: replay.p2A12.runUrl,
        }
      : null,
  };
}

async function collectLocalFaultState(currentCommit) {
  const evidence = await readOptionalJson(files.localFaults, 1024 * 1024);
  if (!evidence.exists)
    return { state: "missing", failures: [], committed: false, summary: null };
  if (evidence.error !== null)
    return {
      state: "invalid",
      failures: [evidence.error],
      committed: false,
      summary: null,
    };
  const inspection = inspectP2LocalFaultGatesReceipt(evidence.value);
  const failures = [...inspection.failures];
  if (
    inspection.accepted &&
    !isAncestor(evidence.value.gitCommit, currentCommit)
  )
    failures.push("local fault receipt commit is not an ancestor of HEAD");
  const accepted = failures.length === 0;
  return {
    state: accepted ? "accepted" : "invalid",
    failures,
    committed: gitBlobEquals(
      currentCommit,
      repositoryFiles.localFaults,
      evidence.bytes,
    ),
    summary: accepted
      ? {
          gitCommit: evidence.value.gitCommit,
          executedAt: evidence.value.executedAt,
        }
      : null,
  };
}

async function collectDataSafetyReadiness({
  gitState,
  externalGates,
  localFaults,
}) {
  const evidence = await readOptionalJson(files.defectRegister, 1024 * 1024);
  const reportsCommitted = P2_DATA_SAFETY_REPORTS.every((fileName) =>
    gitPathExists(gitState.currentCommit, fileName),
  );
  if (!evidence.exists)
    return {
      ready: false,
      registerState: "missing",
      registerCommitted: false,
      reportsCommitted,
      candidateClean: gitState.cleanAll,
      failures: ["defect register is missing"],
      summary: null,
    };
  if (evidence.error !== null)
    return {
      ready: false,
      registerState: "invalid",
      registerCommitted: false,
      reportsCommitted,
      candidateClean: gitState.cleanAll,
      failures: [evidence.error],
      summary: null,
    };

  const minimumReviewedAt = laterIsoInstant(
    externalGates.summary?.evidenceAt,
    localFaults.summary?.executedAt,
  );
  const inspection = inspectDefectRegister(evidence.value, {
    requireReviewed: true,
    minimumReviewedAt,
    maximumReviewedAt: commitTime(gitState.currentCommit),
  });
  const registerCommitted = gitBlobEquals(
    gitState.currentCommit,
    repositoryFiles.defectRegister,
    evidence.bytes,
  );
  const failures = [...inspection.failures];
  if (!registerCommitted) failures.push("defect register is not committed");
  if (!reportsCommitted)
    failures.push("one or more data safety reports are not committed");
  if (!gitState.cleanAll)
    failures.push("data safety candidate working tree is not clean");
  const upstreamReady =
    externalGates.state === "accepted" &&
    localFaults.state === "accepted" &&
    localFaults.committed === true;
  return {
    ready:
      upstreamReady &&
      inspection.accepted &&
      registerCommitted &&
      reportsCommitted &&
      gitState.cleanAll,
    registerState: ["draft", "reviewed"].includes(evidence.value?.status)
      ? evidence.value.status
      : "invalid",
    registerCommitted,
    reportsCommitted,
    candidateClean: gitState.cleanAll,
    failures,
    summary: {
      reviewedAt: evidence.value?.reviewedAt ?? null,
      counts: inspection.counts,
    },
  };
}

async function collectDataSafetyState(currentCommit) {
  const evidence = await readOptionalJson(files.dataSafety, 1024 * 1024);
  if (!evidence.exists)
    return { state: "missing", failures: [], committed: false, summary: null };
  if (evidence.error !== null)
    return {
      state: "invalid",
      failures: [evidence.error],
      committed: false,
      summary: null,
    };
  const inspection = inspectP2DataSafetyAuditReceipt(evidence.value);
  const failures = [...inspection.failures];
  if (
    inspection.accepted &&
    !isAncestor(evidence.value.candidateCommit, currentCommit)
  )
    failures.push("data safety candidate commit is not an ancestor of HEAD");
  const accepted = failures.length === 0;
  return {
    state: accepted ? "accepted" : "invalid",
    failures,
    committed: gitBlobEquals(
      currentCommit,
      repositoryFiles.dataSafety,
      evidence.bytes,
    ),
    summary: accepted
      ? {
          candidateCommit: evidence.value.candidateCommit,
          evidenceAt: evidence.value.evidenceAt,
          openP0: evidence.value.defectRegister.counts.open.P0,
          openP1: evidence.value.defectRegister.counts.open.P1,
        }
      : null,
  };
}

async function collectFinalExitState(currentCommit) {
  const evidence = await readOptionalJson(files.finalExit, 1024 * 1024);
  if (!evidence.exists)
    return { state: "missing", failures: [], committed: false, summary: null };
  if (evidence.error !== null)
    return {
      state: "invalid",
      failures: [evidence.error],
      committed: false,
      summary: null,
    };
  const inspection = inspectPhase2ExitGatesReceipt(evidence.value);
  const failures = [...inspection.failures];
  if (
    inspection.accepted &&
    !isAncestor(evidence.value.candidateCommit, currentCommit)
  )
    failures.push("final exit candidate commit is not an ancestor of HEAD");
  const accepted = failures.length === 0;
  return {
    state: accepted ? "accepted" : "invalid",
    failures,
    committed: gitBlobEquals(
      currentCommit,
      repositoryFiles.finalExit,
      evidence.bytes,
    ),
    summary: accepted
      ? {
          candidateCommit: evidence.value.candidateCommit,
          evidenceAt: evidence.value.evidenceAt,
        }
      : null,
  };
}

async function readOptionalJson(filePath, limit) {
  try {
    const stats = await lstat(filePath);
    if (!stats.isFile())
      return optionalError("evidence path is not a regular file");
    if (stats.size > limit)
      return optionalError("evidence file exceeds its size limit");
    const bytes = await readFile(filePath);
    try {
      return {
        exists: true,
        bytes,
        value: JSON.parse(bytes.toString("utf8")),
        error: null,
      };
    } catch (error) {
      return optionalError(
        `evidence is not valid JSON: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  } catch (error) {
    if (error?.code === "ENOENT")
      return { exists: false, bytes: null, value: null, error: null };
    return optionalError(
      error instanceof Error ? error.message : String(error),
    );
  }
}

async function inspectOptionalDirectory(directory) {
  try {
    const stats = await lstat(directory);
    return stats.isDirectory()
      ? { exists: true, error: null }
      : { exists: true, error: "platform archive is not a directory" };
  } catch (error) {
    if (error?.code === "ENOENT") return { exists: false, error: null };
    return {
      exists: true,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

function optionalError(error) {
  return { exists: true, bytes: null, value: null, error };
}

function gitBlobEquals(commit, filePath, expected) {
  const result = runGit(["show", `${commit}:${filePath}`], null);
  return (
    result.status === 0 &&
    Buffer.isBuffer(result.stdout) &&
    result.stdout.equals(expected)
  );
}

function gitPathExists(commit, filePath) {
  return gitStatus(["cat-file", "-e", `${commit}:${filePath}`]);
}

function commitTime(commit) {
  const value = git(["show", "-s", "--format=%cI", commit]).trim();
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp))
    throw new Error(`cannot read commit time for ${commit}`);
  return new Date(timestamp).toISOString();
}

function laterIsoInstant(...values) {
  const candidates = values.filter(isIsoInstant);
  return candidates.length === 0
    ? null
    : candidates.toSorted((a, b) => Date.parse(a) - Date.parse(b)).at(-1);
}

function isIsoInstant(value) {
  if (typeof value !== "string") return false;
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z$/u.test(value))
    return false;
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return false;
  const canonical = new Date(timestamp).toISOString();
  return canonical === value || canonical === value.replace(/Z$/u, ".000Z");
}

function isAncestor(ancestor, descendant) {
  return (
    runGit(["merge-base", "--is-ancestor", ancestor, descendant], "utf8")
      .status === 0
  );
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
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
