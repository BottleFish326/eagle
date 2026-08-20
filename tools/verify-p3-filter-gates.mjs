import { spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  link,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  unlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

import {
  buildP3FilterGatesReport,
  inspectP3FilterGatesReceipt,
  P3_FILTER_COUNT,
  P3_SIDECAR_ABORT_AFTER,
  P3_TAG_RENAME_FAULT_CASES,
} from "./p3-filter-gates.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const outputPath = path.join(
  repository,
  "docs",
  "reports",
  "evidence",
  "p3-a04-a05-filter-gates.json",
);
let temporaryRoot = null;
let temporaryWorkspacesRemoved = false;

try {
  assertNode24();
  if (process.argv.length !== 2)
    throw new Error("usage: node tools/verify-p3-filter-gates.mjs");
  await assertOutputAbsent(outputPath);
  const gitCommit = cleanRepositoryCommit();
  requireSuccess(
    "cargo",
    ["build", "--locked", "--release", "-p", "p3-filter-gate"],
    { timeout: 10 * 60_000 },
  );
  const binary = path.join(
    repository,
    "target",
    "release",
    process.platform === "win32" ? "p3-filter-gate.exe" : "p3-filter-gate",
  );
  const binarySha256 = createHash("sha256")
    .update(await readFile(binary))
    .digest("hex");
  temporaryRoot = await mkdtemp(
    path.join(tmpdir(), "material-eagle-p3-filters-"),
  );

  const filterWorkspace = path.join(temporaryRoot, "saved-filter-restart");
  const seed = runJson(binary, ["filter-seed", filterWorkspace]);
  const mutation = runJson(binary, ["filter-mutate-current", filterWorkspace]);
  await rm(path.join(filterWorkspace, "derived-cache"), { recursive: true });
  const verify = runJson(binary, ["filter-verify", filterWorkspace]);
  const adversarial = runJson(binary, [
    "filter-adversarial",
    path.join(temporaryRoot, "saved-filter-adversarial"),
  ]);

  const faultCases = P3_TAG_RENAME_FAULT_CASES.map(({ point, action }) => {
    const workspace = path.join(temporaryRoot, `fault-${point}`);
    const env =
      point === "sidecars-mid"
        ? {
            EAGLE_TRANSACTION_ABORT_AFTER_APPLIED: String(
              P3_SIDECAR_ABORT_AFTER,
            ),
          }
        : { EAGLE_TAG_RENAME_ABORT_AT: point };
    const abort = runProcess(
      binary,
      ["rename-execute", workspace, "--count", String(P3_FILTER_COUNT)],
      { env, timeout: 2 * 60_000 },
    );
    const recovery = runJson(binary, [
      "rename-recover",
      workspace,
      "--count",
      String(P3_FILTER_COUNT),
      "--action",
      action,
    ]);
    return { point, action, abort, recovery };
  });

  const externalCases = [
    {
      target: "filter",
      point: "filter-before-replace",
      flag: "--external-filter",
    },
    {
      target: "sidecar",
      point: "filter-after-replace",
      flag: "--external-sidecar",
    },
  ].map(({ target, point, flag }) => {
    const workspace = path.join(temporaryRoot, `external-${target}`);
    const abort = runProcess(
      binary,
      ["rename-execute", workspace, "--count", String(P3_FILTER_COUNT)],
      { env: { EAGLE_TAG_RENAME_ABORT_AT: point }, timeout: 2 * 60_000 },
    );
    const recovery = runJson(binary, [
      "rename-recover",
      workspace,
      "--count",
      String(P3_FILTER_COUNT),
      "--action",
      "restore",
      flag,
    ]);
    return { target, faultPoint: point, abort, recovery };
  });

  await rm(temporaryRoot, { recursive: true });
  temporaryWorkspacesRemoved = true;
  const report = buildP3FilterGatesReport({
    gitCommit,
    executedAt: new Date().toISOString(),
    repositoryClean: true,
    environment: {
      platform: process.platform,
      architecture: process.arch,
      nodeVersion: process.version,
      rustc: requireSuccess("rustc", ["--version"]).stdout.trim(),
      cargo: requireSuccess("cargo", ["--version"]).stdout.trim(),
    },
    binarySha256,
    p3A04: { seed, mutation, cacheRemoved: true, verify, adversarial },
    faultCases,
    externalCases,
    temporaryWorkspacesRemoved: true,
  });
  if (!report.accepted)
    throw new Error(`P3 filter gates rejected: ${report.failures.join("; ")}`);
  const inspection = inspectP3FilterGatesReceipt(report);
  if (!inspection.accepted)
    throw new Error(
      `P3 filter receipt self-check rejected: ${inspection.failures.join("; ")}`,
    );
  await writeExclusive(outputPath, report);
  console.log(JSON.stringify(report, null, 2));
} catch (error) {
  const preserved =
    temporaryRoot !== null && !temporaryWorkspacesRemoved
      ? "; temporary workspaces retained for inspection"
      : "";
  console.error(
    `${error instanceof Error ? error.message : String(error)}${preserved}`,
  );
  process.exitCode = 1;
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `P3 filter gates require Node.js 24.x, received ${process.version}`,
    );
}

function cleanRepositoryCommit() {
  const commit = requireSuccess("git", ["rev-parse", "HEAD"]).stdout.trim();
  const status = requireSuccess("git", [
    "status",
    "--porcelain=v1",
    "--untracked-files=all",
  ]).stdout;
  if (status !== "")
    throw new Error("P3 filter gates require a clean working tree");
  return commit;
}

function runJson(command, args) {
  const result = requireSuccess(command, args, { timeout: 2 * 60_000 });
  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(
      `P3 filter harness returned invalid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
}

function requireSuccess(command, args, options = {}) {
  const result = runProcess(command, args, options);
  if (result.error !== null || result.status !== 0 || result.signal !== null)
    throw new Error(
      result.error ||
        result.stderr.trim() ||
        result.stdout.trim() ||
        `${command} exited with status ${String(result.status)}`,
    );
  return result;
}

function runProcess(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repository,
    encoding: "utf8",
    env: { ...process.env, ...(options.env ?? {}) },
    maxBuffer: 16 * 1024 * 1024,
    timeout: options.timeout ?? 30_000,
  });
  return {
    status: result.status,
    signal: result.signal,
    error: result.error?.message ?? null,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
  };
}

async function assertOutputAbsent(destination) {
  try {
    await lstat(destination);
    throw new Error(
      "P3 filter evidence already exists and will not be overwritten",
    );
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
}

async function writeExclusive(destination, value) {
  await mkdir(path.dirname(destination), { recursive: true });
  const bytes = Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
  const temporary = path.join(
    path.dirname(destination),
    `.${path.basename(destination)}.${randomUUID()}.tmp`,
  );
  await writeFile(temporary, bytes, { flag: "wx" });
  try {
    await link(temporary, destination);
  } finally {
    await unlink(temporary).catch(() => {});
  }
}
