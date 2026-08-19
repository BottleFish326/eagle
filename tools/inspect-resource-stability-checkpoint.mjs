import { readFile } from "node:fs/promises";
import path from "node:path";

import { inspectResourceStabilityCheckpoint } from "./resource-stability-checkpoint.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const checkpointPath = path.resolve(
  process.argv[2] ??
    path.join(
      repository,
      "docs",
      "reports",
      "evidence",
      "p2-06-resource-soak.json.partial",
    ),
);
const checkpoint = JSON.parse(await readFile(checkpointPath, "utf8"));
const inspection = inspectResourceStabilityCheckpoint(checkpoint);
console.log(JSON.stringify(inspection, null, 2));
if (!inspection.healthy) process.exitCode = 1;
