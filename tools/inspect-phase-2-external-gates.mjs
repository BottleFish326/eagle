import { lstat, readFile } from "node:fs/promises";
import path from "node:path";

import { inspectPhase2ExternalGatesReceipt } from "./phase-2-external-gates.mjs";

const repository = path.resolve(import.meta.dirname, "..");

try {
  assertNode24();
  const evidencePath = parseArguments(process.argv.slice(2));
  const stats = await lstat(evidencePath);
  if (!stats.isFile())
    throw new Error("phase 2 external gate evidence is not a regular file");
  if (stats.size > 1024 * 1024)
    throw new Error("phase 2 external gate evidence exceeds 1 MiB");
  const value = JSON.parse(await readFile(evidencePath, "utf8"));
  const inspection = inspectPhase2ExternalGatesReceipt(value);
  console.log(JSON.stringify(inspection, null, 2));
  if (!inspection.accepted) process.exitCode = 1;
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

function parseArguments(args) {
  if (args.length > 1)
    throw new Error(
      "usage: node tools/inspect-phase-2-external-gates.mjs [receipt.json]",
    );
  return path.resolve(
    args[0] ??
      path.join(
        repository,
        "docs",
        "reports",
        "evidence",
        "p2-external-gates.json",
      ),
  );
}

function assertNode24() {
  if (Number.parseInt(process.versions.node.split(".", 1)[0], 10) !== 24)
    throw new Error(
      `phase 2 external gate inspection requires Node.js 24.x, received ${process.version}`,
    );
}
