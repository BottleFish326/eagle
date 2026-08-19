import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";

import { githubRepositorySlug } from "./p2-hosted-readiness.mjs";

export async function collectP2HostedReadinessInputs(repository) {
  const ghAvailable = commandSucceeds(repository, "gh", ["--version"]);
  const remoteUrl = optionalGit(repository, [
    "config",
    "--get",
    "remote.origin.url",
  ]);
  const remoteSlug = githubRepositorySlug(remoteUrl);
  const githubRepository =
    ghAvailable && remoteSlug !== null
      ? optionalJsonCommand(repository, "gh", [
          "repo",
          "view",
          remoteSlug,
          "--json",
          "nameWithOwner,defaultBranchRef",
        ])
      : null;
  const workflow = await readFile(
    path.join(repository, ".github", "workflows", "ci.yml"),
    "utf8",
  );
  return {
    ghAvailable,
    ghAuthenticated:
      ghAvailable &&
      commandSucceeds(repository, "gh", [
        "auth",
        "status",
        "--active",
        "--hostname",
        "github.com",
      ]),
    remoteUrl,
    repositorySlug: githubRepository?.nameWithOwner ?? null,
    defaultBranch: githubRepository?.defaultBranchRef?.name ?? null,
    branch: optionalGit(repository, ["branch", "--show-current"]),
    upstream: optionalGit(repository, [
      "rev-parse",
      "--abbrev-ref",
      "--symbolic-full-name",
      "@{upstream}",
    ]),
    currentCommit: optionalGit(repository, ["rev-parse", "HEAD"]),
    remoteCommit:
      remoteUrl === null
        ? null
        : parseLsRemote(
            optionalGit(repository, [
              "ls-remote",
              "--exit-code",
              "origin",
              "refs/heads/main",
            ]),
          ),
    cleanTracked:
      gitStatus(repository, ["diff", "--quiet"]) &&
      gitStatus(repository, ["diff", "--cached", "--quiet"]),
    workflowDispatchConfigured: /^ {2}workflow_dispatch:\s*$/mu.test(workflow),
  };
}

export function runHostedCommand(repository, command, args, timeout = 15_000) {
  const result = spawnSync(command, args, {
    cwd: repository,
    encoding: "utf8",
    maxBuffer: 8 * 1024 * 1024,
    timeout,
  });
  return {
    status: result.status ?? -1,
    error: result.error?.message ?? null,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
  };
}

function optionalGit(repository, args) {
  return optionalCommand(repository, "git", args);
}

function gitStatus(repository, args) {
  return runHostedCommand(repository, "git", args).status === 0;
}

function commandSucceeds(repository, command, args) {
  return runHostedCommand(repository, command, args).status === 0;
}

function optionalCommand(repository, command, args) {
  const result = runHostedCommand(repository, command, args);
  return result.status === 0 && result.stdout.trim() !== ""
    ? result.stdout.trim()
    : null;
}

function optionalJsonCommand(repository, command, args) {
  const value = optionalCommand(repository, command, args);
  if (value === null) return null;
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}

function parseLsRemote(value) {
  return value?.split(/\s+/u, 1)[0] ?? null;
}
