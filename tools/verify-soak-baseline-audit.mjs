import { spawnSync } from "node:child_process";
import path from "node:path";

import {
  buildSoakBaselineAudit,
  FORMAL_SOAK_BASELINE_COMMIT,
  FORMAL_SOAK_LOADED_PATHS,
  FORMAL_SOAK_PRODUCT_SCOPES,
} from "./soak-baseline-audit.mjs";

const repository = path.resolve(import.meta.dirname, "..");

try {
  assertNode24();
  assertNoArguments(process.argv.slice(2));
  assertCommitExists(FORMAL_SOAK_BASELINE_COMMIT);
  const currentCommit = git(["rev-parse", "HEAD"]).trim();
  const report = buildSoakBaselineAudit({
    baselineCommit: FORMAL_SOAK_BASELINE_COMMIT,
    currentCommit,
    descendantOfBaseline: gitStatus([
      "merge-base",
      "--is-ancestor",
      FORMAL_SOAK_BASELINE_COMMIT,
      currentCommit,
    ]),
    loadedChangedPaths: changedPaths(
      FORMAL_SOAK_BASELINE_COMMIT,
      FORMAL_SOAK_LOADED_PATHS,
    ),
    productChangedPaths: changedPaths(
      FORMAL_SOAK_BASELINE_COMMIT,
      FORMAL_SOAK_PRODUCT_SCOPES,
    ),
  });
  console.log(JSON.stringify(report, null, 2));
  if (!report.accepted) process.exitCode = 1;
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `soak baseline audit requires Node.js 24.x, received ${process.version}`,
    );
}

function assertNoArguments(args) {
  if (args.length !== 0)
    throw new Error("usage: node tools/verify-soak-baseline-audit.mjs");
}

function assertCommitExists(commit) {
  const result = runGit(["cat-file", "-e", `${commit}^{commit}`]);
  if (result.status !== 0)
    throw new Error(
      `formal soak baseline commit is unavailable: ${diagnostic(result)}`,
    );
}

function changedPaths(baselineCommit, scopes) {
  const tracked = gitNullDelimited([
    "diff",
    "--name-only",
    "--no-renames",
    "-z",
    baselineCommit,
    "--",
    ...scopes,
  ]);
  const untracked = gitNullDelimited([
    "ls-files",
    "--others",
    "--exclude-standard",
    "-z",
    "--",
    ...scopes,
  ]);
  return [...new Set([...tracked, ...untracked])].toSorted();
}

function gitNullDelimited(args) {
  const output = git(args);
  return output.split("\0").filter((entry) => entry.length > 0);
}

function git(args) {
  const result = runGit(args);
  if (result.status !== 0)
    throw new Error(`git ${args[0]} failed: ${diagnostic(result)}`);
  return result.stdout;
}

function gitStatus(args) {
  return runGit(args).status === 0;
}

function runGit(args) {
  const result = spawnSync("git", args, {
    cwd: repository,
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024,
  });
  return {
    status: result.status ?? -1,
    error: result.error?.message ?? null,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
  };
}

function diagnostic(result) {
  return (
    result.error ||
    result.stderr.trim() ||
    result.stdout.trim() ||
    `process exited with status ${String(result.status)}`
  );
}
