import { rename, rm, writeFile } from "node:fs/promises";

export function createResourceStabilityCheckpoint({
  startedAt,
  gitCommit,
  environment,
  options,
  childPid,
  internalSamples,
  externalSamples,
  sampleParseErrors,
  monitorErrors,
  stderr,
}) {
  return {
    schema: 1,
    status: "running",
    gitCommit,
    environment,
    startedAt: startedAt.toISOString(),
    updatedAt: new Date().toISOString(),
    options: {
      durationSeconds: options.durationSeconds,
      warmupSeconds: options.warmupSeconds,
      fixtureCount: options.fixtureCount,
      sampleIntervalSeconds: options.sampleIntervalSeconds,
      checkpointIntervalSeconds: options.checkpointIntervalSeconds,
    },
    childPid,
    internalSamples,
    externalSamples,
    sampleParseErrors,
    monitorErrors,
    stderr: stderr.trim(),
  };
}

export async function writeJsonAtomic(output, value) {
  const temporary = `${output}.tmp-${String(process.pid)}-${String(Date.now())}`;
  try {
    await writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`, {
      flag: "wx",
    });
    await rename(temporary, output);
  } finally {
    await rm(temporary, { force: true });
  }
}
