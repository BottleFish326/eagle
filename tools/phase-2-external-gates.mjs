import { createHash } from "node:crypto";

import { inspectPlatformMatrixArchive } from "./platform-matrix-archive.mjs";
import { inspectP2HostedRunReceipt } from "./p2-hosted-run-receipt.mjs";
import {
  FORMAL_RESOURCE_STABILITY_OPTIONS,
  inspectResourceStabilityReport,
} from "./resource-stability-report.mjs";

export function buildPhase2ExternalGatesReport({
  resourceBytes,
  resourceReport,
  platformBundle,
  hostedRunBytes,
  hostedRunReceipt,
  commitOrderVerified,
  expectedResourceOptions = FORMAL_RESOURCE_STABILITY_OPTIONS,
}) {
  const failures = [];
  const resourceInspection = inspectResourceStabilityReport(resourceReport, {
    expectedOptions: expectedResourceOptions,
  });
  for (const failure of resourceInspection.failures)
    failures.push(`P2-A11: ${failure}`);

  const platformInspection = inspectPlatformMatrixArchive(platformBundle);
  for (const failure of platformInspection.failures)
    failures.push(`P2-A12: ${failure}`);
  const hostedRunInspection = inspectP2HostedRunReceipt(
    hostedRunReceipt,
    platformBundle,
  );
  for (const failure of hostedRunInspection.failures)
    failures.push(`P2-A12 hosted run: ${failure}`);

  if (commitOrderVerified !== true)
    failures.push(
      "P2 commits are not ordered soak <= hosted matrix <= current HEAD",
    );

  const resource = resourceInspection.replayedReport;
  const matrix = platformInspection.replayedReport;
  const hostedRun = hostedRunInspection.replayedReceipt;
  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    evidenceAt: laterIsoInstant(
      resource?.completedAt,
      matrix?.verifiedAt,
      hostedRun?.verifiedAt,
    ),
    commitOrderVerified: commitOrderVerified === true,
    p2A11: {
      accepted: resourceInspection.accepted,
      fileName: "p2-06-resource-soak.json",
      sha256: sha256(resourceBytes),
      gitCommit: resource?.gitCommit ?? null,
      startedAt: resource?.startedAt ?? null,
      completedAt: resource?.completedAt ?? null,
      durationSeconds: resource?.durationSeconds ?? null,
      fixtureCount: resource?.fixtureCount ?? null,
      environment: resource?.environment ?? null,
      summary: resource?.summary ?? null,
    },
    p2A12: {
      accepted: platformInspection.accepted && hostedRunInspection.accepted,
      matrixArtifactName: platformBundle?.matrixArtifactName ?? null,
      matrixSha256: sha256(platformBundle?.matrixBytes),
      hostedRunReceiptSha256: sha256(hostedRunBytes),
      hostedRunVerifiedAt: hostedRun?.verifiedAt ?? null,
      gitCommit: matrix?.gitCommit ?? null,
      verifiedAt: matrix?.verifiedAt ?? null,
      githubRunAttempt: matrix?.workflow?.githubRunAttempt ?? null,
      runUrl: matrix?.workflow?.runUrl ?? null,
      verificationEnvironment: matrix?.verificationEnvironment ?? null,
      artifacts: matrix?.artifacts ?? [],
      hostedJobs: hostedRun?.jobs ?? [],
    },
  };
}

function sha256(bytes) {
  return Buffer.isBuffer(bytes) || bytes instanceof Uint8Array
    ? createHash("sha256").update(bytes).digest("hex")
    : null;
}

function laterIsoInstant(...candidates) {
  const values = candidates.filter(isIsoInstant);
  return values.length === 0
    ? null
    : values
        .toSorted((first, second) => Date.parse(first) - Date.parse(second))
        .at(-1);
}

function isIsoInstant(value) {
  if (typeof value !== "string") return false;
  const timestamp = Date.parse(value);
  return (
    Number.isFinite(timestamp) && new Date(timestamp).toISOString() === value
  );
}
