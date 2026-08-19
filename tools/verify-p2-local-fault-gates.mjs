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
  buildP2LocalFaultGatesReport,
  P2_CACHE_FAULT_POINTS,
  P2_TRANSACTION_ABORT_AFTER,
  P2_TRANSACTION_COUNT,
} from "./p2-local-fault-gates.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const outputPath = path.join(
  repository,
  "docs",
  "reports",
  "evidence",
  "p2-local-fault-gates.json",
);
let temporaryRoot = null;
let temporaryWorkspacesRemoved = false;

try {
  assertNode24();
  assertNoArguments(process.argv.slice(2));
  await assertOutputAbsentOrFail(outputPath);
  const gitCommit = readRepositoryState();
  requireSuccess(
    "cargo",
    [
      "build",
      "--locked",
      "--release",
      "-p",
      "transaction-fault",
      "-p",
      "cache-fault",
    ],
    { cwd: repository, timeout: 10 * 60_000 },
  );

  const transactionBinary = binaryPath("transaction-fault");
  const cacheBinary = binaryPath("cache-fault");
  const binarySha256 = {
    transactionFault: await sha256File(transactionBinary),
    cacheFault: await sha256File(cacheBinary),
  };
  temporaryRoot = await mkdtemp(
    path.join(tmpdir(), "material-eagle-p2-local-faults-"),
  );

  const transactionWorkspace = path.join(temporaryRoot, "transaction");
  const transaction = {
    abort: runProcess(
      transactionBinary,
      [
        "execute",
        transactionWorkspace,
        "--count",
        String(P2_TRANSACTION_COUNT),
      ],
      {
        cwd: temporaryRoot,
        env: {
          EAGLE_TRANSACTION_ABORT_AFTER_APPLIED: String(
            P2_TRANSACTION_ABORT_AFTER,
          ),
        },
        timeout: 2 * 60_000,
      },
    ),
    recover: runProcess(
      transactionBinary,
      [
        "recover",
        transactionWorkspace,
        "--count",
        String(P2_TRANSACTION_COUNT),
      ],
      { cwd: temporaryRoot, timeout: 2 * 60_000 },
    ),
  };

  const cacheCases = [];
  for (const faultPoint of P2_CACHE_FAULT_POINTS) {
    const workspace = path.join(temporaryRoot, faultPoint);
    cacheCases.push({
      faultPoint,
      seed: runProcess(cacheBinary, ["seed", workspace], {
        cwd: temporaryRoot,
        timeout: 60_000,
      }),
      abort: runProcess(cacheBinary, ["clear", workspace], {
        cwd: temporaryRoot,
        env: { EAGLE_CACHE_FAULT_POINT: faultPoint },
        timeout: 60_000,
      }),
      recover: runProcess(cacheBinary, ["recover", workspace], {
        cwd: temporaryRoot,
        timeout: 60_000,
      }),
    });
  }

  const reportInput = {
    gitCommit,
    executedAt: new Date().toISOString(),
    environment: {
      platform: process.platform,
      architecture: process.arch,
      nodeVersion: process.version,
      rustc: requireSuccess("rustc", ["--version"], {
        cwd: repository,
      }).stdout.trim(),
      cargo: requireSuccess("cargo", ["--version"], {
        cwd: repository,
      }).stdout.trim(),
    },
    binarySha256,
    repositoryClean: true,
    transaction,
    cacheCases,
    temporaryWorkspacesRemoved: true,
  };
  const report = buildP2LocalFaultGatesReport(reportInput);
  if (!report.accepted)
    throw new Error(
      `P2 local fault gates rejected: ${report.failures.join("; ")}`,
    );

  await rm(temporaryRoot, { recursive: true });
  temporaryWorkspacesRemoved = true;
  await writeExclusive(outputPath, report);
  console.log(JSON.stringify(report, null, 2));
} catch (error) {
  const preserved =
    temporaryRoot !== null && !temporaryWorkspacesRemoved
      ? `; temporary workspaces preserved at ${temporaryRoot}`
      : "";
  console.error(
    `${error instanceof Error ? error.message : String(error)}${preserved}`,
  );
  process.exitCode = 1;
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `P2 local fault gates require Node.js 24.x, received ${process.version}`,
    );
}

function assertNoArguments(args) {
  if (args.length !== 0)
    throw new Error("usage: node tools/verify-p2-local-fault-gates.mjs");
}

function readRepositoryState() {
  const revision = requireSuccess("git", ["rev-parse", "HEAD"], {
    cwd: repository,
  }).stdout.trim();
  const status = requireSuccess(
    "git",
    ["status", "--porcelain=v1", "--untracked-files=all"],
    { cwd: repository },
  ).stdout;
  if (status !== "")
    throw new Error(
      "P2 local fault gates require a completely clean working tree",
    );
  return revision;
}

function binaryPath(name) {
  return path.join(
    repository,
    "target",
    "release",
    process.platform === "win32" ? `${name}.exe` : name,
  );
}

async function sha256File(filePath) {
  const stats = await lstat(filePath);
  if (!stats.isFile())
    throw new Error(`fault harness is not a regular file: ${filePath}`);
  return createHash("sha256")
    .update(await readFile(filePath))
    .digest("hex");
}

function requireSuccess(command, args, options) {
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
    cwd: options.cwd ?? repository,
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

async function assertOutputAbsentOrFail(destination) {
  try {
    await lstat(destination);
    throw new Error(
      "P2 local fault evidence already exists and will not be overwritten",
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
