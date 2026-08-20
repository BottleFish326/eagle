import path from "node:path";

import { inspectFormatFixtureManifest } from "./format-fixture-manifest.mjs";

const defaultManifest = path.resolve(
  import.meta.dirname,
  "..",
  "fixtures",
  "formats",
  "manifest.json",
);

try {
  const manifestPath = parseArguments(process.argv.slice(2));
  const report = await inspectFormatFixtureManifest({ manifestPath });
  console.log(JSON.stringify(report, null, 2));
  if (!report.accepted) process.exitCode = 1;
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

function parseArguments(args) {
  if (args.length === 0) return defaultManifest;
  if (args.length === 2 && args[0] === "--manifest")
    return path.resolve(args[1]);
  throw new Error(
    "usage: node tools/verify-format-fixtures.mjs [--manifest <manifest.json>]",
  );
}
