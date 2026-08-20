import { createHash } from "node:crypto";

export const P3_FILTER_COUNT = 64;
export const P3_SIDECAR_ABORT_AFTER = 17;
export const P3_TAG_RENAME_FAULT_CASES = Object.freeze([
  Object.freeze({ point: "coordinator-written", action: "restore" }),
  Object.freeze({ point: "sidecars-mid", action: "continue" }),
  Object.freeze({ point: "sidecars-completed", action: "restore" }),
  Object.freeze({ point: "filter-before-replace", action: "retain" }),
  Object.freeze({ point: "filter-after-replace", action: "restore" }),
]);

const UUID_V7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u;
const SHA256 = /^[0-9a-f]{64}$/u;

export function buildP3FilterGatesReport(input) {
  const failures = [];
  if (!/^[0-9a-f]{40,64}$/u.test(input.gitCommit ?? ""))
    failures.push("P3 filter gate commit is invalid");
  if (input.repositoryClean !== true)
    failures.push("P3 filter gates require a clean repository");
  if (!String(input.environment?.nodeVersion ?? "").startsWith("v24."))
    failures.push("P3 filter gates require Node.js 24.x");
  if (!SHA256.test(input.binarySha256 ?? ""))
    failures.push("P3 filter gate binary digest is invalid");

  validateP3A04(input.p3A04, failures);
  const faultCases = P3_TAG_RENAME_FAULT_CASES.map((expected, index) => {
    const actual = input.faultCases?.[index];
    if (actual?.point !== expected.point)
      failures.push(
        `P3-A05 fault point is missing or out of order: ${expected.point}`,
      );
    if (actual?.action !== expected.action)
      failures.push(`P3-A05 recovery action is invalid: ${expected.point}`);
    if (!isAborted(actual?.abort))
      failures.push(`P3-A05 process did not abort: ${expected.point}`);
    const expectedState =
      expected.action === "restore" ? "restored" : "completed";
    const expectedOutcome =
      expected.action === "restore"
        ? "restored"
        : expected.action === "retain"
          ? "retained"
          : "updated";
    validateRenameReceipt(
      actual?.recovery,
      {
        state: expectedState,
        outcome: expectedOutcome,
        action: expected.action,
      },
      expected.point,
      failures,
    );
    return {
      point: expected.point,
      action: expected.action,
      abort: processReceipt(actual?.abort),
      recovery: actual?.recovery ?? null,
    };
  });
  if (input.faultCases?.length !== P3_TAG_RENAME_FAULT_CASES.length)
    failures.push("P3-A05 requires exactly five process fault cases");

  const externalCases = [
    { target: "filter", faultPoint: "filter-before-replace" },
    { target: "sidecar", faultPoint: "filter-after-replace" },
  ].map(({ target, faultPoint }, index) => {
    const actual = input.externalCases?.[index];
    if (actual?.target !== target)
      failures.push(
        `P3-A05 external case is missing or out of order: ${target}`,
      );
    if (actual?.faultPoint !== faultPoint)
      failures.push(`P3-A05 external fault point is invalid: ${target}`);
    if (!isAborted(actual?.abort))
      failures.push(`P3-A05 external case did not abort first: ${target}`);
    validateRenameReceipt(
      actual?.recovery,
      { state: "conflict", outcome: null, action: "restore" },
      `external-${target}`,
      failures,
    );
    if (
      target === "filter" &&
      actual?.recovery?.externalFilterPreserved !== true
    )
      failures.push("P3-A05 external filter bytes were not preserved");
    if (
      target === "sidecar" &&
      actual?.recovery?.externalSidecarPreserved !== true
    )
      failures.push("P3-A05 external Sidecar bytes were not preserved");
    return {
      target,
      faultPoint,
      abort: processReceipt(actual?.abort),
      recovery: actual?.recovery ?? null,
    };
  });
  if (input.externalCases?.length !== 2)
    failures.push("P3-A05 requires exactly two external-change cases");
  if (input.temporaryWorkspacesRemoved !== true)
    failures.push("P3 filter gate temporary workspaces were not removed");

  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    gitCommit: input.gitCommit,
    executedAt: input.executedAt,
    repositoryClean: input.repositoryClean,
    environment: input.environment,
    build: {
      command: "cargo build --locked --release -p p3-filter-gate",
      binarySha256: input.binarySha256,
    },
    p3A04: input.p3A04,
    p3A05: {
      assetCount: P3_FILTER_COUNT,
      sidecarAbortAfterApplied: P3_SIDECAR_ABORT_AFTER,
      faultCases,
      externalCases,
    },
    temporaryWorkspacesRemoved: input.temporaryWorkspacesRemoved,
  };
}

export function inspectP3FilterGatesReceipt(report) {
  const failures = [];
  if (
    !hasExactKeys(report, [
      "accepted",
      "build",
      "environment",
      "executedAt",
      "failures",
      "gitCommit",
      "p3A04",
      "p3A05",
      "repositoryClean",
      "schema",
      "temporaryWorkspacesRemoved",
    ])
  )
    failures.push("P3 filter receipt fields are invalid");
  if (
    report?.schema !== 1 ||
    report?.accepted !== true ||
    report?.failures?.length !== 0
  )
    failures.push("P3 filter receipt is not accepted-only");
  if (!/^[0-9a-f]{40,64}$/u.test(report?.gitCommit ?? ""))
    failures.push("P3 filter receipt commit is invalid");
  if (
    report?.repositoryClean !== true ||
    report?.temporaryWorkspacesRemoved !== true
  )
    failures.push("P3 filter receipt cleanup provenance is invalid");
  if (!String(report?.environment?.nodeVersion ?? "").startsWith("v24."))
    failures.push("P3 filter receipt Node.js version is invalid");
  if (!SHA256.test(report?.build?.binarySha256 ?? ""))
    failures.push("P3 filter receipt binary digest is invalid");
  validateP3A04(report?.p3A04, failures);
  if (
    report?.p3A05?.assetCount !== P3_FILTER_COUNT ||
    report?.p3A05?.sidecarAbortAfterApplied !== P3_SIDECAR_ABORT_AFTER
  )
    failures.push("P3-A05 receipt constants are invalid");
  for (const [index, expected] of P3_TAG_RENAME_FAULT_CASES.entries()) {
    const actual = report?.p3A05?.faultCases?.[index];
    if (actual?.point !== expected.point || actual?.action !== expected.action)
      failures.push(`P3-A05 receipt fault case is invalid: ${expected.point}`);
    if (!isAborted(actual?.abort))
      failures.push(`P3-A05 receipt abort is invalid: ${expected.point}`);
    const expectedState =
      expected.action === "restore" ? "restored" : "completed";
    const expectedOutcome =
      expected.action === "restore"
        ? "restored"
        : expected.action === "retain"
          ? "retained"
          : "updated";
    validateRenameReceipt(
      actual?.recovery,
      {
        state: expectedState,
        outcome: expectedOutcome,
        action: expected.action,
      },
      expected.point,
      failures,
    );
  }
  if (report?.p3A05?.faultCases?.length !== 5)
    failures.push("P3-A05 receipt fault case count is invalid");
  for (const [index, expected] of [
    { target: "filter", faultPoint: "filter-before-replace" },
    { target: "sidecar", faultPoint: "filter-after-replace" },
  ].entries()) {
    const actual = report?.p3A05?.externalCases?.[index];
    if (
      actual?.target !== expected.target ||
      actual?.faultPoint !== expected.faultPoint ||
      !isAborted(actual?.abort)
    )
      failures.push(
        `P3-A05 receipt external case is invalid: ${expected.target}`,
      );
    validateRenameReceipt(
      actual?.recovery,
      { state: "conflict", outcome: null, action: "restore" },
      `external-${expected.target}`,
      failures,
    );
  }
  if (report?.p3A05?.externalCases?.length !== 2)
    failures.push("P3-A05 receipt external case count is invalid");
  return { accepted: failures.length === 0, failures };
}

function validateP3A04(value, failures) {
  const seed = value?.seed;
  const mutation = value?.mutation;
  const verify = value?.verify;
  const adversarial = value?.adversarial;
  if (
    seed?.schema !== 1 ||
    seed?.assetCount !== 4 ||
    seed?.savedFilterCount !== 2 ||
    seed?.cacheCreated !== true
  )
    failures.push("P3-A04 seed receipt is invalid");
  if (mutation?.schema !== 1 || mutation?.assetCount !== 5)
    failures.push("P3-A04 current-file mutation receipt is invalid");
  if (
    verify?.schema !== 1 ||
    verify?.accepted !== true ||
    verify?.scannedAssetCount !== 5 ||
    verify?.allEnabledMatchCount !== 4 ||
    verify?.selectedRootsMatchCount !== 5 ||
    verify?.selectedMissingRootCount !== 1 ||
    verify?.cacheAbsent !== true ||
    verify?.resultSnapshotAbsent !== true ||
    verify?.sourceSha256Unchanged !== true
  )
    failures.push("P3-A04 restart/cache receipt is invalid");
  if (
    adversarial?.schema !== 1 ||
    adversarial?.accepted !== true ||
    adversarial?.validCount !== 1 ||
    adversarial?.unavailableCount !== 1 ||
    adversarial?.invalidCount !== 6 ||
    adversarial?.unknownFieldsPreserved !== true ||
    adversarial?.externalChangeBlocked !== true ||
    adversarial?.externalBytesPreserved !== true
  )
    failures.push("P3-A04 adversarial receipt is invalid");
  if (value?.cacheRemoved !== true)
    failures.push("P3-A04 cache removal was not recorded");
}

function validateRenameReceipt(value, expected, label, failures) {
  if (
    value?.schema !== 1 ||
    !UUID_V7.test(value?.operationId ?? "") ||
    value?.state !== expected.state ||
    value?.action !== expected.action ||
    value?.assetCount !== P3_FILTER_COUNT ||
    value?.sourceSha256Unchanged !== true ||
    value?.externalFilterPreserved !== true ||
    value?.externalSidecarPreserved !== true ||
    (expected.outcome !== null && value?.filterOutcome !== expected.outcome)
  )
    failures.push(`P3-A05 recovery receipt is invalid: ${label}`);
}

function processReceipt(value) {
  return {
    status: value?.status ?? null,
    signal: value?.signal ?? null,
    stdoutSha256: digest(value?.stdout ?? ""),
    stderrSha256: digest(value?.stderr ?? ""),
  };
}

function isAborted(value) {
  return (
    (value?.error === null || value?.error === undefined) &&
    ((value?.signal === "SIGABRT" && value?.status === null) ||
      (value?.signal === null &&
        Number.isInteger(value?.status) &&
        value.status !== 0))
  );
}

function digest(value) {
  return createHash("sha256").update(value).digest("hex");
}

function hasExactKeys(value, keys) {
  return (
    value !== null &&
    typeof value === "object" &&
    JSON.stringify(Object.keys(value).toSorted()) ===
      JSON.stringify(keys.toSorted())
  );
}
