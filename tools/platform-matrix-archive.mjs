import { buildPlatformMatrixReport } from "./platform-matrix-analysis.mjs";

export function inspectPlatformMatrixArchive({
  matrixArtifactName,
  matrixReport,
  sources,
}) {
  const failures = [];
  if (!isRecord(matrixReport)) {
    return {
      accepted: false,
      failures: ["consolidated matrix report is not an object"],
      replayedReport: null,
    };
  }
  if (matrixReport.schema !== 1)
    failures.push("consolidated matrix schema is not 1");
  if (matrixReport.accepted !== true)
    failures.push("consolidated matrix is not accepted");
  if (
    !Array.isArray(matrixReport.failures) ||
    matrixReport.failures.length !== 0
  )
    failures.push("consolidated matrix failures are not empty");

  const expectedArtifactName = `p2-a12-matrix-${String(matrixReport.gitCommit)}-attempt-${String(matrixReport.workflow?.githubRunAttempt)}`;
  if (matrixArtifactName !== expectedArtifactName)
    failures.push("matrix artifact name does not bind commit and run attempt");

  let replayedReport = null;
  try {
    replayedReport = buildPlatformMatrixReport({
      sources,
      repositoryCommit: matrixReport.gitCommit,
      nodeVersion: matrixReport.verificationEnvironment?.nodeVersion,
      verifiedAt: matrixReport.verifiedAt,
      workflowContext: matrixReport.verificationEnvironment,
    });
    for (const failure of replayedReport.failures)
      failures.push(`offline replay: ${failure}`);
    if (!sameJsonValue(replayedReport, matrixReport))
      failures.push("downloaded matrix does not equal the offline replay");
  } catch (error) {
    failures.push(
      `offline replay failed: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  return {
    accepted: failures.length === 0,
    failures,
    replayedReport,
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
