import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";

import { inspectP2LocalFaultGatesReceipt } from "./p2-local-fault-gates.mjs";
import { inspectPhase2ExternalGatesReceipt } from "./phase-2-external-gates.mjs";
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

export function inspectPhase2ExitGatesReceipt(value) {
  const failures = [];
  checkExactKeys(
    value,
    [
      "schema",
      "accepted",
      "failures",
      "evidenceAt",
      "candidateCommit",
      "workingTreeClean",
      "commitOrderVerified",
      "externalEvidenceInLocalCandidate",
      "localEvidenceCommitted",
      "soakBaseline",
      "localCandidate",
      "p2External",
      "p2LocalFaults",
    ],
    failures,
    "phase 2 exit receipt",
  );
  if (value?.schema !== 1)
    failures.push("phase 2 exit receipt schema is invalid");
  if (value?.accepted !== true)
    failures.push("phase 2 exit receipt is not accepted");
  if (!isEmptyArray(value?.failures))
    failures.push("phase 2 exit receipt failures are not empty");
  if (!isIsoInstant(value?.evidenceAt))
    failures.push("phase 2 exit receipt evidence time is invalid");
  if (!isCommit(value?.candidateCommit))
    failures.push("phase 2 exit receipt candidate commit is invalid");
  for (const field of [
    "workingTreeClean",
    "commitOrderVerified",
    "externalEvidenceInLocalCandidate",
    "localEvidenceCommitted",
  ])
    if (value?.[field] !== true)
      failures.push(`phase 2 exit receipt ${field} is not true`);

  inspectBaselineReceipt(value?.soakBaseline, failures);
  inspectLocalCandidateReceipt(value?.localCandidate, failures);
  inspectExternalReceiptSummary(value?.p2External, failures);
  inspectLocalFaultReceiptSummary(value?.p2LocalFaults, failures);
  if (
    isCommit(value?.localCandidate?.gitCommit) &&
    isCommit(value?.p2LocalFaults?.gitCommit) &&
    value.localCandidate.gitCommit !== value.p2LocalFaults.gitCommit
  )
    failures.push("phase 2 exit receipt local fault commits do not match");
  const expectedEvidenceAt = laterIsoInstant(
    value?.p2External?.evidenceAt,
    value?.p2LocalFaults?.executedAt,
  );
  if (value?.evidenceAt !== expectedEvidenceAt)
    failures.push("phase 2 exit receipt evidence time is not deterministic");

  return { accepted: failures.length === 0, failures };
}

function inspectBaselineReceipt(value, failures) {
  checkExactKeys(
    value,
    ["verified", "baselineCommit", "loadedChangedPaths", "productChangedPaths"],
    failures,
    "phase 2 exit soak baseline",
  );
  if (value?.verified !== true)
    failures.push("phase 2 exit soak baseline is not verified");
  if (value?.baselineCommit !== FORMAL_SOAK_BASELINE_COMMIT)
    failures.push("phase 2 exit soak baseline commit is invalid");
  for (const field of ["loadedChangedPaths", "productChangedPaths"])
    if (!isEmptyArray(value?.[field]))
      failures.push(`phase 2 exit soak baseline ${field} is not empty`);
}

function inspectLocalCandidateReceipt(value, failures) {
  checkExactKeys(
    value,
    ["gitCommit", "allowedScopes", "driftPaths"],
    failures,
    "phase 2 local candidate",
  );
  if (!isCommit(value?.gitCommit))
    failures.push("phase 2 local candidate commit is invalid");
  if (
    !Array.isArray(value?.allowedScopes) ||
    !isDeepStrictEqual(value.allowedScopes, P2_FINAL_ALLOWED_AFTER_LOCAL_FAULTS)
  )
    failures.push("phase 2 local candidate allowed scopes are invalid");
  if (!isEmptyArray(value?.driftPaths))
    failures.push("phase 2 local candidate drift paths are not empty");
}

function inspectExternalReceiptSummary(value, failures) {
  checkExactKeys(
    value,
    [
      "fileName",
      "sha256",
      "evidenceAt",
      "p2A11Commit",
      "p2A12Commit",
      "runUrl",
    ],
    failures,
    "phase 2 external receipt summary",
  );
  if (value?.fileName !== "p2-external-gates.json")
    failures.push("phase 2 external receipt filename is invalid");
  if (!isSha256(value?.sha256))
    failures.push("phase 2 external receipt digest is invalid");
  if (!isIsoInstant(value?.evidenceAt))
    failures.push("phase 2 external receipt evidence time is invalid");
  for (const field of ["p2A11Commit", "p2A12Commit"])
    if (!isCommit(value?.[field]))
      failures.push(`phase 2 external receipt ${field} is invalid`);
  if (!isRunUrl(value?.runUrl))
    failures.push("phase 2 external receipt run URL is invalid");
}

function inspectLocalFaultReceiptSummary(value, failures) {
  checkExactKeys(
    value,
    [
      "fileName",
      "sha256",
      "gitCommit",
      "executedAt",
      "environment",
      "binaries",
      "transaction",
      "cacheFaultPoints",
      "temporaryWorkspacesRemoved",
    ],
    failures,
    "phase 2 local fault receipt summary",
  );
  if (value?.fileName !== "p2-local-fault-gates.json")
    failures.push("phase 2 local fault receipt filename is invalid");
  if (!isSha256(value?.sha256))
    failures.push("phase 2 local fault receipt digest is invalid");
  if (!isCommit(value?.gitCommit))
    failures.push("phase 2 local fault receipt commit is invalid");
  if (!isIsoInstant(value?.executedAt))
    failures.push("phase 2 local fault receipt execution time is invalid");
  inspectLocalEnvironment(value?.environment, failures);
  inspectFaultBinaries(value?.binaries, failures);
  inspectFaultTransaction(value?.transaction, failures);
  if (
    !Array.isArray(value?.cacheFaultPoints) ||
    !isDeepStrictEqual(value.cacheFaultPoints, [
      "after-cache-rename",
      "after-cache-recreate",
    ])
  )
    failures.push("phase 2 local fault points are invalid");
  if (value?.temporaryWorkspacesRemoved !== true)
    failures.push("phase 2 local fault workspaces were not removed");
}

function inspectLocalEnvironment(value, failures) {
  checkExactKeys(
    value,
    ["platform", "architecture", "nodeVersion", "rustc", "cargo"],
    failures,
    "phase 2 local fault environment",
  );
  for (const field of ["platform", "architecture", "rustc", "cargo"])
    if (typeof value?.[field] !== "string" || value[field] === "")
      failures.push(`phase 2 local fault environment ${field} is invalid`);
  if (value?.nodeVersion?.startsWith("v24.") !== true)
    failures.push("phase 2 local fault environment Node.js is not 24.x");
}

function inspectFaultBinaries(value, failures) {
  checkExactKeys(
    value,
    ["transactionFault", "cacheFault"],
    failures,
    "phase 2 local fault binaries",
  );
  for (const field of ["transactionFault", "cacheFault"])
    if (!isSha256(value?.[field]))
      failures.push(`phase 2 local fault binary ${field} is invalid`);
}

function inspectFaultTransaction(value, failures) {
  checkExactKeys(
    value,
    ["abortAfterApplied", "appliedAtDiscovery", "recoveredCount"],
    failures,
    "phase 2 local fault transaction",
  );
  if (value?.abortAfterApplied !== 317)
    failures.push("phase 2 local fault abort boundary is not 317");
  if (value?.appliedAtDiscovery !== 317)
    failures.push("phase 2 local fault discovery boundary is not 317");
  if (value?.recoveredCount !== 1_000)
    failures.push("phase 2 local fault recovery count is not 1000");
}

function checkExactKeys(value, expected, failures, label) {
  if (!isRecord(value)) {
    failures.push(`${label} is not an object`);
    return;
  }
  const actual = Object.keys(value).toSorted();
  const wanted = [...expected].toSorted();
  if (!isDeepStrictEqual(actual, wanted))
    failures.push(`${label} fields are invalid`);
}

function isEmptyArray(value) {
  return Array.isArray(value) && value.length === 0;
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function inspectExternalGates(stored, replayed, failures) {
  const inspection = inspectPhase2ExternalGatesReceipt(replayed);
  for (const failure of inspection.failures)
    failures.push(`P2-A11/P2-A12 external gate replay: ${failure}`);
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
