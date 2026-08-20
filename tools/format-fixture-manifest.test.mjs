import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import { inspectFormatFixtureManifest } from "./format-fixture-manifest.mjs";

const repository = path.resolve(import.meta.dirname, "..");
const trackedManifest = path.join(
  repository,
  "fixtures",
  "formats",
  "manifest.json",
);

test("accepts the tracked format fixture manifest with real integrity metadata", async () => {
  const report = await inspectFormatFixtureManifest({
    manifestPath: trackedManifest,
  });
  assert.equal(report.accepted, true, report.failures.join("; "));
  assert.equal(report.fixtureCount, 32);
  assert.equal(report.sourceBytes, 1_463_323);
  assert.equal(report.referenceBytes, 24_623);
});

test("rejects placeholder hashes, digest changes, and incomplete provider coverage", async (context) => {
  const workspace = await makeWorkspace();
  context.after(() => rm(workspace.root, { recursive: true, force: true }));
  const manifest = makeManifest(workspace.source);
  manifest.fixtures[0].sha256 = "0".repeat(64);
  manifest.fixtures[0].expectations[0].platforms = ["windows", "macos"];
  await writeManifest(workspace.manifestPath, manifest);

  const report = await inspectFormatFixtureManifest({
    manifestPath: workspace.manifestPath,
  });
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) => failure.includes("placeholder SHA-256")),
  );
  assert.ok(
    report.failures.some((failure) => failure.includes("SHA-256 mismatch")),
  );
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("missing linux coverage"),
    ),
  );
});

test("rejects duplicate fixture/profile/platform expectations across entries", async (context) => {
  const workspace = await makeWorkspace();
  context.after(() => rm(workspace.root, { recursive: true, force: true }));
  const manifest = makeManifest(workspace.source);
  const expected = manifest.fixtures[0].expectations[0].result;
  manifest.fixtures[0].expectations = [
    {
      platforms: ["windows", "macos"],
      providerProfile: "core-only",
      result: expected,
    },
    {
      platforms: ["macos", "linux"],
      providerProfile: "core-only",
      result: expected,
    },
  ];
  await writeManifest(workspace.manifestPath, manifest);

  const report = await inspectFormatFixtureManifest({
    manifestPath: workspace.manifestPath,
  });
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("duplicate expectation for core-only/macos"),
    ),
  );
});

test("checks reference PNG integrity and IHDR dimensions", async (context) => {
  const workspace = await makeWorkspace();
  context.after(() => rm(workspace.root, { recursive: true, force: true }));
  const png = Buffer.from(
    "89504e470d0a1a0a0000000d4948445200000001000000010804000000b51c0c020000000b4944415478da6364f80f0000010501012718e3660000000049454e44ae426082",
    "hex",
  );
  const referencePath = path.join(workspace.root, "references", "one.png");
  await mkdir(path.dirname(referencePath), { recursive: true });
  await writeFile(referencePath, png);
  const manifest = makeManifest(workspace.source);
  manifest.fixtures[0].expectations[0].result.preview = {
    status: "available",
    referencePath: "references/one.png",
    referenceSha256: sha256(png),
    width: 2,
    height: 1,
  };
  await writeManifest(workspace.manifestPath, manifest);

  const report = await inspectFormatFixtureManifest({
    manifestPath: workspace.manifestPath,
  });
  assert.equal(report.accepted, false);
  assert.ok(
    report.failures.some((failure) =>
      failure.includes("dimensions mismatch: expected 2x1, received 1x1"),
    ),
  );
});

test(
  "rejects a fixture path containing a symbolic link",
  { skip: process.platform === "win32" },
  async (context) => {
    const workspace = await makeWorkspace();
    context.after(() => rm(workspace.root, { recursive: true, force: true }));
    const linkedPath = path.join(workspace.root, "linked.svg");
    await symlink(workspace.sourcePath, linkedPath);
    const manifest = makeManifest(workspace.source);
    manifest.fixtures[0].path = "linked.svg";
    await writeManifest(workspace.manifestPath, manifest);

    const report = await inspectFormatFixtureManifest({
      manifestPath: workspace.manifestPath,
    });
    assert.equal(report.accepted, false);
    assert.ok(
      report.failures.some((failure) => failure.includes("symbolic link")),
    );
  },
);

async function makeWorkspace() {
  const root = await mkdtemp(path.join(os.tmpdir(), "material-eagle-formats-"));
  const sourcePath = path.join(root, "sources", "minimal.svg");
  await mkdir(path.dirname(sourcePath), { recursive: true });
  const source = Buffer.from(
    '<svg xmlns="http://www.w3.org/2000/svg"></svg>\n',
  );
  await writeFile(sourcePath, source);
  return {
    root,
    sourcePath,
    source,
    manifestPath: path.join(root, "manifest.json"),
  };
}

function makeManifest(source) {
  return {
    schema: 1,
    suite: { id: "test-formats", version: 1 },
    fixtures: [
      {
        id: "svg-minimal",
        path: "sources/minimal.svg",
        format: "svg",
        case: "normal",
        source: { origin: "generated-in-repository", license: "MIT" },
        sha256: sha256(source),
        size: source.length,
        expectations: [
          {
            platforms: ["windows", "macos", "linux"],
            providerProfile: "core-only",
            result: {
              recognized: true,
              mime: "image/svg+xml",
              kind: "image",
              issueCodes: [],
              metadata: {
                status: "unsupported-feature",
                reasonCode: "metadata-provider-unavailable",
                properties: {},
              },
              preview: {
                status: "codec-unavailable",
                reasonCode: "preview-provider-unavailable",
              },
            },
          },
        ],
        budgets: {
          scanMaxMs: 100,
          previewMaxMs: 250,
          maxRssDeltaBytes: 16777216,
          maxPreviewBytes: 1048576,
        },
      },
    ],
  };
}

async function writeManifest(manifestPath, manifest) {
  await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}
