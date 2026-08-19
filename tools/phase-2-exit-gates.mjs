import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";

import { inspectP2LocalFaultGatesReceipt } from "./p2-local-fault-gates.mjs";
import { FORMAL_SOAK_BASELINE_COMMIT } from "./soak-baseline-audit.mjs";

export const P2_FINAL_ALLOWED_AFTER_LOCAL_FAULTS = Object.freeze([
  "README.md",
  "docs",
]);

export function buildPhase2ExitGatesReport({
  externalBytes,
  externalReport,
  externalReplay,
  localFaultBytes,
  localFaultReceipt,
  candidateCommit,
  workingTreeClean,
  commitOrderVerified,
  externalEvidenceInLocalCandidate,
  localEvidenceCommitted,
  soakBaselineAudit,
  localCandidateDriftPaths,
}) {
  const failures = [];
  if (!isCommit(candidateCommit))
    failures.push("phase 2 exit candidate commit is invalid");
  if (workingTreeClean !== true)
    failures.push("phase 2 exit candidate working tree is not clean");

  const externalSha256 = sha256(externalBytes);
  const localFaultSha256 = sha256(localFaultBytes);
  if (!isSha256(externalSha256))
    failures.push("phase 2 external evidence bytes are invalid");
  if (!isSha256(localFaultSha256))
    failures.push("phase 2 local fault evidence bytes are invalid");

  inspectExternalGates(externalReport, externalReplay, failures);
  const localInspection = inspectP2LocalFaultGatesReceipt(localFaultReceipt);
  for (const failure of localInspection.failures)
    failures.push(`P2-A04/P2-A10: ${failure}`);

  if (commitOrderVerified !== true)
    failures.push(
      "P2 commits are not ordered soak <= hosted matrix <= local faults <= candidate",
    );
  if (externalEvidenceInLocalCandidate !== true)
    failures.push(
      "local fault candidate does not contain the exact external gate evidence",
    );
  if (localEvidenceCommitted !== true)
    failures.push(
      "local fault evidence is not committed in the exit candidate",
    );

  inspectSoakBaseline(soakBaselineAudit, candidateCommit, failures);
  const driftPaths = normalizePaths(
    localCandidateDriftPaths,
    failures,
    "local fault candidate drift",
  );
  if (driftPaths.length > 0)
    failures.push(
      `non-documentation paths changed after local fault execution: ${driftPaths.join(", ")}`,
    );

  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    evidenceAt: laterIsoInstant(
      externalReplay?.evidenceAt,
      localFaultReceipt?.executedAt,
    ),
    candidateCommit,
    workingTreeClean: workingTreeClean === true,
    commitOrderVerified: commitOrderVerified === true,
    externalEvidenceInLocalCandidate: externalEvidenceInLocalCandidate === true,
    localEvidenceCommitted: localEvidenceCommitted === true,
    soakBaseline: {
      verified: soakBaselineAccepted(soakBaselineAudit, candidateCommit),
      baselineCommit: soakBaselineAudit?.baselineCommit ?? null,
      loadedChangedPaths: Array.isArray(
        soakBaselineAudit?.loadedInputs?.changedPaths,
      )
        ? [...soakBaselineAudit.loadedInputs.changedPaths]
        : [],
      productChangedPaths: Array.isArray(
        soakBaselineAudit?.productInputs?.changedPaths,
      )
        ? [...soakBaselineAudit.productInputs.changedPaths]
        : [],
    },
    localCandidate: {
      gitCommit: localFaultReceipt?.gitCommit ?? null,
      allowedScopes: [...P2_FINAL_ALLOWED_AFTER_LOCAL_FAULTS],
      driftPaths,
    },
    p2External: {
      fileName: "p2-external-gates.json",
      sha256: externalSha256,
      evidenceAt: externalReplay?.evidenceAt ?? null,
      p2A11Commit: externalReplay?.p2A11?.gitCommit ?? null,
      p2A12Commit: externalReplay?.p2A12?.gitCommit ?? null,
      runUrl: externalReplay?.p2A12?.runUrl ?? null,
    },
    p2LocalFaults: {
      fileName: "p2-local-fault-gates.json",
      sha256: localFaultSha256,
      gitCommit: localFaultReceipt?.gitCommit ?? null,
      executedAt: localFaultReceipt?.executedAt ?? null,
      environment: localFaultReceipt?.environment ?? null,
      binaries: localFaultReceipt?.build?.binaries ?? null,
      transaction: {
        abortAfterApplied: localFaultReceipt?.p2A04?.abortAfterApplied ?? null,
        appliedAtDiscovery:
          localFaultReceipt?.p2A04?.appliedAtDiscovery ?? null,
        recoveredCount: localFaultReceipt?.p2A04?.recoveredCount ?? null,
      },
      cacheFaultPoints: Array.isArray(localFaultReceipt?.p2A10?.cases)
        ? localFaultReceipt.p2A10.cases.map(
            (entry) => entry?.faultPoint ?? null,
          )
        : [],
      temporaryWorkspacesRemoved:
        localFaultReceipt?.temporaryWorkspacesRemoved === true,
    },
  };
}

function inspectExternalGates(stored, replayed, failures) {
  if (
    replayed?.schema !== 1 ||
    replayed?.accepted !== true ||
    !Array.isArray(replayed?.failures) ||
    replayed.failures.length !== 0 ||
    replayed?.commitOrderVerified !== true ||
    !isIsoInstant(replayed?.evidenceAt) ||
    replayed?.p2A11?.accepted !== true ||
    !isCommit(replayed?.p2A11?.gitCommit) ||
    replayed?.p2A12?.accepted !== true ||
    !isCommit(replayed?.p2A12?.gitCommit) ||
    !isRunUrl(replayed?.p2A12?.runUrl)
  )
    failures.push("P2-A11/P2-A12 external gate replay is not accepted");
  if (!isDeepStrictEqual(stored, replayed))
    failures.push("stored external gate evidence does not equal its replay");
}

function inspectSoakBaseline(value, candidateCommit, failures) {
  if (!soakBaselineAccepted(value, candidateCommit))
    failures.push(
      "formal soak baseline audit is not accepted for the candidate",
    );
}

function soakBaselineAccepted(value, candidateCommit) {
  return (
    value?.schema === 1 &&
    value?.accepted === true &&
    Array.isArray(value?.failures) &&
    value.failures.length === 0 &&
    value?.baselineCommit === FORMAL_SOAK_BASELINE_COMMIT &&
    value?.currentCommit === candidateCommit &&
    value?.descendantOfBaseline === true &&
    Array.isArray(value?.loadedInputs?.changedPaths) &&
    value.loadedInputs.changedPaths.length === 0 &&
    Array.isArray(value?.productInputs?.changedPaths) &&
    value.productInputs.changedPaths.length === 0
  );
}

function normalizePaths(values, failures, label) {
  if (!Array.isArray(values)) {
    failures.push(`${label} paths are not an array`);
    return [];
  }
  const normalized = [];
  for (const value of values) {
    if (
      typeof value !== "string" ||
      value.length === 0 ||
      value.startsWith("/") ||
      value.split("/").includes("..")
    ) {
      failures.push(`${label} contains an invalid repository path`);
      continue;
    }
    normalized.push(value);
  }
  return [...new Set(normalized)].toSorted();
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

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}

function isSha256(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function isRunUrl(value) {
  return (
    typeof value === "string" &&
    /^https:\/\/github\.com\/[^/\s]+\/[^/\s]+\/actions\/runs\/[0-9]+$/u.test(
      value,
    )
  );
}

function isIsoInstant(value) {
  if (typeof value !== "string") return false;
  const timestamp = Date.parse(value);
  return (
    Number.isFinite(timestamp) && new Date(timestamp).toISOString() === value
  );
}
