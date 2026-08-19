export function buildP2HostedReadiness({
  ghAvailable,
  ghInstallCommand,
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
    remediations:
      failures.length === 0
        ? []
        : buildRemediations({
            ghAvailable,
            ghInstallCommand,
            ghAuthenticated,
            remoteUrl,
            remoteSlug,
            repositorySlug,
            defaultBranch,
            branch,
            upstream,
            currentCommit,
            remoteCommit,
            cleanTracked,
            workflowDispatchConfigured,
          }),
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
    "npm run collect:p2-hosted-evidence -- --run-id <run-id> --attempt <attempt>",
  ];
}

function buildRemediations({
  ghAvailable,
  ghInstallCommand,
  ghAuthenticated,
  remoteUrl,
  remoteSlug,
  repositorySlug,
  defaultBranch,
  branch,
  upstream,
  currentCommit,
  remoteCommit,
  cleanTracked,
  workflowDispatchConfigured,
}) {
  const actions = [];
  if (ghAvailable !== true)
    actions.push(
      remediation(
        "install-github-cli",
        typeof ghInstallCommand === "string" && ghInstallCommand !== ""
          ? ghInstallCommand
          : null,
        "Install GitHub CLI using the platform's trusted package manager.",
      ),
    );
  if (ghAvailable === true && ghAuthenticated !== true)
    actions.push(
      remediation(
        "authenticate-github-cli",
        "gh auth login --hostname github.com --web --git-protocol https",
        "Authenticate GitHub CLI for github.com in the user's browser.",
      ),
    );
  if (remoteSlug === null)
    actions.push(
      remediation(
        "configure-github-origin",
        remoteUrl === null
          ? "git remote add origin <github-repository-url>"
          : "git remote set-url origin <github-repository-url>",
        "Point origin at the intended github.com repository; the URL must be chosen by the user.",
      ),
    );
  if (branch !== "main")
    actions.push(
      remediation(
        "switch-main",
        "git switch main",
        "Use the local main branch for the formal hosted candidate.",
      ),
    );
  if (cleanTracked !== true)
    actions.push(
      remediation(
        "clean-tracked-worktree",
        "git status --short",
        "Inspect and commit or intentionally resolve all tracked changes.",
      ),
    );
  if (remoteSlug !== null && repositorySlug !== remoteSlug)
    actions.push(
      remediation(
        "verify-github-repository",
        `gh repo view ${remoteSlug} --json nameWithOwner,defaultBranchRef`,
        "Confirm the authenticated account can access the repository configured as origin.",
      ),
    );
  if (remoteSlug !== null && defaultBranch !== "main")
    actions.push(
      remediation(
        "set-default-main",
        `gh repo edit ${remoteSlug} --default-branch main`,
        "Set the GitHub repository default branch to main after confirming repository policy.",
      ),
    );
  if (
    remoteSlug !== null &&
    (upstream !== "origin/main" ||
      !isCommit(remoteCommit) ||
      (isCommit(currentCommit) &&
        isCommit(remoteCommit) &&
        currentCommit !== remoteCommit))
  )
    actions.push(
      remediation(
        "publish-main",
        "git push --set-upstream origin main",
        "Publish the exact clean local main commit and set origin/main as upstream.",
      ),
    );
  if (workflowDispatchConfigured !== true)
    actions.push(
      remediation(
        "enable-workflow-dispatch",
        null,
        "Add workflow_dispatch to .github/workflows/ci.yml and commit it before publication.",
      ),
    );
  return actions;
}

function remediation(kind, command, message) {
  return { kind, command, message };
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}
