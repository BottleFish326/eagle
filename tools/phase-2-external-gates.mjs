import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";

import { inspectPlatformMatrixArchive } from "./platform-matrix-archive.mjs";
import { REQUIRED_P2_HOSTED_JOBS } from "./p2-hosted-evidence.mjs";
import { inspectP2HostedRunReceipt } from "./p2-hosted-run-receipt.mjs";
import { expectedPlatformPathTests } from "./platform-path-evidence.mjs";
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

export function inspectPhase2ExternalGatesReceipt(value) {
  const failures = [];
  checkExactKeys(
    value,
    [
      "schema",
      "accepted",
      "failures",
      "evidenceAt",
      "commitOrderVerified",
      "p2A11",
      "p2A12",
    ],
    failures,
    "phase 2 external gate receipt",
  );
  if (value?.schema !== 1)
    failures.push("phase 2 external gate receipt schema is invalid");
  if (value?.accepted !== true)
    failures.push("phase 2 external gate receipt is not accepted");
  if (!isEmptyArray(value?.failures))
    failures.push("phase 2 external gate receipt failures are not empty");
  if (!isIsoInstant(value?.evidenceAt))
    failures.push("phase 2 external gate receipt evidence time is invalid");
  if (value?.commitOrderVerified !== true)
    failures.push("phase 2 external gate receipt commit order is unverified");

  inspectResourceReceipt(value?.p2A11, failures);
  inspectPlatformReceipt(value?.p2A12, failures);
  const expectedEvidenceAt = laterIsoInstant(
    value?.p2A11?.completedAt,
    value?.p2A12?.verifiedAt,
    value?.p2A12?.hostedRunVerifiedAt,
  );
  if (value?.evidenceAt !== expectedEvidenceAt)
    failures.push("phase 2 external gate evidence time is not deterministic");

  return { accepted: failures.length === 0, failures };
}

function inspectResourceReceipt(value, failures) {
  checkExactKeys(
    value,
    [
      "accepted",
      "fileName",
      "sha256",
      "gitCommit",
      "startedAt",
      "completedAt",
      "durationSeconds",
      "fixtureCount",
      "environment",
      "summary",
    ],
    failures,
    "P2-A11 external receipt",
  );
  if (value?.accepted !== true)
    failures.push("P2-A11 external receipt is not accepted");
  if (value?.fileName !== "p2-06-resource-soak.json")
    failures.push("P2-A11 external receipt filename is invalid");
  if (!isSha256(value?.sha256))
    failures.push("P2-A11 external receipt digest is invalid");
  if (!isCommit(value?.gitCommit))
    failures.push("P2-A11 external receipt commit is invalid");
  if (!isOrderedIsoRange(value?.startedAt, value?.completedAt)) {
    failures.push("P2-A11 external receipt timestamps are invalid");
  } else if (
    Date.parse(value.completedAt) - Date.parse(value.startedAt) <
    FORMAL_RESOURCE_STABILITY_OPTIONS.durationSeconds * 1_000
  ) {
    failures.push(
      "P2-A11 external receipt duration is shorter than eight hours",
    );
  }
  if (
    value?.durationSeconds !== FORMAL_RESOURCE_STABILITY_OPTIONS.durationSeconds
  )
    failures.push("P2-A11 external receipt duration is not 28800 seconds");
  if (value?.fixtureCount !== FORMAL_RESOURCE_STABILITY_OPTIONS.fixtureCount)
    failures.push("P2-A11 external receipt fixture count is not 100000");
  inspectResourceEnvironment(value?.environment, failures);
  inspectResourceSummary(value?.summary, failures);
}

function inspectResourceEnvironment(value, failures) {
  checkExactKeys(
    value,
    ["platform", "architecture", "nodeVersion"],
    failures,
    "P2-A11 external environment",
  );
  for (const field of ["platform", "architecture"])
    if (!isNonemptyString(value?.[field]))
      failures.push(`P2-A11 external environment ${field} is invalid`);
  if (!isNode24(value?.nodeVersion))
    failures.push("P2-A11 external environment Node.js is not 24.x");
}

function inspectResourceSummary(value, failures) {
  const fields = [
    "nativeSampleCount",
    "minimumNativeSampleCount",
    "internalSampleCount",
    "minimumInternalSampleCount",
    "invalidInternalSampleCount",
    "invalidExternalSampleCount",
    "rssGrowthKiB",
    "rssSlopeKiBPerMinute",
    "maxRssKiB",
    "handleGrowth",
    "maxHandles",
    "threadBaseline",
    "maxThreads",
    "maxCpuPercent",
    "scanPasses",
    "generatedEvents",
    "watcherBatches",
    "thumbnailRequests",
    "hashRequests",
    "cacheEntries",
    "scheduler",
  ];
  checkExactKeys(value, fields, failures, "P2-A11 external summary");
  const minimumInternalSampleCount = minimumCoverageSamples(
    FORMAL_RESOURCE_STABILITY_OPTIONS.durationSeconds,
  );
  const minimumNativeSampleCount = minimumCoverageSamples(
    FORMAL_RESOURCE_STABILITY_OPTIONS.durationSeconds -
      FORMAL_RESOURCE_STABILITY_OPTIONS.warmupSeconds,
  );
  if (value?.minimumInternalSampleCount !== minimumInternalSampleCount)
    failures.push("P2-A11 external minimum internal sample count is invalid");
  if (value?.minimumNativeSampleCount !== minimumNativeSampleCount)
    failures.push("P2-A11 external minimum native sample count is invalid");
  if (
    !isNonnegativeInteger(value?.internalSampleCount) ||
    value.internalSampleCount < minimumInternalSampleCount
  )
    failures.push("P2-A11 external internal sample coverage is invalid");
  if (
    !isNonnegativeInteger(value?.nativeSampleCount) ||
    value.nativeSampleCount < minimumNativeSampleCount
  )
    failures.push("P2-A11 external native sample coverage is invalid");
  for (const field of [
    "invalidInternalSampleCount",
    "invalidExternalSampleCount",
  ])
    if (value?.[field] !== 0)
      failures.push(`P2-A11 external ${field} is not zero`);
  if (!isFiniteNumber(value?.rssGrowthKiB) || value.rssGrowthKiB > 262_144)
    failures.push("P2-A11 external RSS growth is invalid");
  if (
    !isFiniteNumber(value?.rssSlopeKiBPerMinute) ||
    value.rssSlopeKiBPerMinute > 8_192
  )
    failures.push("P2-A11 external RSS slope is invalid");
  if (!isNonnegativeNumber(value?.maxRssKiB))
    failures.push("P2-A11 external maximum RSS is invalid");
  if (!Number.isSafeInteger(value?.handleGrowth) || value.handleGrowth > 64)
    failures.push("P2-A11 external handle growth is invalid");
  for (const field of ["maxHandles", "threadBaseline", "maxThreads"])
    if (!isNonnegativeInteger(value?.[field]))
      failures.push(`P2-A11 external ${field} is invalid`);
  if (
    isNonnegativeInteger(value?.maxThreads) &&
    isNonnegativeInteger(value?.threadBaseline) &&
    value.maxThreads > value.threadBaseline + 16
  )
    failures.push("P2-A11 external thread growth is invalid");
  if (!isNonnegativeNumber(value?.maxCpuPercent))
    failures.push("P2-A11 external maximum CPU is invalid");
  for (const field of [
    "scanPasses",
    "generatedEvents",
    "watcherBatches",
    "thumbnailRequests",
    "hashRequests",
  ])
    if (!isPositiveInteger(value?.[field]))
      failures.push(`P2-A11 external ${field} is invalid`);
  if (!isNonnegativeInteger(value?.cacheEntries) || value.cacheEntries > 20_000)
    failures.push("P2-A11 external cache entry count is invalid");
  inspectScheduler(value?.scheduler, value?.maxCpuPercent, failures);
}

function inspectScheduler(value, maxCpuPercent, failures) {
  checkExactKeys(
    value,
    [
      "mode",
      "activeTotal",
      "waitingTotal",
      "peakActiveTotal",
      "peakWaitingTotal",
      "foregroundLimit",
      "backgroundLimit",
      "maxWaiters",
      "scan",
      "hash",
      "decode",
    ],
    failures,
    "P2-A11 external scheduler",
  );
  if (!["foreground", "background"].includes(value?.mode))
    failures.push("P2-A11 external scheduler mode is invalid");
  for (const field of [
    "foregroundLimit",
    "maxWaiters",
    "activeTotal",
    "waitingTotal",
    "peakActiveTotal",
    "peakWaitingTotal",
    "backgroundLimit",
  ])
    if (!isNonnegativeInteger(value?.[field]))
      failures.push(`P2-A11 external scheduler ${field} is invalid`);
  for (const kind of ["scan", "hash", "decode"])
    inspectWorkSnapshot(value?.[kind], kind, failures);
  if (
    isNonnegativeInteger(value?.backgroundLimit) &&
    isNonnegativeInteger(value?.foregroundLimit) &&
    value.backgroundLimit > value.foregroundLimit
  )
    failures.push(
      "P2-A11 external scheduler background capacity exceeds foreground capacity",
    );
  if (
    isNonnegativeInteger(value?.activeTotal) &&
    isNonnegativeInteger(value?.foregroundLimit) &&
    value.activeTotal > value.foregroundLimit
  )
    failures.push("P2-A11 external scheduler active work is unbounded");
  if (
    isNonnegativeInteger(value?.waitingTotal) &&
    isNonnegativeInteger(value?.maxWaiters) &&
    value.waitingTotal > value.maxWaiters
  )
    failures.push("P2-A11 external scheduler waiting work is unbounded");
  if (
    isNonnegativeInteger(value?.peakActiveTotal) &&
    isNonnegativeInteger(value?.foregroundLimit) &&
    value.peakActiveTotal > value.foregroundLimit
  )
    failures.push("P2-A11 external scheduler peak active work is unbounded");
  if (
    isNonnegativeInteger(value?.peakWaitingTotal) &&
    isNonnegativeInteger(value?.maxWaiters) &&
    value.peakWaitingTotal > value.maxWaiters
  )
    failures.push("P2-A11 external scheduler peak waiting work is unbounded");
  if (
    isNonnegativeNumber(maxCpuPercent) &&
    isNonnegativeInteger(value?.foregroundLimit) &&
    maxCpuPercent > value.foregroundLimit * 100 + 50
  )
    failures.push("P2-A11 external CPU exceeds its scheduler envelope");
}

function inspectWorkSnapshot(value, kind, failures) {
  const fields = [
    "active",
    "waiting",
    "peakActive",
    "peakWaiting",
    "completed",
    "rejected",
    "timedOut",
    "cancelled",
  ];
  const label = `P2-A11 external scheduler ${kind}`;
  checkExactKeys(value, fields, failures, label);
  for (const field of fields)
    if (!isNonnegativeInteger(value?.[field]))
      failures.push(`${label} ${field} is invalid`);
  if (
    isNonnegativeInteger(value?.active) &&
    isNonnegativeInteger(value?.peakActive) &&
    value.active > value.peakActive
  )
    failures.push(`${label} active work exceeds its peak`);
  if (
    isNonnegativeInteger(value?.waiting) &&
    isNonnegativeInteger(value?.peakWaiting) &&
    value.waiting > value.peakWaiting
  )
    failures.push(`${label} waiting work exceeds its peak`);
}

function inspectPlatformReceipt(value, failures) {
  checkExactKeys(
    value,
    [
      "accepted",
      "matrixArtifactName",
      "matrixSha256",
      "hostedRunReceiptSha256",
      "hostedRunVerifiedAt",
      "gitCommit",
      "verifiedAt",
      "githubRunAttempt",
      "runUrl",
      "verificationEnvironment",
      "artifacts",
      "hostedJobs",
    ],
    failures,
    "P2-A12 external receipt",
  );
  if (value?.accepted !== true)
    failures.push("P2-A12 external receipt is not accepted");
  if (!isCommit(value?.gitCommit))
    failures.push("P2-A12 external receipt commit is invalid");
  if (!isPositiveIntegerString(value?.githubRunAttempt))
    failures.push("P2-A12 external receipt run attempt is invalid");
  if (!isRunUrl(value?.runUrl))
    failures.push("P2-A12 external receipt run URL is invalid");
  if (
    value?.matrixArtifactName !==
    `p2-a12-matrix-${String(value?.gitCommit)}-attempt-${String(value?.githubRunAttempt)}`
  )
    failures.push("P2-A12 external matrix artifact name is invalid");
  for (const field of ["matrixSha256", "hostedRunReceiptSha256"])
    if (!isSha256(value?.[field]))
      failures.push(`P2-A12 external ${field} is invalid`);
  if (!isOrderedIsoRange(value?.verifiedAt, value?.hostedRunVerifiedAt))
    failures.push("P2-A12 external verification timestamps are invalid");

  const workflow = inspectVerificationEnvironment(
    value?.verificationEnvironment,
    value,
    failures,
  );
  inspectPlatformArtifacts(value?.artifacts, value, workflow, failures);
  inspectHostedJobs(value?.hostedJobs, value, failures);
}

function inspectVerificationEnvironment(value, receipt, failures) {
  const fields = [
    "nodeVersion",
    "githubActions",
    "githubSha",
    "githubRunId",
    "githubRunAttempt",
    "githubWorkflowRef",
    "githubRepository",
    "githubServerUrl",
    "runnerOs",
    "runnerArch",
    "runnerEnvironment",
  ];
  checkExactKeys(
    value,
    fields,
    failures,
    "P2-A12 external verification environment",
  );
  if (!isNode24(value?.nodeVersion))
    failures.push("P2-A12 external verification Node.js is not 24.x");
  if (value?.githubActions !== "true")
    failures.push("P2-A12 external verification is not GitHub Actions");
  if (value?.githubSha !== receipt?.gitCommit)
    failures.push("P2-A12 external verification commit does not match");
  if (!/^\d+$/u.test(value?.githubRunId ?? ""))
    failures.push("P2-A12 external verification run ID is invalid");
  if (value?.githubRunAttempt !== receipt?.githubRunAttempt)
    failures.push("P2-A12 external verification attempt does not match");
  if (!isRepositorySlug(value?.githubRepository))
    failures.push("P2-A12 external verification repository is invalid");
  const expectedWorkflowRef = `${String(value?.githubRepository)}/.github/workflows/ci.yml@refs/heads/main`;
  if (value?.githubWorkflowRef !== expectedWorkflowRef)
    failures.push("P2-A12 external verification workflow ref is invalid");
  if (value?.githubServerUrl !== "https://github.com")
    failures.push("P2-A12 external verification server is invalid");
  if (value?.runnerOs !== "Linux")
    failures.push("P2-A12 external verification runner OS is invalid");
  if (!isNonemptyString(value?.runnerArch))
    failures.push(
      "P2-A12 external verification runner architecture is invalid",
    );
  if (value?.runnerEnvironment !== "github-hosted")
    failures.push("P2-A12 external verification runner is not hosted");
  const expectedRunUrl =
    isRepositorySlug(value?.githubRepository) &&
    /^\d+$/u.test(value?.githubRunId ?? "")
      ? `https://github.com/${value.githubRepository}/actions/runs/${value.githubRunId}`
      : null;
  if (receipt?.runUrl !== expectedRunUrl)
    failures.push("P2-A12 external run URL does not match its environment");
  return value;
}

function inspectPlatformArtifacts(values, receipt, workflow, failures) {
  const platforms = ["darwin", "linux", "win32"];
  if (!Array.isArray(values) || values.length !== platforms.length) {
    failures.push("P2-A12 external receipt does not contain three artifacts");
    return;
  }
  for (const [index, platform] of platforms.entries()) {
    const value = values[index];
    inspectPlatformArtifact(value, platform, receipt, workflow, failures);
  }
}

function inspectPlatformArtifact(value, platform, receipt, workflow, failures) {
  const label = `P2-A12 ${platform} artifact`;
  checkExactKeys(
    value,
    [
      "artifactName",
      "fileName",
      "sha256",
      "platform",
      "accepted",
      "startedAt",
      "completedAt",
      "environment",
      "expectedTests",
      "listedTests",
      "executedTests",
      "summary",
    ],
    failures,
    label,
  );
  if (value?.platform !== platform)
    failures.push(`${label} platform or order is invalid`);
  if (value?.accepted !== true) failures.push(`${label} is not accepted`);
  if (value?.fileName !== "p2-a12-platform-paths.json")
    failures.push(`${label} filename is invalid`);
  if (!isSha256(value?.sha256)) failures.push(`${label} digest is invalid`);
  if (!isOrderedIsoRange(value?.startedAt, value?.completedAt)) {
    failures.push(`${label} timestamps are invalid`);
  } else if (
    isIsoInstant(receipt?.verifiedAt) &&
    Date.parse(value.completedAt) > Date.parse(receipt.verifiedAt)
  ) {
    failures.push(`${label} completed after matrix verification`);
  }

  const environment = value?.environment;
  inspectArtifactEnvironment(
    environment,
    platform,
    receipt,
    workflow,
    failures,
  );
  const expectedName = `p2-a12-source-${runnerOsFor(platform)}-${String(receipt?.gitCommit)}-attempt-${String(receipt?.githubRunAttempt)}`;
  if (value?.artifactName !== expectedName)
    failures.push(`${label} name is not commit/attempt bound`);
  const expectedTests = expectedPlatformPathTests(platform);
  for (const field of ["expectedTests", "listedTests", "executedTests"])
    if (!isDeepStrictEqual(value?.[field], expectedTests))
      failures.push(`${label} ${field} is invalid`);
  inspectPlatformSummary(value?.summary, expectedTests.length, label, failures);
}

function inspectArtifactEnvironment(
  value,
  platform,
  receipt,
  workflow,
  failures,
) {
  const label = `P2-A12 ${platform} environment`;
  const fields = [
    "architecture",
    "nodeVersion",
    "rustc",
    "cargo",
    "runnerOs",
    "runnerArch",
    "runnerEnvironment",
    "githubRunId",
    "githubRunAttempt",
    "githubWorkflowRef",
    "githubRepository",
    "githubServerUrl",
  ];
  checkExactKeys(value, fields, failures, label);
  for (const field of ["architecture", "rustc", "cargo", "runnerArch"])
    if (!isNonemptyString(value?.[field]))
      failures.push(`${label} ${field} is invalid`);
  if (!isNode24(value?.nodeVersion))
    failures.push(`${label} Node.js is not 24.x`);
  if (value?.runnerOs !== runnerOsFor(platform))
    failures.push(`${label} runner OS is invalid`);
  if (value?.runnerEnvironment !== "github-hosted")
    failures.push(`${label} runner is not hosted`);
  for (const field of [
    "githubRunId",
    "githubRunAttempt",
    "githubWorkflowRef",
    "githubRepository",
    "githubServerUrl",
  ])
    if (value?.[field] !== workflow?.[field])
      failures.push(`${label} ${field} does not match matrix verification`);
  if (value?.githubRunAttempt !== receipt?.githubRunAttempt)
    failures.push(`${label} attempt does not match external receipt`);
}

function inspectPlatformSummary(value, expectedCount, label, failures) {
  checkExactKeys(
    value,
    ["result", "passed", "failed", "ignored", "measured", "filteredOut"],
    failures,
    `${label} summary`,
  );
  if (
    value?.result !== "ok" ||
    value?.passed !== expectedCount ||
    value?.failed !== 0 ||
    value?.ignored !== 0 ||
    value?.measured !== 0 ||
    !isNonnegativeInteger(value?.filteredOut)
  )
    failures.push(`${label} summary is invalid`);
}

function inspectHostedJobs(values, receipt, failures) {
  if (
    !Array.isArray(values) ||
    values.length !== REQUIRED_P2_HOSTED_JOBS.length
  ) {
    failures.push("P2-A12 external receipt does not contain five hosted jobs");
    return;
  }
  for (const [index, name] of REQUIRED_P2_HOSTED_JOBS.entries()) {
    const value = values[index];
    const label = `P2-A12 hosted job ${name}`;
    checkExactKeys(
      value,
      [
        "databaseId",
        "name",
        "status",
        "conclusion",
        "startedAt",
        "completedAt",
        "url",
      ],
      failures,
      label,
    );
    if (!isPositiveInteger(value?.databaseId))
      failures.push(`${label} ID is invalid`);
    if (value?.name !== name)
      failures.push(`${label} name or order is invalid`);
    if (value?.status !== "completed" || value?.conclusion !== "success")
      failures.push(`${label} result is invalid`);
    if (!isOrderedIsoRange(value?.startedAt, value?.completedAt)) {
      failures.push(`${label} timestamps are invalid`);
    } else if (
      isIsoInstant(receipt?.hostedRunVerifiedAt) &&
      Date.parse(value.completedAt) > Date.parse(receipt.hostedRunVerifiedAt)
    ) {
      failures.push(`${label} completed after hosted verification`);
    }
    if (
      value?.url !==
      `${String(receipt?.runUrl)}/job/${String(value?.databaseId)}`
    )
      failures.push(`${label} URL is invalid`);
  }
}

function checkExactKeys(value, expected, failures, label) {
  if (!isRecord(value)) {
    failures.push(`${label} is not an object`);
    return;
  }
  if (
    !isDeepStrictEqual(Object.keys(value).toSorted(), [...expected].toSorted())
  )
    failures.push(`${label} fields are invalid`);
}

function minimumCoverageSamples(durationSeconds) {
  const expected =
    Math.floor(
      durationSeconds / FORMAL_RESOURCE_STABILITY_OPTIONS.sampleIntervalSeconds,
    ) + 1;
  return Math.max(2, Math.floor(expected * 0.75));
}

function runnerOsFor(platform) {
  return { darwin: "macOS", linux: "Linux", win32: "Windows" }[platform];
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isEmptyArray(value) {
  return Array.isArray(value) && value.length === 0;
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}

function isSha256(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function isNode24(value) {
  return typeof value === "string" && /^v24\./u.test(value);
}

function isRunUrl(value) {
  return (
    typeof value === "string" &&
    /^https:\/\/github\.com\/[^/\s]+\/[^/\s]+\/actions\/runs\/[1-9][0-9]*$/u.test(
      value,
    )
  );
}

function isRepositorySlug(value) {
  return typeof value === "string" && /^[^/\s]+\/[^/\s]+$/u.test(value);
}

function isPositiveIntegerString(value) {
  return typeof value === "string" && /^[1-9][0-9]*$/u.test(value);
}

function isPositiveInteger(value) {
  return Number.isSafeInteger(value) && value > 0;
}

function isNonnegativeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

function isFiniteNumber(value) {
  return typeof value === "number" && Number.isFinite(value);
}

function isNonnegativeNumber(value) {
  return isFiniteNumber(value) && value >= 0;
}

function isNonemptyString(value) {
  return typeof value === "string" && value.trim() !== "";
}

function isOrderedIsoRange(...values) {
  if (values.length < 2 || !values.every(isIsoInstant)) return false;
  const times = values.map(Date.parse);
  return times.every(
    (value, index) => index === 0 || value >= times[index - 1],
  );
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
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z$/u.test(value))
    return false;
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return false;
  const canonical = new Date(timestamp).toISOString();
  return canonical === value || canonical === value.replace(/Z$/u, ".000Z");
}
