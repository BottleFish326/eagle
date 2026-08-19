import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, readdir, rm, stat } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const parent = await mkdtemp(path.join(os.tmpdir(), "material-eagle-ci-"));
const library = path.join(parent, "library");
const marker = path.join(library, ".eagle-fixture-manifest.json");

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
    "small",
  ]);
  const manifest = JSON.parse(await readFile(marker, "utf8"));
  assert(
    manifest.count === 1_000,
    `fixture manifest count is ${manifest.count}`,
  );
  assert(
    manifest.sidecarCount === 200,
    `fixture sidecar count is ${manifest.sidecarCount}`,
  );

  const files = await walkFiles(library);
  const sample = files.find((file) => file.endsWith(".png"));
  assert(sample !== undefined, "fixture contains no PNG sample");
  const sampleDigestBefore = await sha256(sample);

  const scan = runCargo([
    "run",
    "--quiet",
    "--release",
    "-p",
    "eagle-p0",
    "--",
    "scan",
    library,
    "--json",
  ]);
  const report = JSON.parse(scan);
  assert(
    report.assets.length === manifest.count,
    `scan returned ${report.assets.length} assets`,
  );
  assert(
    report.problems.length === 0,
    `scan reported ${report.problems.length} problems`,
  );
  const assetsWithDimensions = report.assets.filter(
    (asset) => asset.dimensions !== null,
  ).length;
  const damagedImages = report.assets.filter((asset) =>
    asset.issues.some((issue) => issue.type === "invalid-image-metadata"),
  ).length;
  assert(
    assetsWithDimensions === manifest.count - 1,
    `expected ${manifest.count - 1} dimension records, got ${assetsWithDimensions}`,
  );
  assert(
    damagedImages === 1,
    `expected one isolated damaged image, got ${damagedImages}`,
  );
  assert(
    report.assets.every(
      (asset) =>
        typeof asset.relativePath === "string" &&
        typeof asset.size === "number" &&
        typeof asset.modifiedUnixMs === "number",
    ),
    "formal AssetRecord fields are incomplete",
  );
  assert(
    (await sha256(sample)) === sampleDigestBefore,
    "scan modified an original asset",
  );
  assert(
    files.every((file) => !/\.(?:db|sqlite|sqlite3)$/iu.test(file)),
    "fixture contains a forbidden database file",
  );

  console.log(
    `S dataset accepted: assets=${report.assets.length} sidecars=${manifest.sidecarCount} dimensions=${assetsWithDimensions} isolatedDamagedImages=${damagedImages} problems=0 scanMs=${report.elapsedMs} assetDigestUnchanged=true`,
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
    // The parent directory was created by mkdtemp in this process and is safe to remove below.
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

async function walkFiles(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await walkFiles(entryPath)));
    } else if (entry.isFile()) {
      files.push(entryPath);
    }
  }
  return files;
}

async function sha256(filePath) {
  return createHash("sha256")
    .update(await readFile(filePath))
    .digest("hex");
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}
