import {
  expectedPlatformPathTests,
  inspectHostedEvidenceContext,
  inspectPlatformPathRun,
} from "./platform-path-evidence.mjs";

const REQUIRED_PLATFORMS = ["darwin", "linux", "win32"];
const COMMIT_PATTERN = /^[0-9a-f]{40,64}$/u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;

export function buildPlatformMatrixReport({
  sources,
  repositoryCommit,
  nodeVersion,
  verifiedAt,
  workflowContext,
}) {
  const failures = [];
  const artifacts = [];

  if (!COMMIT_PATTERN.test(repositoryCommit ?? ""))
    failures.push("repository commit is not a lowercase Git object ID");
  if (!isNode24(nodeVersion))
    failures.push(
      `matrix verification requires Node.js 24.x, received ${String(nodeVersion)}`,
    );
  if (!isIsoInstant(verifiedAt))
    failures.push("verifiedAt is not an ISO instant");
  if (!Array.isArray(sources)) {
    failures.push("matrix sources must be an array");
    sources = [];
  }
  if (sources.length !== REQUIRED_PLATFORMS.length) {
    failures.push(
      `matrix contains ${String(sources.length)} source artifacts, expected 3`,
    );
  }

  for (const [index, source] of sources.entries()) {
    inspectSource({
      source,
      index,
      repositoryCommit,
      verifiedAt,
      failures,
      artifacts,
    });
  }

  const platforms = artifacts
    .map((artifact) => artifact.platform)
    .filter((platform) => REQUIRED_PLATFORMS.includes(platform));
  for (const platform of REQUIRED_PLATFORMS) {
    const count = platforms.filter(
      (candidate) => candidate === platform,
    ).length;
    if (count !== 1)
      failures.push(
        `matrix platform ${platform} appears ${String(count)} times, expected once`,
      );
  }
  const workflow = commonWorkflowContext(artifacts, failures);
  const verificationEnvironment = inspectVerificationEnvironment({
    workflowContext,
    workflow,
    repositoryCommit,
    nodeVersion,
    failures,
  });
  artifacts.sort(
    (left, right) =>
      REQUIRED_PLATFORMS.indexOf(left.platform) -
      REQUIRED_PLATFORMS.indexOf(right.platform),
  );

  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    verifiedAt,
    gitCommit: repositoryCommit,
    workflow,
    verificationEnvironment,
    artifacts,
  };
}

function inspectSource({
  source,
  index,
  repositoryCommit,
  verifiedAt,
  failures,
  artifacts,
}) {
  const label = `source[${String(index)}]`;
  if (!isRecord(source)) {
    failures.push(`${label} is not an object`);
    return;
  }
  if (!isSafeName(source.artifactName))
    failures.push(`${label} artifactName is not a safe basename`);
  if (source.fileName !== "p2-a12-platform-paths.json")
    failures.push(`${label} has unexpected evidence filename`);
  if (!SHA256_PATTERN.test(source.sha256 ?? ""))
    failures.push(`${label} SHA-256 digest is invalid`);

  const report = source.report;
  if (!isRecord(report)) {
    failures.push(`${label} report is not an object`);
    return;
  }
  if (report.schema !== 1) failures.push(`${label} report schema is not 1`);
  if (report.accepted !== true)
    failures.push(`${label} source report is not accepted`);
  if (!Array.isArray(report.failures) || report.failures.length !== 0)
    failures.push(`${label} source failures are not empty`);
  if (
    report.command !==
    "cargo test --locked -p asset-filesystem p2_platform -- --nocapture"
  )
    failures.push(`${label} command is not the formal P2-A12 command`);
  if (!COMMIT_PATTERN.test(report.gitCommit ?? ""))
    failures.push(`${label} git commit is invalid`);
  if (report.gitCommit !== repositoryCommit)
    failures.push(`${label} git commit does not match the repository commit`);
  if (!isIsoInstant(report.startedAt) || !isIsoInstant(report.completedAt)) {
    failures.push(`${label} timestamps are not ISO instants`);
  } else {
    if (Date.parse(report.completedAt) < Date.parse(report.startedAt))
      failures.push(`${label} completed before it started`);
    if (
      isIsoInstant(verifiedAt) &&
      Date.parse(report.completedAt) > Date.parse(verifiedAt)
    )
      failures.push(`${label} completed after matrix verification`);
  }

  const environment = report.environment;
  if (!isRecord(environment)) {
    failures.push(`${label} environment is not an object`);
    return;
  }
  const platform = environment.platform;
  if (!REQUIRED_PLATFORMS.includes(platform))
    failures.push(`${label} has unsupported platform ${String(platform)}`);
  if (!isNode24(environment.nodeVersion))
    failures.push(`${label} did not run with Node.js 24.x`);
  for (const field of ["architecture", "rustc", "cargo"]) {
    if (!isNonemptyString(environment[field]))
      failures.push(`${label} environment ${field} is missing`);
  }
  if (environment.gitCommit !== report.gitCommit)
    failures.push(`${label} environment commit does not match its report`);

  let inspection = null;
  if (REQUIRED_PLATFORMS.includes(platform)) {
    const processResults = report.processResults;
    if (!isRecord(processResults)) {
      failures.push(`${label} process results are missing`);
    } else {
      const list = inspectProcessResult(
        processResults.list,
        `${label} list`,
        failures,
      );
      const test = inspectProcessResult(
        processResults.test,
        `${label} test`,
        failures,
      );
      if (list !== null && test !== null) {
        inspection = inspectPlatformPathRun({
          platform,
          listStatus: list.status,
          listOutput: `${list.stdout}\n${list.stderr}`,
          testStatus: test.status,
          testOutput: `${test.stdout}\n${test.stderr}`,
          requireWindowsSymlink: report.requireWindowsSymlink === true,
        });
        for (const failure of inspection.failures)
          failures.push(`${label} replay: ${failure}`);
        compareStored(
          `${label} expectedTests`,
          report.expectedTests,
          inspection.expectedTests,
          failures,
        );
        compareStored(
          `${label} listedTests`,
          report.listedTests,
          inspection.listedTests,
          failures,
        );
        compareStored(
          `${label} executedTests`,
          report.executedTests,
          inspection.executedTests,
          failures,
        );
        compareStored(
          `${label} summary`,
          report.summary,
          inspection.summary,
          failures,
        );
      }
    }

    if (report.requireWindowsSymlink !== (platform === "win32"))
      failures.push(`${label} has an invalid native symlink requirement`);
    for (const failure of inspectHostedEvidenceContext({
      platform,
      gitCommit: report.gitCommit,
      environment,
    })) {
      failures.push(`${label} hosted context: ${failure}`);
    }

    const expectedArtifactName = `p2-a12-source-${environment.runnerOs}-${report.gitCommit}-attempt-${environment.githubRunAttempt}`;
    if (source.artifactName !== expectedArtifactName)
      failures.push(
        `${label} artifact name does not bind runner OS and commit`,
      );
  }

  artifacts.push({
    artifactName: source.artifactName ?? null,
    fileName: source.fileName ?? null,
    sha256: source.sha256 ?? null,
    platform: platform ?? null,
    accepted: report.accepted === true,
    startedAt: report.startedAt ?? null,
    completedAt: report.completedAt ?? null,
    environment: {
      architecture: environment.architecture ?? null,
      nodeVersion: environment.nodeVersion ?? null,
      rustc: environment.rustc ?? null,
      cargo: environment.cargo ?? null,
      runnerOs: environment.runnerOs ?? null,
      runnerArch: environment.runnerArch ?? null,
      runnerEnvironment: environment.runnerEnvironment ?? null,
      githubRunId: environment.githubRunId ?? null,
      githubRunAttempt: environment.githubRunAttempt ?? null,
      githubWorkflowRef: environment.githubWorkflowRef ?? null,
    },
    expectedTests: inspection?.expectedTests ?? safeExpectedTests(platform),
    listedTests: inspection?.listedTests ?? [],
    executedTests: inspection?.executedTests ?? [],
    summary: inspection?.summary ?? null,
  });
}

function inspectProcessResult(result, label, failures) {
  if (!isRecord(result)) {
    failures.push(`${label} process result is missing`);
    return null;
  }
  if (result.status !== 0) failures.push(`${label} exited nonzero`);
  if (result.signal !== null) failures.push(`${label} received a signal`);
  if (result.error !== null) failures.push(`${label} contains a spawn error`);
  if (typeof result.stdout !== "string" || typeof result.stderr !== "string") {
    failures.push(`${label} output is not textual`);
    return null;
  }
  return result;
}

function commonWorkflowContext(artifacts, failures) {
  const fields = ["githubRunId", "githubRunAttempt", "githubWorkflowRef"];
  const workflow = {};
  for (const field of fields) {
    const values = artifacts.map((artifact) => artifact.environment[field]);
    const first = values[0] ?? null;
    workflow[field] = first;
    if (
      typeof first !== "string" ||
      first === "" ||
      values.some((value) => value !== first)
    ) {
      failures.push(`matrix sources do not share one nonempty ${field}`);
    }
  }
  if (!/^\d+$/u.test(workflow.githubRunId ?? ""))
    failures.push("matrix githubRunId is not numeric");
  if (!/^[1-9]\d*$/u.test(workflow.githubRunAttempt ?? ""))
    failures.push("matrix githubRunAttempt is not a positive integer");
  if (
    !isNonemptyString(workflow.githubWorkflowRef) ||
    !workflow.githubWorkflowRef.includes("/.github/workflows/ci.yml@")
  )
    failures.push("matrix githubWorkflowRef is not the repository CI workflow");
  return workflow;
}

function inspectVerificationEnvironment({
  workflowContext,
  workflow,
  repositoryCommit,
  nodeVersion,
  failures,
}) {
  if (!isRecord(workflowContext)) {
    failures.push("matrix verification context is missing");
    return null;
  }
  if (workflowContext.githubActions !== "true")
    failures.push("matrix verification must run in GitHub Actions");
  if (workflowContext.githubSha !== repositoryCommit)
    failures.push("matrix verification GITHUB_SHA does not match HEAD");
  if (workflowContext.runnerEnvironment !== "github-hosted")
    failures.push("matrix verification must use a GitHub-hosted runner");
  if (workflowContext.runnerOs !== "Linux")
    failures.push("matrix verification must use the declared Linux runner");
  if (!isNonemptyString(workflowContext.runnerArch))
    failures.push("matrix verification RUNNER_ARCH is missing");
  for (const [contextField, workflowField] of [
    ["githubRunId", "githubRunId"],
    ["githubRunAttempt", "githubRunAttempt"],
    ["githubWorkflowRef", "githubWorkflowRef"],
  ]) {
    if (workflowContext[contextField] !== workflow[workflowField])
      failures.push(
        `matrix sources ${workflowField} does not match verification job`,
      );
  }
  return {
    nodeVersion,
    githubActions: workflowContext.githubActions ?? null,
    githubSha: workflowContext.githubSha ?? null,
    githubRunId: workflowContext.githubRunId ?? null,
    githubRunAttempt: workflowContext.githubRunAttempt ?? null,
    githubWorkflowRef: workflowContext.githubWorkflowRef ?? null,
    runnerOs: workflowContext.runnerOs ?? null,
    runnerArch: workflowContext.runnerArch ?? null,
    runnerEnvironment: workflowContext.runnerEnvironment ?? null,
  };
}

function compareStored(label, stored, replayed, failures) {
  if (JSON.stringify(stored) !== JSON.stringify(replayed))
    failures.push(`${label} does not match replayed raw output`);
}

function safeExpectedTests(platform) {
  try {
    return expectedPlatformPathTests(platform);
  } catch {
    return [];
  }
}

function isNode24(value) {
  return typeof value === "string" && /^v?24\./u.test(value);
}

function isIsoInstant(value) {
  if (typeof value !== "string") return false;
  const timestamp = Date.parse(value);
  return (
    Number.isFinite(timestamp) && new Date(timestamp).toISOString() === value
  );
}

function isSafeName(value) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 255 &&
    value !== "." &&
    value !== ".." &&
    !value.includes("/") &&
    !value.includes("\\")
  );
}

function isNonemptyString(value) {
  return typeof value === "string" && value.trim() !== "";
}

function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}
