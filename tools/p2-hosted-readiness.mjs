export function buildP2HostedReadiness({
  ghAvailable,
  ghAuthenticated,
  remoteUrl,
  repositorySlug,
  defaultBranch,
  branch,
  upstream,
  currentCommit,
  remoteCommit,
  cleanTracked,
  workflowDispatchConfigured,
}) {
  const failures = [];
  const remoteSlug = githubRepositorySlug(remoteUrl);
  if (ghAvailable !== true) failures.push("GitHub CLI is not installed");
  if (ghAvailable === true && ghAuthenticated !== true)
    failures.push("GitHub CLI is not authenticated for github.com");
  if (remoteSlug === null)
    failures.push("origin is not a supported github.com repository URL");
  if (remoteSlug !== null && repositorySlug !== remoteSlug)
    failures.push("GitHub CLI repository does not match origin");
  if (defaultBranch !== "main")
    failures.push("GitHub default branch is not main");
  if (branch !== "main") failures.push("current branch is not main");
  if (upstream !== "origin/main")
    failures.push("main does not track origin/main");
  if (!isCommit(currentCommit)) failures.push("current commit is invalid");
  if (!isCommit(remoteCommit))
    failures.push("origin/main commit is unavailable");
  if (
    isCommit(currentCommit) &&
    isCommit(remoteCommit) &&
    currentCommit !== remoteCommit
  )
    failures.push("current HEAD is not published at origin/main");
  if (cleanTracked !== true) failures.push("tracked files are not clean");
  if (workflowDispatchConfigured !== true)
    failures.push("CI workflow does not expose workflow_dispatch");

  return {
    schema: 1,
    ready: failures.length === 0,
    failures,
    git: {
      branch,
      upstream,
      currentCommit,
      remoteCommit,
      cleanTracked: cleanTracked === true,
      origin: remoteUrl ?? null,
    },
    github: {
      cliAvailable: ghAvailable === true,
      authenticated: ghAuthenticated === true,
      repository: repositorySlug ?? null,
      originRepository: remoteSlug,
      defaultBranch: defaultBranch ?? null,
    },
    workflow: {
      path: ".github/workflows/ci.yml",
      manualDispatch: workflowDispatchConfigured === true,
    },
    commands:
      failures.length === 0
        ? buildCommands({
            repositorySlug: remoteSlug,
            currentCommit,
          })
        : [],
  };
}

export function githubRepositorySlug(value) {
  if (typeof value !== "string") return null;
  const match = value.match(
    /^(?:https?:\/\/github\.com\/|git@github\.com:|ssh:\/\/git@github\.com\/)([^\s/]+\/[^\s/]+?)(?:\.git)?\/?$/u,
  );
  return match?.[1] ?? null;
}

function buildCommands({ repositorySlug, currentCommit }) {
  if (repositorySlug === null || !isCommit(currentCommit)) return [];
  return [
    `gh workflow run ci.yml --ref main -R ${repositorySlug}`,
    `gh run list --workflow ci.yml --branch main --commit ${currentCommit} --event workflow_dispatch --limit 1 --json attempt,databaseId,headSha,status,conclusion,url -R ${repositorySlug}`,
    `gh run watch <run-id> --exit-status --compact -R ${repositorySlug}`,
    `gh run download <run-id> -R ${repositorySlug} -p 'p2-a12-source-*-${currentCommit}-attempt-<attempt>' -p 'p2-a12-matrix-${currentCommit}-attempt-<attempt>' -D <download-directory>`,
    "node tools/archive-platform-matrix-evidence.mjs --input-directory <download-directory>",
  ];
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}
