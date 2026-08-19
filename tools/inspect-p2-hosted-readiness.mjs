import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";

import {
  buildP2HostedReadiness,
  githubRepositorySlug,
} from "./p2-hosted-readiness.mjs";

const repository = path.resolve(import.meta.dirname, "..");

try {
  assertNode24();
  assertNoArguments(process.argv.slice(2));
  const ghAvailable = commandSucceeds("gh", ["--version"]);
  const remoteUrl = optionalGit(["config", "--get", "remote.origin.url"]);
  const remoteSlug = githubRepositorySlug(remoteUrl);
  const currentCommit = optionalGit(["rev-parse", "HEAD"]);
  const githubRepository =
    ghAvailable && remoteSlug !== null
      ? optionalJsonCommand("gh", [
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
  const report = buildP2HostedReadiness({
    ghAvailable,
    ghAuthenticated:
      ghAvailable &&
      commandSucceeds("gh", [
        "auth",
        "status",
        "--active",
        "--hostname",
        "github.com",
      ]),
    remoteUrl,
    repositorySlug: githubRepository?.nameWithOwner ?? null,
    defaultBranch: githubRepository?.defaultBranchRef?.name ?? null,
    branch: optionalGit(["branch", "--show-current"]),
    upstream: optionalGit([
      "rev-parse",
      "--abbrev-ref",
      "--symbolic-full-name",
      "@{upstream}",
    ]),
    currentCommit,
    remoteCommit:
      remoteUrl === null
        ? null
        : parseLsRemote(
            optionalGit([
              "ls-remote",
              "--exit-code",
              "origin",
              "refs/heads/main",
            ]),
          ),
    cleanTracked:
      gitStatus(["diff", "--quiet"]) &&
      gitStatus(["diff", "--cached", "--quiet"]),
    workflowDispatchConfigured: /^ {2}workflow_dispatch:\s*$/mu.test(workflow),
  });
  console.log(JSON.stringify(report, null, 2));
  if (!report.ready) process.exitCode = 1;
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `P2 hosted readiness requires Node.js 24.x, received ${process.version}`,
    );
}

function assertNoArguments(args) {
  if (args.length !== 0)
    throw new Error("usage: node tools/inspect-p2-hosted-readiness.mjs");
}

function optionalGit(args) {
  return optionalCommand("git", args);
}

function gitStatus(args) {
  return run("git", args).status === 0;
}

function commandSucceeds(command, args) {
  return run(command, args).status === 0;
}

function optionalCommand(command, args) {
  const result = run(command, args);
  return result.status === 0 && result.stdout.trim() !== ""
    ? result.stdout.trim()
    : null;
}

function optionalJsonCommand(command, args) {
  const value = optionalCommand(command, args);
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

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: repository,
    encoding: "utf8",
    maxBuffer: 1024 * 1024,
    timeout: 15_000,
  });
  return {
    status: result.status ?? -1,
    stdout: result.stdout ?? "",
  };
}
