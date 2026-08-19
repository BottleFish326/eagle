import { createHash } from "node:crypto";

export const P2_TRANSACTION_COUNT = 1_000;
export const P2_TRANSACTION_ABORT_AFTER = 317;
export const P2_CACHE_FAULT_POINTS = Object.freeze([
  "after-cache-rename",
  "after-cache-recreate",
]);

export function buildP2LocalFaultGatesReport({
  gitCommit,
  executedAt,
  environment,
  binarySha256,
  repositoryClean,
  transaction,
  cacheCases,
  temporaryWorkspacesRemoved,
}) {
  const failures = [];
  if (!isCommit(gitCommit)) failures.push("local fault gate commit is invalid");
  if (!isIsoInstant(executedAt))
    failures.push("local fault gate execution time is invalid");
  if (environment?.nodeVersion?.startsWith("v24.") !== true)
    failures.push("local fault gates require Node.js 24.x");
  for (const field of ["platform", "architecture", "rustc", "cargo"])
    if (typeof environment?.[field] !== "string" || environment[field] === "")
      failures.push(`local fault gate environment ${field} is missing`);
  for (const name of ["transactionFault", "cacheFault"])
    if (!isSha256(binarySha256?.[name]))
      failures.push(`local fault gate binary digest is invalid: ${name}`);
  if (repositoryClean !== true)
    failures.push("local fault gate repository was not clean");

  const transactionSummary = inspectTransactionFault(transaction, failures);
  const cacheSummaries = inspectCacheFaults(cacheCases, failures);
  if (temporaryWorkspacesRemoved !== true)
    failures.push("local fault gate temporary workspaces were not removed");

  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    gitCommit,
    executedAt,
    repositoryClean: repositoryClean === true,
    environment,
    build: {
      command:
        "cargo build --locked --release -p transaction-fault -p cache-fault",
      binaries: binarySha256,
    },
    p2A04: transactionSummary,
    p2A10: { cases: cacheSummaries },
    temporaryWorkspacesRemoved: temporaryWorkspacesRemoved === true,
  };
}

function inspectTransactionFault(value, failures) {
  const abort = summarizeProcess(value?.abort);
  const recover = summarizeProcess(value?.recover);
  if (!isAbort(value?.abort))
    failures.push("P2-A04 transaction process did not abort as required");
  if (!isSuccess(value?.recover))
    failures.push("P2-A04 transaction recovery did not exit successfully");

  const recovery = value?.recover?.stdout?.match(
    /^discovered ([0-9a-f-]{36}) Active applied=([0-9]+)\r?\nrecovered ([0-9a-f-]{36}) ([0-9]+)\r?\n?$/u,
  );
  const transactionId = recovery?.[1] ?? null;
  const appliedAtDiscovery = numberOrNull(recovery?.[2]);
  const recoveredCount = numberOrNull(recovery?.[4]);
  if (
    !isUuidV7(transactionId) ||
    recovery?.[3] !== transactionId ||
    appliedAtDiscovery !== P2_TRANSACTION_ABORT_AFTER ||
    recoveredCount !== P2_TRANSACTION_COUNT
  )
    failures.push("P2-A04 recovery output does not prove 317 -> 1000");

  return {
    count: P2_TRANSACTION_COUNT,
    abortAfterApplied: P2_TRANSACTION_ABORT_AFTER,
    abort,
    recover,
    transactionId,
    discoveredState: recovery === null ? null : "active",
    appliedAtDiscovery,
    recoveredCount,
  };
}

function inspectCacheFaults(values, failures) {
  if (!Array.isArray(values)) {
    failures.push("P2-A10 cache fault cases are not an array");
    return [];
  }
  if (values.length !== P2_CACHE_FAULT_POINTS.length)
    failures.push("P2-A10 requires exactly two cache fault cases");
  const summaries = [];
  for (const faultPoint of P2_CACHE_FAULT_POINTS) {
    const matches = values.filter((value) => value?.faultPoint === faultPoint);
    if (matches.length !== 1) {
      failures.push(`P2-A10 requires exactly one cache case: ${faultPoint}`);
      continue;
    }
    const value = matches[0];
    if (!isSuccess(value.seed))
      failures.push(`P2-A10 cache seed failed: ${faultPoint}`);
    if (!isAbort(value.abort))
      failures.push(`P2-A10 cache process did not abort: ${faultPoint}`);
    if (!isSuccess(value.recover))
      failures.push(`P2-A10 cache recovery failed: ${faultPoint}`);
    const seedVerified = /^seeded cache=1 asset=true sidecar=true\r?\n?$/u.test(
      value.seed?.stdout ?? "",
    );
    const recoveryVerified =
      /^recovered disposition=maintained cache=1 asset=true sidecar=true\r?\n?$/u.test(
        value.recover?.stdout ?? "",
      );
    if (!seedVerified)
      failures.push(`P2-A10 seed output is incomplete: ${faultPoint}`);
    if (!recoveryVerified)
      failures.push(`P2-A10 recovery output is incomplete: ${faultPoint}`);
    summaries.push({
      faultPoint,
      seed: summarizeProcess(value.seed),
      abort: summarizeProcess(value.abort),
      recover: summarizeProcess(value.recover),
      seedVerified,
      recoveryVerified,
    });
  }
  return summaries;
}

function summarizeProcess(value) {
  return {
    status: Number.isInteger(value?.status) ? value.status : null,
    signal: typeof value?.signal === "string" ? value.signal : null,
    stdoutSha256: sha256Text(value?.stdout),
    stderrSha256: sha256Text(value?.stderr),
  };
}

function isSuccess(value) {
  return value?.error == null && value?.status === 0 && value?.signal == null;
}

function isAbort(value) {
  return (
    value?.error == null &&
    ((value?.status == null && value?.signal === "SIGABRT") ||
      (Number.isInteger(value?.status) &&
        value.status !== 0 &&
        value?.signal == null))
  );
}

function sha256Text(value) {
  return createHash("sha256")
    .update(typeof value === "string" ? value : "")
    .digest("hex");
}

function numberOrNull(value) {
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : null;
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}

function isSha256(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function isUuidV7(value) {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u.test(
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
