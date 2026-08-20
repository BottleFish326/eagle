import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  chmod,
  lstat,
  mkdtemp,
  readFile,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { packageWorker } from "./package-format-worker.mjs";

test("atomically packages a worker with its exact runtime manifest", async () => {
  const directory = await mkdtemp(
    join(tmpdir(), "material-eagle-worker-package-"),
  );
  const binary = join(directory, "worker-build-output");
  const outputDirectory = join(directory, "bundle");
  await writeFile(binary, "fixed worker bytes");
  await chmod(binary, 0o755);

  const manifest = await packageWorker({
    binary,
    outputDirectory,
    platform: "linux",
    architecture: "x86_64",
  });
  const expectedDigest = createHash("sha256")
    .update("fixed worker bytes")
    .digest("hex");
  const storedManifest = JSON.parse(
    await readFile(join(outputDirectory, "manifest.json"), "utf8"),
  );

  assert.deepEqual(storedManifest, manifest);
  assert.equal(manifest.schema, 1);
  assert.equal(manifest.providerId, "bundled-libheif");
  assert.equal(manifest.providerVersion, "libheif-1.23.1-r1");
  assert.equal(manifest.sha256, expectedDigest);
  assert.equal(
    await readFile(join(outputDirectory, manifest.executable), "utf8"),
    "fixed worker bytes",
  );
  assert.notEqual(
    (await lstat(join(outputDirectory, manifest.executable))).mode & 0o111,
    0,
  );

  await assert.rejects(
    packageWorker({
      binary,
      outputDirectory,
      platform: "linux",
      architecture: "x86_64",
    }),
    /output directory already exists/u,
  );
});

test("rejects symlinked build outputs and unsupported targets", async (context) => {
  const directory = await mkdtemp(
    join(tmpdir(), "material-eagle-worker-reject-"),
  );
  const binary = join(directory, "worker-build-output");
  await writeFile(binary, "fixed worker bytes");
  const linked = join(directory, "linked-worker");
  try {
    await symlink(binary, linked);
  } catch (error) {
    if (process.platform === "win32" && error?.code === "EPERM") {
      context.skip("symbolic link creation is unavailable");
      return;
    }
    throw error;
  }

  await assert.rejects(
    packageWorker({
      binary: linked,
      outputDirectory: join(directory, "bundle"),
      platform: "linux",
      architecture: "x86_64",
    }),
    /non-symbolic-link regular file/u,
  );
  await assert.rejects(
    packageWorker({
      binary,
      outputDirectory: join(directory, "unsupported"),
      platform: "freebsd",
      architecture: "x86_64",
    }),
    /unsupported platform/u,
  );
});
