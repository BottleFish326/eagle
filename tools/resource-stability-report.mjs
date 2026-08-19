import { buildResourceStabilityReport } from "./resource-stability-analysis.mjs";

export const FORMAL_RESOURCE_STABILITY_OPTIONS = Object.freeze({
  durationSeconds: 28_800,
  warmupSeconds: 60,
  fixtureCount: 100_000,
  sampleIntervalSeconds: 5,
  checkpointIntervalSeconds: 60,
});

export function inspectResourceStabilityReport(
  report,
  { expectedOptions = FORMAL_RESOURCE_STABILITY_OPTIONS } = {},
) {
  const failures = [];
  if (!isRecord(report)) {
    return {
      accepted: false,
      failures: ["resource stability report is not an object"],
      replayedReport: null,
    };
  }
  if (report.schema !== 1)
    failures.push("resource stability report schema is not 1");
  if (report.accepted !== true)
    failures.push("resource stability report is not accepted");
  if (!Array.isArray(report.failures) || report.failures.length !== 0)
    failures.push("resource stability report failures are not empty");
  if (!isIsoInstant(report.startedAt) || !isIsoInstant(report.completedAt)) {
    failures.push("resource stability report timestamps are not ISO instants");
  } else if (Date.parse(report.completedAt) < Date.parse(report.startedAt)) {
    failures.push("resource stability report completed before it started");
  }
  for (const field of [
    "durationSeconds",
    "warmupSeconds",
    "fixtureCount",
    "sampleIntervalSeconds",
    "checkpointIntervalSeconds",
  ]) {
    if (report[field] !== expectedOptions[field])
      failures.push(
        `resource stability ${field} is ${String(report[field])}, expected ${String(expectedOptions[field])}`,
      );
  }
  if (
    !isNonemptyString(report.environment?.platform) ||
    !isNonemptyString(report.environment?.architecture)
  )
    failures.push("resource stability platform or architecture is missing");

  let replayedReport = null;
  try {
    replayedReport = buildResourceStabilityReport({
      startedAt: new Date(report.startedAt),
      exit: report.exit,
      stderr: report.stderr,
      internalSamples: report.internalSamples,
      externalSamples: report.externalSamples,
      sampleParseErrors: report.sampleParseErrors,
      monitorErrors: report.monitorErrors,
      options: {
        durationSeconds: report.durationSeconds,
        warmupSeconds: report.warmupSeconds,
        fixtureCount: report.fixtureCount,
        sampleIntervalSeconds: report.sampleIntervalSeconds,
        checkpointIntervalSeconds: report.checkpointIntervalSeconds,
      },
      gitCommit: report.gitCommit,
      environment: report.environment,
    });
    replayedReport.completedAt = report.completedAt;
    for (const failure of replayedReport.failures)
      failures.push(`resource stability replay: ${failure}`);
    if (canonicalJson(replayedReport) !== canonicalJson(report))
      failures.push(
        "resource stability report does not equal its raw-sample replay",
      );
  } catch (error) {
    failures.push(
      `resource stability replay failed: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  return {
    accepted: failures.length === 0,
    failures,
    replayedReport,
  };
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNonemptyString(value) {
  return typeof value === "string" && value.trim() !== "";
}

function isIsoInstant(value) {
  if (typeof value !== "string") return false;
  const timestamp = Date.parse(value);
  return (
    Number.isFinite(timestamp) && new Date(timestamp).toISOString() === value
  );
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
