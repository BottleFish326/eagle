import path from "node:path";

export const REQUIRED_P2_HOSTED_JOBS = Object.freeze([
  "Path compatibility (ubuntu-24.04)",
  "Path compatibility (macos-15)",
  "Path compatibility (windows-2025)",
  "Consolidate path compatibility evidence",
  "Format, test, and build",
]);

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
      jobs: [],
      runUrl: null,
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
  const expectedAttemptUrl = `${expectedUrl}/attempts/${String(requestedAttempt)}`;
  if (run.url !== expectedUrl && run.url !== expectedAttemptUrl)
    failures.push("hosted run URL does not match repository and run ID");
  if (!isOrderedIsoRange(run.createdAt, run.startedAt, run.updatedAt))
    failures.push("hosted run timestamps are invalid or unordered");

  const jobs = [];
  if (!Array.isArray(run.jobs)) {
    failures.push("hosted run jobs are not an array");
  } else {
    for (const name of REQUIRED_P2_HOSTED_JOBS) {
      const matches = run.jobs.filter((job) => job?.name === name);
      if (matches.length !== 1) {
        failures.push(
          `hosted run must contain exactly one required job: ${name}`,
        );
        continue;
      }
      const job = matches[0];
      const id = job.databaseId;
      if (!isPositiveInteger(id))
        failures.push(`hosted job ID is invalid: ${name}`);
      if (job.status !== "completed")
        failures.push(`hosted job is not completed: ${name}`);
      if (job.conclusion !== "success")
        failures.push(`hosted job conclusion is not success: ${name}`);
      if (!isOrderedIsoRange(job.startedAt, job.completedAt))
        failures.push(
          `hosted job timestamps are invalid or unordered: ${name}`,
        );
      if (job.url !== `${expectedUrl}/job/${String(id)}`)
        failures.push(`hosted job URL is invalid: ${name}`);
      jobs.push({
        databaseId: id,
        name,
        status: job.status,
        conclusion: job.conclusion,
        startedAt: job.startedAt,
        completedAt: job.completedAt,
        url: job.url,
      });
    }
  }
  return {
    accepted: failures.length === 0,
    failures,
    jobs,
    runUrl: expectedUrl,
  };
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
  repositorySlug,
  patterns,
  downloadDirectory,
  downloadArtifacts,
  archiveEvidence,
  removeDownloadDirectory,
  publishEvidence,
}) {
  if (inspection?.accepted !== true)
    throw new Error(
      `P2-A12 hosted run rejected: ${inspection?.failures?.join("; ") ?? "invalid inspection"}`,
    );
  const acceptedRun = { ...run, url: inspection.runUrl };
  const expectedPatterns = p2HostedArtifactPatterns(
    acceptedRun.headSha,
    acceptedRun.attempt,
  );
  if (JSON.stringify(patterns) !== JSON.stringify(expectedPatterns))
    throw new Error("P2-A12 artifact patterns do not match the accepted run");
  await downloadArtifacts({ patterns, downloadDirectory });
  const archive = await archiveEvidence({ downloadDirectory });
  if (archive?.archived !== true)
    throw new Error("P2-A12 archive did not report success");
  const receipt = buildP2HostedRunReceipt({
    inspection,
    run: acceptedRun,
    repositorySlug,
    archive,
  });
  if (!receipt.accepted)
    throw new Error(
      `P2-A12 hosted receipt rejected: ${receipt.failures.join("; ")}`,
    );
  await removeDownloadDirectory(downloadDirectory);
  const acceptedReceipt = {
    ...receipt,
    temporaryDownloadRemoved: true,
  };
  await publishEvidence(acceptedReceipt);
  return acceptedReceipt;
}

export function buildP2HostedRunReceipt({
  inspection,
  run,
  repositorySlug,
  archive,
}) {
  const failures = [...(inspection?.failures ?? [])];
  if (inspection?.accepted !== true)
    failures.push("hosted run inspection is not accepted");
  if (archive?.archived !== true)
    failures.push("platform evidence archive is not successful");
  if (archive?.gitCommit !== run?.headSha)
    failures.push("archive commit does not match hosted run");
  if (String(archive?.githubRunAttempt) !== String(run?.attempt))
    failures.push("archive attempt does not match hosted run");
  if (archive?.runUrl !== run?.url)
    failures.push("archive URL does not match hosted run");
  const files = normalizeArchiveFiles(archive?.files, failures);
  const jobs = inspection?.jobs ?? [];
  if (jobs.length !== REQUIRED_P2_HOSTED_JOBS.length)
    failures.push("hosted run receipt does not contain all required jobs");
  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    repository: repositorySlug,
    verifiedAt: run?.updatedAt ?? null,
    run: {
      databaseId: run?.databaseId ?? null,
      attempt: run?.attempt ?? null,
      event: run?.event ?? null,
      headBranch: run?.headBranch ?? null,
      headSha: run?.headSha ?? null,
      status: run?.status ?? null,
      conclusion: run?.conclusion ?? null,
      workflowName: run?.workflowName ?? null,
      createdAt: run?.createdAt ?? null,
      startedAt: run?.startedAt ?? null,
      updatedAt: run?.updatedAt ?? null,
      url: run?.url ?? null,
    },
    jobs,
    archive: {
      gitCommit: archive?.gitCommit ?? null,
      githubRunAttempt: archive?.githubRunAttempt ?? null,
      runUrl: archive?.runUrl ?? null,
      files,
    },
  };
}

function normalizeArchiveFiles(values, failures) {
  if (!Array.isArray(values) || values.length !== 4) {
    failures.push("platform evidence archive must contain exactly four files");
    return [];
  }
  const files = values
    .map((value) => ({
      relativePath:
        typeof value?.relativePath === "string"
          ? value.relativePath.split(path.sep).join("/")
          : null,
      sha256: value?.sha256 ?? null,
      bytes: value?.bytes ?? null,
    }))
    .toSorted((left, right) => {
      const first = String(left.relativePath);
      const second = String(right.relativePath);
      return first < second ? -1 : first > second ? 1 : 0;
    });
  if (new Set(files.map((file) => file.relativePath)).size !== files.length)
    failures.push("platform evidence archive file paths are not unique");
  for (const file of files) {
    if (
      typeof file.relativePath !== "string" ||
      !/^[^/]+\/[^/]+\.json$/u.test(file.relativePath)
    )
      failures.push("platform evidence archive path is invalid");
    if (typeof file.sha256 !== "string" || !/^[0-9a-f]{64}$/u.test(file.sha256))
      failures.push("platform evidence archive SHA-256 is invalid");
    if (!isPositiveInteger(file.bytes))
      failures.push("platform evidence archive byte count is invalid");
  }
  return files;
}

function isPositiveInteger(value) {
  return Number.isSafeInteger(value) && value > 0;
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}

function isOrderedIsoRange(...values) {
  if (values.length < 2 || !values.every(isIsoInstant)) return false;
  const times = values.map(Date.parse);
  return times.every(
    (value, index) => index === 0 || value >= times[index - 1],
  );
}

function isIsoInstant(value) {
  if (typeof value !== "string") return false;
  return (
    /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?Z$/u.test(value) &&
    Number.isFinite(Date.parse(value))
  );
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
