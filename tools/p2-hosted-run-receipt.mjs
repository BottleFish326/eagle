import { createHash } from "node:crypto";

import {
  buildP2HostedRunReceipt,
  inspectP2HostedRun,
} from "./p2-hosted-evidence.mjs";
import { platformMatrixBundleEntries } from "./platform-matrix-bundle.mjs";

export function inspectP2HostedRunReceipt(receipt, platformBundle) {
  const failures = [];
  if (!isRecord(receipt)) {
    return {
      accepted: false,
      failures: ["hosted run receipt is not an object"],
      replayedReceipt: null,
    };
  }
  if (receipt.schema !== 1) failures.push("hosted run receipt schema is not 1");
  if (receipt.accepted !== true)
    failures.push("hosted run receipt is not accepted");
  if (!Array.isArray(receipt.failures) || receipt.failures.length !== 0)
    failures.push("hosted run receipt failures are not empty");
  if (receipt.temporaryDownloadRemoved !== true)
    failures.push("hosted run receipt did not remove its temporary download");

  const matrix = platformBundle?.matrixReport;
  const repositorySlug = matrix?.verificationEnvironment?.githubRepository;
  if (receipt.repository !== repositorySlug)
    failures.push("hosted run receipt repository does not match the matrix");
  if (String(receipt.run?.databaseId) !== String(matrix?.workflow?.githubRunId))
    failures.push("hosted run receipt ID does not match the matrix");

  let replayedReceipt = null;
  try {
    const run = { ...receipt.run, jobs: receipt.jobs };
    const inspection = inspectP2HostedRun({
      run,
      requestedRunId: matrix?.workflow?.githubRunId,
      requestedAttempt: Number(matrix?.workflow?.githubRunAttempt),
      expectedCommit: matrix?.gitCommit,
      repositorySlug,
    });
    const archive = actualArchiveReport(platformBundle);
    replayedReceipt = {
      ...buildP2HostedRunReceipt({
        inspection,
        run,
        repositorySlug,
        archive,
      }),
      temporaryDownloadRemoved: true,
    };
    for (const failure of replayedReceipt.failures)
      failures.push(`hosted run receipt replay: ${failure}`);
    if (!sameJsonValue(replayedReceipt, receipt))
      failures.push("hosted run receipt does not equal its offline replay");
  } catch (error) {
    failures.push(
      `hosted run receipt replay failed: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  return {
    accepted: failures.length === 0,
    failures,
    replayedReceipt,
  };
}

function actualArchiveReport(platformBundle) {
  const matrix = platformBundle.matrixReport;
  return {
    archived: true,
    gitCommit: matrix.gitCommit,
    githubRunAttempt: matrix.workflow.githubRunAttempt,
    runUrl: matrix.workflow.runUrl,
    files: platformMatrixBundleEntries(platformBundle).map((entry) => ({
      relativePath: entry.relativePath,
      sha256: createHash("sha256").update(entry.bytes).digest("hex"),
      bytes: entry.bytes.length,
    })),
  };
}

function sameJsonValue(left, right) {
  return canonicalJson(left) === canonicalJson(right);
}

function canonicalJson(value) {
  if (Array.isArray(value))
    return `[${value.map((item) => canonicalJson(item)).join(",")}]`;
  if (isRecord(value)) {
    return `{${Object.keys(value)
      .toSorted()
      .map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
