import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, stat } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const parent = await mkdtemp(path.join(os.tmpdir(), "material-eagle-medium-"));
const library = path.join(parent, "library");
const marker = path.join(library, ".eagle-fixture-manifest.json");
const sample = path.join(library, "group-000", "asset-000001.png");

try {
  runCargo([
    "run",
    "--quiet",
    "--release",
    "-p",
    "fixture-generator",
    "--",
    "generate",
    library,
    "--scale",
    "medium",
  ]);
  const manifest = JSON.parse(await readFile(marker, "utf8"));
  assert(
    manifest.count === 10_000,
    `fixture manifest count is ${manifest.count}`,
  );
  assert(
    manifest.sidecarCount === 2_000,
    `fixture sidecar count is ${manifest.sidecarCount}`,
  );
  const sampleDigestBefore = await sha256(sample);
  const benchmark = parseProperties(
    runCargo([
      "run",
      "--quiet",
      "--release",
      "-p",
      "eagle-p0",
      "--",
      "benchmark",
      library,
      "--iterations",
      "200",
    ]),
  );
  assert(
    benchmark.assets === 10_000,
    `benchmark assets is ${benchmark.assets}`,
  );
  assert(
    benchmark.scan_ms <= 60_000,
    `M scan ${benchmark.scan_ms} ms exceeds 60,000 ms`,
  );
  assert(
    benchmark.query_p95_us <= 100_000,
    `M query p95 ${benchmark.query_p95_us} us exceeds 100,000 us`,
  );
  assert(benchmark.query_matches > 0, "M compound query returned no records");
  assert(
    (await sha256(sample)) === sampleDigestBefore,
    "M benchmark modified an original asset",
  );

  console.log(
    [
      "M dataset accepted",
      `assets=${benchmark.assets}`,
      `sidecars=${manifest.sidecarCount}`,
      `scanMs=${benchmark.scan_ms}`,
      `queryIterations=${benchmark.query_iterations}`,
      `queryMatches=${benchmark.query_matches}`,
      `queryP50Us=${benchmark.query_p50_us}`,
      `queryP95Us=${benchmark.query_p95_us}`,
      "assetDigestUnchanged=true",
    ].join(" "),
  );
} finally {
  try {
    await stat(marker);
    runCargo([
      "run",
      "--quiet",
      "--release",
      "-p",
      "fixture-generator",
      "--",
      "clean",
      library,
    ]);
  } catch {
    // The parent was created by mkdtemp in this process and is safe to remove.
  }
  await rm(parent, { recursive: true, force: true });
}

function runCargo(argumentsList) {
  const result = spawnSync("cargo", argumentsList, {
    cwd: path.resolve(import.meta.dirname, ".."),
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error !== undefined) {
    throw new Error(`failed to launch cargo: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(
      result.stderr ||
        result.stdout ||
        `cargo exited with status ${result.status}`,
    );
  }
  return result.stdout;
}

function parseProperties(output) {
  return Object.fromEntries(
    output
      .trim()
      .split("\n")
      .map((line) => line.split(":", 2))
      .map(([key, value]) => [key, Number(value.trim())]),
  );
}

async function sha256(filePath) {
  return createHash("sha256")
    .update(await readFile(filePath))
    .digest("hex");
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}
