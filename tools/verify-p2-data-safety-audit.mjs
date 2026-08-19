import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import {
  link,
  lstat,
  mkdir,
  readFile,
  unlink,
  writeFile,
} from "node:fs/promises";
import path from "node:path";

import {
  buildP2DataSafetyAuditReport,
  inspectP2DataSafetyAuditReceipt,
  P2_DATA_SAFETY_REPORTS,
} from "./p2-data-safety-audit.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const files = {
  defectRegister: "docs/defects.json",
  external: "docs/reports/evidence/p2-external-gates.json",
  localFaults: "docs/reports/evidence/p2-local-fault-gates.json",
  output: "docs/reports/evidence/p2-data-safety-audit.json",
};

try {
  assertNode24();
  assertNoArguments(process.argv.slice(2));
  const candidateCommit = git(["rev-parse", "HEAD"]).trim();
  const candidateCommittedAt = new Date(
    git(["show", "-s", "--format=%cI", candidateCommit]).trim(),
  ).toISOString();
  const repositoryClean = cleanExceptExistingOutput(
    git(["status", "--porcelain=v1", "--untracked-files=all"]),
  );
  const defectRegisterBytes = await readBoundedFile(
    files.defectRegister,
    1024 * 1024,
  );
  const externalBytes = await readBoundedFile(files.external, 1024 * 1024);
  const localFaultBytes = await readBoundedFile(files.localFaults, 1024 * 1024);
  const defectRegister = JSON.parse(defectRegisterBytes.toString("utf8"));
  const externalReceipt = JSON.parse(externalBytes.toString("utf8"));
  const localFaultReceipt = JSON.parse(localFaultBytes.toString("utf8"));
  const reportFiles = await Promise.all(
    P2_DATA_SAFETY_REPORTS.map(async (fileName) => ({
      fileName,
      bytes: await readBoundedFile(fileName, 4 * 1024 * 1024),
    })),
  );
  const inputFiles = [
    { fileName: files.defectRegister, bytes: defectRegisterBytes },
    { fileName: files.external, bytes: externalBytes },
    { fileName: files.localFaults, bytes: localFaultBytes },
    ...reportFiles,
  ];
  const inputsCommitted = inputFiles.every(({ fileName, bytes }) =>
    gitBlobEquals(candidateCommit, fileName, bytes),
  );
  const commitOrderVerified =
    isCommit(externalReceipt?.p2A11?.gitCommit) &&
    isCommit(externalReceipt?.p2A12?.gitCommit) &&
    isCommit(localFaultReceipt?.gitCommit) &&
    isAncestor(
      externalReceipt.p2A11.gitCommit,
      externalReceipt.p2A12.gitCommit,
    ) &&
    isAncestor(externalReceipt.p2A12.gitCommit, localFaultReceipt.gitCommit) &&
    isAncestor(localFaultReceipt.gitCommit, candidateCommit);
  const report = buildP2DataSafetyAuditReport({
    candidateCommit,
    candidateCommittedAt,
    repositoryClean,
    commitOrderVerified,
    inputsCommitted,
    defectRegisterBytes,
    defectRegister,
    externalBytes,
    externalReceipt,
    localFaultBytes,
    localFaultReceipt,
    reportFiles,
  });
  console.log(JSON.stringify(report, null, 2));
  if (!report.accepted) {
    process.exitCode = 1;
  } else {
    const inspection = inspectP2DataSafetyAuditReceipt(report);
    if (!inspection.accepted)
      throw new Error(
        `data safety receipt self-check failed: ${inspection.failures.join("; ")}`,
      );
    await writeExclusiveOrIdentical(files.output, report);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `P2 data safety audit requires Node.js 24.x, received ${process.version}`,
    );
}

function assertNoArguments(args) {
  if (args.length !== 0)
    throw new Error("usage: node tools/verify-p2-data-safety-audit.mjs");
}

function cleanExceptExistingOutput(status) {
  const entries = status.split("\n").filter(Boolean);
  return (
    entries.length === 0 ||
    (entries.length === 1 && entries[0] === `?? ${files.output}`)
  );
}

async function readBoundedFile(fileName, maximumBytes) {
  const filePath = path.join(repository, fileName);
  const stats = await lstat(filePath);
  if (!stats.isFile()) throw new Error(`${fileName} is not a regular file`);
  if (stats.size > maximumBytes)
    throw new Error(`${fileName} exceeds its size limit`);
  return readFile(filePath);
}

function git(args) {
  const result = spawnSync("git", args, {
    cwd: repository,
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  });
  if (result.status !== 0)
    throw new Error(
      `git ${args.join(" ")} failed: ${(result.stderr || result.stdout).trim()}`,
    );
  return result.stdout;
}

function gitBlobEquals(commit, fileName, expectedBytes) {
  const result = spawnSync("git", ["show", `${commit}:${fileName}`], {
    cwd: repository,
    encoding: null,
    maxBuffer: 16 * 1024 * 1024,
  });
  return (
    result.status === 0 && Buffer.from(result.stdout).equals(expectedBytes)
  );
}

function isAncestor(ancestor, descendant) {
  return (
    spawnSync("git", ["merge-base", "--is-ancestor", ancestor, descendant], {
      cwd: repository,
      stdio: "ignore",
    }).status === 0
  );
}

function isCommit(value) {
  return typeof value === "string" && /^[0-9a-f]{40,64}$/u.test(value);
}

async function writeExclusiveOrIdentical(fileName, value) {
  const destination = path.join(repository, fileName);
  const bytes = Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
  try {
    const stats = await lstat(destination);
    if (!stats.isFile())
      throw new Error(
        "existing P2 data safety audit evidence is not a regular file",
      );
    if (stats.size > 1024 * 1024)
      throw new Error("existing P2 data safety audit evidence exceeds 1 MiB");
    const existing = await readFile(destination);
    if (!existing.equals(bytes))
      throw new Error("existing P2 data safety audit evidence differs");
    return;
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
  await mkdir(path.dirname(destination), { recursive: true });
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
