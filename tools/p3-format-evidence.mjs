import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { lstat, readFile, readdir, realpath } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { analyzeFormatResourceRuns } from "./format-resource-gate.mjs";
import { writeJsonAtomic } from "./resource-stability-checkpoint.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const EXPECTED_FIXTURES = 43;
const EXPECTED_ADVERSARIAL_FIXTURES = 31;
const EXPECTED_SOURCE_BYTES = 1_468_344;
const EXPECTED_RSS_LIMIT_KIB = 512 * 1024;
const MAX_REPORT_BYTES = 2 * 1024 * 1024;
const artifactPattern =
  /^(p3-a01|p3-a02)-(core-only|bundled-codecs)-(Linux|macOS|Windows)(?:-(X64|ARM64))?-([0-9a-f]{40})-attempt-([1-9][0-9]*)$/u;

const platformNames = {
  Linux: { gate: "linux", node: "linux" },
  macOS: { gate: "macos", node: "darwin" },
  Windows: { gate: "windows", node: "win32" },
};

const architectureNames = {
  ARM64: "arm64",
  X64: "x64",
};

export function analyzeP3FormatEvidence({ sources, context }) {
  const failures = [];
  const byKey = new Map();
  if (!/^v24\./u.test(context.nodeVersion)) {
    failures.push(
      `consolidation requires Node.js 24.x, received ${context.nodeVersion}`,
    );
  }
  if (!/^[0-9a-f]{40}$/u.test(context.gitCommit)) {
    failures.push("Git commit must be a lowercase 40-character SHA-1");
  }
  if (!/^[1-9][0-9]*$/u.test(context.runId)) failures.push("run ID is invalid");
  if (!/^[1-9][0-9]*$/u.test(context.runAttempt)) {
    failures.push("run attempt is invalid");
  }

  for (const source of sources) {
    const match = artifactPattern.exec(source.artifactName);
    if (match === null) {
      failures.push(`unexpected artifact name: ${source.artifactName}`);
      continue;
    }
    const [, gate, profile, platform, architecture, commit, attempt] = match;
    if (commit !== context.gitCommit) {
      failures.push(`${source.artifactName} is bound to a different commit`);
    }
    if (attempt !== context.runAttempt) {
      failures.push(
        `${source.artifactName} is bound to a different run attempt`,
      );
    }
    if (profile === "core-only" && architecture !== undefined) {
      failures.push(
        `${source.artifactName} unexpectedly includes an architecture`,
      );
    }
    if (profile === "bundled-codecs" && architecture === undefined) {
      failures.push(
        `${source.artifactName} is missing its worker architecture`,
      );
    }
    const expectedFile =
      gate === "p3-a01"
        ? `p3-a01-${profile}.json`
        : `p3-a02-${profile}-resources.json`;
    if (source.fileName !== expectedFile) {
      failures.push(
        `${source.artifactName} contains unexpected file ${source.fileName}`,
      );
    }
    if (!/^[0-9a-f]{64}$/u.test(source.sha256)) {
      failures.push(`${source.artifactName} has an invalid SHA-256`);
    }
    const key = `${gate}:${profile}:${platform}`;
    if (byKey.has(key)) {
      failures.push(`duplicate evidence for ${key}`);
      continue;
    }
    byKey.set(key, { ...source, gate, profile, platform, architecture });
  }

  for (const gate of ["p3-a01", "p3-a02"]) {
    for (const profile of ["core-only", "bundled-codecs"]) {
      for (const platform of Object.keys(platformNames)) {
        const key = `${gate}:${profile}:${platform}`;
        if (!byKey.has(key)) failures.push(`missing evidence for ${key}`);
      }
    }
  }
  if (sources.length !== 12) {
    failures.push(
      `expected exactly 12 evidence artifacts, received ${sources.length}`,
    );
  }

  const manifestDigests = new Set();
  for (const source of byKey.values()) {
    const label = `${source.gate}:${source.profile}:${source.platform}`;
    if (source.gate === "p3-a01") {
      validateFormatReport(source, label, failures, manifestDigests);
    } else {
      validateResourceReport(source, label, failures, manifestDigests, context);
    }
  }
  if (manifestDigests.size !== 1) {
    failures.push(
      "all format and resource reports must share one manifest SHA-256",
    );
  }

  const profiles = ["core-only", "bundled-codecs"].map((profile) => ({
    profile,
    platforms: Object.keys(platformNames).map((platform) => {
      const format = byKey.get(`p3-a01:${profile}:${platform}`);
      const resources = byKey.get(`p3-a02:${profile}:${platform}`);
      return {
        platform,
        architecture: resources?.architecture ?? null,
        formatEvidenceSha256: format?.sha256 ?? null,
        resourceEvidenceSha256: resources?.sha256 ?? null,
        maxRssKiB: resources?.report.processTree?.maxRssKiB ?? null,
        rssSampleCount: resources?.report.processTree?.sampleCount ?? null,
      };
    }),
  }));

  return {
    schema: 1,
    accepted: failures.length === 0,
    gitCommit: context.gitCommit,
    workflow: {
      runId: context.runId,
      runAttempt: context.runAttempt,
      workflowRef: context.workflowRef,
      repository: context.repository,
      runUrl: `${context.serverUrl}/${context.repository}/actions/runs/${context.runId}`,
    },
    manifestSha256: [...manifestDigests][0] ?? null,
    fixtureCount: EXPECTED_FIXTURES,
    adversarialFixtureCount: EXPECTED_ADVERSARIAL_FIXTURES,
    profiles,
    sources: [...sources]
      .sort((left, right) =>
        left.artifactName.localeCompare(right.artifactName),
      )
      .map(({ artifactName, fileName, sha256 }) => ({
        artifactName,
        fileName,
        sha256,
      })),
    failures,
  };
}

function validateFormatReport(source, label, failures, manifestDigests) {
  const report = source.report;
  if (report.schema !== 1) failures.push(`${label} has an unsupported schema`);
  if (report.accepted !== true || report.failures?.length !== 0) {
    failures.push(`${label} was not accepted without failures`);
  }
  if (report.platform !== platformNames[source.platform].gate) {
    failures.push(`${label} reports the wrong platform`);
  }
  if (report.providerProfile !== source.profile) {
    failures.push(`${label} reports the wrong provider profile`);
  }
  if (
    report.fixtureCount !== EXPECTED_FIXTURES ||
    report.checkedFixtureCount !== EXPECTED_FIXTURES
  ) {
    failures.push(`${label} did not replay all ${EXPECTED_FIXTURES} fixtures`);
  }
  if (report.adversarialFixtureCount !== EXPECTED_ADVERSARIAL_FIXTURES) {
    failures.push(`${label} has the wrong adversarial fixture count`);
  }
  if (report.sourceBytes !== EXPECTED_SOURCE_BYTES) {
    failures.push(`${label} has the wrong source byte count`);
  }
  if (report.sourceDigestUnchanged !== true) {
    failures.push(`${label} did not preserve fixture source bytes`);
  }
  if (report.cancellation?.accepted !== true) {
    failures.push(`${label} did not prove cooperative cancellation`);
  }
  recordManifestDigest(report.manifestSha256, label, failures, manifestDigests);
}

function validateResourceReport(
  source,
  label,
  failures,
  manifestDigests,
  context,
) {
  const report = source.report;
  if (report.gitCommit !== context.gitCommit) {
    failures.push(`${label} reports the wrong Git commit`);
  }
  if (report.environment?.platform !== platformNames[source.platform].node) {
    failures.push(`${label} reports the wrong runtime platform`);
  }
  if (!/^v24\./u.test(report.environment?.nodeVersion ?? "")) {
    failures.push(`${label} did not run with Node.js 24.x`);
  }
  if (
    source.architecture !== undefined &&
    report.environment?.architecture !== architectureNames[source.architecture]
  ) {
    failures.push(`${label} reports the wrong worker architecture`);
  }
  const replay = analyzeFormatResourceRuns({
    reports: Array.isArray(report.runs) ? report.runs : [],
    rssSamples: Array.isArray(report.processTree?.samples)
      ? report.processTree.samples
      : [],
    maxRssKiB: EXPECTED_RSS_LIMIT_KIB,
    repositoryState: { gitCommit: context.gitCommit, dirty: false },
    providerProfile: source.profile,
    environment: report.environment,
  });
  if (!isDeepStrictEqual(replay, report)) {
    failures.push(`${label} does not equal an independent raw-sample replay`);
  }
  if (report.iterations !== 3)
    failures.push(`${label} did not run three iterations`);
  recordManifestDigest(report.manifestSha256, label, failures, manifestDigests);
}

function recordManifestDigest(digest, label, failures, manifestDigests) {
  if (!/^[0-9a-f]{64}$/u.test(digest ?? "")) {
    failures.push(`${label} has an invalid manifest SHA-256`);
  } else {
    manifestDigests.add(digest);
  }
}

async function collectSources(inputDirectory) {
  const root = await realpath(inputDirectory);
  const entries = await readdir(root, { withFileTypes: true });
  const sources = [];
  for (const entry of entries) {
    const artifactPath = path.join(root, entry.name);
    const artifactStat = await lstat(artifactPath);
    if (!entry.isDirectory() || artifactStat.isSymbolicLink()) {
      throw new Error(
        `evidence entry is not a regular directory: ${entry.name}`,
      );
    }
    const files = await readdir(artifactPath, { withFileTypes: true });
    if (files.length !== 1 || !files[0].isFile()) {
      throw new Error(
        `evidence artifact must contain exactly one regular file: ${entry.name}`,
      );
    }
    const filePath = path.join(artifactPath, files[0].name);
    const fileStat = await lstat(filePath);
    if (fileStat.isSymbolicLink() || !fileStat.isFile()) {
      throw new Error(`evidence report is not a regular file: ${entry.name}`);
    }
    if (fileStat.size > MAX_REPORT_BYTES) {
      throw new Error(`evidence report exceeds 2 MiB: ${entry.name}`);
    }
    const canonicalFile = await realpath(filePath);
    if (!canonicalFile.startsWith(`${root}${path.sep}`)) {
      throw new Error(
        `evidence report escaped the input directory: ${entry.name}`,
      );
    }
    const bytes = await readFile(canonicalFile);
    sources.push({
      artifactName: entry.name,
      fileName: files[0].name,
      sha256: createHash("sha256").update(bytes).digest("hex"),
      report: JSON.parse(bytes.toString("utf8")),
    });
  }
  return sources;
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  const sources = await collectSources(options.inputDirectory);
  const report = analyzeP3FormatEvidence({
    sources,
    context: {
      gitCommit: process.env.GITHUB_SHA ?? "",
      runId: process.env.GITHUB_RUN_ID ?? "",
      runAttempt: process.env.GITHUB_RUN_ATTEMPT ?? "",
      workflowRef: process.env.GITHUB_WORKFLOW_REF ?? "",
      repository: process.env.GITHUB_REPOSITORY ?? "",
      serverUrl: process.env.GITHUB_SERVER_URL ?? "https://github.com",
      nodeVersion: process.version,
    },
  });
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  if (!report.accepted) {
    process.exitCode = 1;
    return;
  }
  await writeJsonAtomic(options.output, report);
}

function parseArguments(arguments_) {
  let inputDirectory;
  let output;
  for (let index = 0; index < arguments_.length; index += 2) {
    const argument = arguments_[index];
    const value = arguments_[index + 1];
    if (value === undefined) throw new Error(`${argument} requires a value`);
    if (argument === "--input-directory") inputDirectory = path.resolve(value);
    else if (argument === "--output") output = path.resolve(value);
    else throw new Error(`unknown argument: ${argument}`);
  }
  if (inputDirectory === undefined || output === undefined) {
    throw new Error("--input-directory and --output are required");
  }
  return { inputDirectory, output };
}

if (path.resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  await main();
}
