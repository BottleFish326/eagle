export function inspectP2HostedRun({
  run,
  requestedRunId,
  requestedAttempt,
  expectedCommit,
  repositorySlug,
}) {
  const failures = [];
  if (!isRecord(run)) {
    return {
      accepted: false,
      failures: ["hosted run metadata is not an object"],
    };
  }
  if (String(run.databaseId) !== String(requestedRunId))
    failures.push("hosted run ID does not match the requested run");
  if (String(run.attempt) !== String(requestedAttempt))
    failures.push("hosted run attempt does not match the requested attempt");
  if (run.status !== "completed") failures.push("hosted run is not completed");
  if (run.conclusion !== "success")
    failures.push("hosted run conclusion is not success");
  if (run.event !== "workflow_dispatch")
    failures.push("hosted run was not triggered by workflow_dispatch");
  if (run.headBranch !== "main") failures.push("hosted run branch is not main");
  if (run.headSha !== expectedCommit)
    failures.push("hosted run commit does not match the published candidate");
  if (run.workflowName !== "CI")
    failures.push("hosted run workflow name is not CI");
  const expectedUrl = `https://github.com/${String(repositorySlug)}/actions/runs/${String(requestedRunId)}`;
  if (run.url !== expectedUrl)
    failures.push("hosted run URL does not match repository and run ID");
  return { accepted: failures.length === 0, failures };
}

export function p2HostedArtifactPatterns(commit, attempt) {
  if (!isCommit(commit)) throw new Error("hosted evidence commit is invalid");
  if (!isPositiveInteger(attempt))
    throw new Error("hosted evidence attempt is invalid");
  return [
    `p2-a12-source-*-${commit}-attempt-${String(attempt)}`,
    `p2-a12-matrix-${commit}-attempt-${String(attempt)}`,
  ];
}

export async function collectP2HostedEvidence({
  inspection,
  run,
  patterns,
  downloadDirectory,
  downloadArtifacts,
  archiveEvidence,
  removeDownloadDirectory,
}) {
  if (inspection?.accepted !== true)
    throw new Error(
      `P2-A12 hosted run rejected: ${inspection?.failures?.join("; ") ?? "invalid inspection"}`,
    );
  const expectedPatterns = p2HostedArtifactPatterns(run?.headSha, run?.attempt);
  if (JSON.stringify(patterns) !== JSON.stringify(expectedPatterns))
    throw new Error("P2-A12 artifact patterns do not match the accepted run");
  await downloadArtifacts({ patterns, downloadDirectory });
  const archive = await archiveEvidence({ downloadDirectory });
  if (archive?.archived !== true)
    throw new Error("P2-A12 archive did not report success");
  await removeDownloadDirectory(downloadDirectory);
  return {
    schema: 1,
    collected: true,
    run: {
      databaseId: run.databaseId,
      attempt: run.attempt,
      headSha: run.headSha,
      url: run.url,
    },
    artifactPatterns: [...patterns],
    temporaryDownloadRemoved: true,
    archive,
  };
}

function isPositiveInteger(value) {
  return Number.isSafeInteger(value) && value > 0;
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
