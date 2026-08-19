import path from "node:path";

import { collectP2HostedReadinessInputs } from "./p2-hosted-environment.mjs";
import { buildP2HostedReadiness } from "./p2-hosted-readiness.mjs";

const repository = path.resolve(import.meta.dirname, "..");

try {
  assertNode24();
  assertNoArguments(process.argv.slice(2));
  const report = buildP2HostedReadiness(
    await collectP2HostedReadinessInputs(repository),
  );
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
