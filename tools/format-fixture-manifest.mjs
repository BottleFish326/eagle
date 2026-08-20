import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { lstat, open, readFile, realpath } from "node:fs/promises";
import path from "node:path";

import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";

export const FORMAT_FIXTURE_MANIFEST_MAX_BYTES = 1024 * 1024;
export const REQUIRED_FORMAT_FIXTURE_PLATFORMS = Object.freeze([
  "windows",
  "macos",
  "linux",
]);

export async function inspectFormatFixtureManifest({
  manifestPath,
  schemaPath = path.resolve(
    import.meta.dirname,
    "..",
    "schemas",
    "format-fixture-manifest.schema.json",
  ),
}) {
  const failures = [];
  const absoluteManifest = path.resolve(manifestPath);
  const root = path.dirname(absoluteManifest);
  const rootRealPath = await inspectRoot(root, failures);
  const manifestBytes = await readManifest(absoluteManifest, failures);
  if (manifestBytes === null)
    return result(failures, absoluteManifest, rootRealPath, null);

  let manifest;
  try {
    manifest = JSON.parse(manifestBytes.toString("utf8"));
  } catch (error) {
    failures.push(`manifest JSON is invalid: ${errorMessage(error)}`);
    return result(failures, absoluteManifest, rootRealPath, null);
  }

  await validateSchema(manifest, schemaPath, failures);
  if (failures.length > 0)
    return result(failures, absoluteManifest, rootRealPath, manifest);

  const ids = new Set();
  const paths = new Set();
  let sourceBytes = 0;
  let referenceBytes = 0;
  for (const fixture of manifest.fixtures) {
    if (ids.has(fixture.id))
      failures.push(`duplicate fixture id: ${fixture.id}`);
    ids.add(fixture.id);
    if (paths.has(fixture.path))
      failures.push(`duplicate fixture path: ${fixture.path}`);
    paths.add(fixture.path);
    rejectPlaceholderHash(fixture.sha256, `${fixture.id} source`, failures);
    const source = await inspectContainedRegularFile({
      root,
      rootRealPath,
      relativePath: fixture.path,
      label: `${fixture.id} source`,
      failures,
    });
    if (source !== null) {
      sourceBytes += source.size;
      if (source.size !== fixture.size)
        failures.push(
          `${fixture.id} source size mismatch: expected ${fixture.size}, received ${source.size}`,
        );
      const digest = await sha256File(source.path);
      if (digest !== fixture.sha256)
        failures.push(
          `${fixture.id} source SHA-256 mismatch: expected ${fixture.sha256}, received ${digest}`,
        );
    }
    inspectPlatformCoverage(fixture, failures);
    for (const expectation of fixture.expectations) {
      const preview = expectation.result.preview;
      if (preview.status !== "available") continue;
      rejectPlaceholderHash(
        preview.referenceSha256,
        `${fixture.id} preview reference`,
        failures,
      );
      const reference = await inspectContainedRegularFile({
        root,
        rootRealPath,
        relativePath: preview.referencePath,
        label: `${fixture.id} preview reference`,
        failures,
      });
      if (reference === null) continue;
      referenceBytes += reference.size;
      const digest = await sha256File(reference.path);
      if (digest !== preview.referenceSha256)
        failures.push(
          `${fixture.id} preview reference SHA-256 mismatch: expected ${preview.referenceSha256}, received ${digest}`,
        );
      const dimensions = await readPngDimensions(reference.path);
      if (dimensions === null) {
        failures.push(
          `${fixture.id} preview reference is not a PNG with an IHDR header`,
        );
      } else if (
        dimensions.width !== preview.width ||
        dimensions.height !== preview.height
      ) {
        failures.push(
          `${fixture.id} preview reference dimensions mismatch: expected ${preview.width}x${preview.height}, received ${dimensions.width}x${dimensions.height}`,
        );
      }
    }
  }
  return {
    ...result(failures, absoluteManifest, rootRealPath, manifest),
    fixtureCount: manifest.fixtures.length,
    sourceBytes,
    referenceBytes,
  };
}

async function inspectRoot(root, failures) {
  try {
    const metadata = await lstat(root);
    if (metadata.isSymbolicLink())
      failures.push("fixture root must not be a symbolic link");
    if (!metadata.isDirectory())
      failures.push("fixture root is not a directory");
    return await realpath(root);
  } catch (error) {
    failures.push(`fixture root cannot be inspected: ${errorMessage(error)}`);
    return null;
  }
}

async function readManifest(manifestPath, failures) {
  try {
    const metadata = await lstat(manifestPath);
    if (metadata.isSymbolicLink()) {
      failures.push("manifest must not be a symbolic link");
      return null;
    }
    if (!metadata.isFile()) {
      failures.push("manifest is not a regular file");
      return null;
    }
    if (metadata.size > FORMAT_FIXTURE_MANIFEST_MAX_BYTES) {
      failures.push(
        `manifest exceeds ${FORMAT_FIXTURE_MANIFEST_MAX_BYTES} byte safety limit`,
      );
      return null;
    }
    return await readFile(manifestPath);
  } catch (error) {
    failures.push(`manifest cannot be read: ${errorMessage(error)}`);
    return null;
  }
}

async function validateSchema(manifest, schemaPath, failures) {
  try {
    const schema = JSON.parse(await readFile(schemaPath, "utf8"));
    const ajv = new Ajv2020({ allErrors: true, strict: true });
    addFormats(ajv);
    const validate = ajv.compile(schema);
    if (!validate(manifest)) {
      for (const error of validate.errors ?? [])
        failures.push(
          `schema ${error.instancePath || "/"} ${error.message ?? "validation failed"}`,
        );
    }
  } catch (error) {
    failures.push(`manifest schema cannot be applied: ${errorMessage(error)}`);
  }
}

async function inspectContainedRegularFile({
  root,
  rootRealPath,
  relativePath,
  label,
  failures,
}) {
  if (rootRealPath === null) return null;
  const segments = relativePath.split("/");
  let current = root;
  try {
    for (const segment of segments) {
      current = path.join(current, segment);
      const metadata = await lstat(current);
      if (metadata.isSymbolicLink()) {
        failures.push(`${label} contains a symbolic link: ${relativePath}`);
        return null;
      }
    }
    const metadata = await lstat(current);
    if (!metadata.isFile()) {
      failures.push(`${label} is not a regular file: ${relativePath}`);
      return null;
    }
    const canonical = await realpath(current);
    if (!isInside(rootRealPath, canonical)) {
      failures.push(
        `${label} resolves outside the fixture root: ${relativePath}`,
      );
      return null;
    }
    return { path: canonical, size: metadata.size };
  } catch (error) {
    failures.push(
      `${label} cannot be inspected: ${relativePath}: ${errorMessage(error)}`,
    );
    return null;
  }
}

function inspectPlatformCoverage(fixture, failures) {
  const profiles = new Map();
  for (const expectation of fixture.expectations) {
    const platforms = profiles.get(expectation.providerProfile) ?? new Set();
    for (const platform of expectation.platforms) {
      if (platforms.has(platform))
        failures.push(
          `${fixture.id} has duplicate expectation for ${expectation.providerProfile}/${platform}`,
        );
      platforms.add(platform);
    }
    profiles.set(expectation.providerProfile, platforms);
  }
  for (const [profile, platforms] of profiles) {
    for (const platform of REQUIRED_FORMAT_FIXTURE_PLATFORMS) {
      if (!platforms.has(platform))
        failures.push(
          `${fixture.id} ${profile} is missing ${platform} coverage`,
        );
    }
  }
}

function rejectPlaceholderHash(value, label, failures) {
  if (/^(?:0{64}|1{64})$/u.test(value))
    failures.push(`${label} uses a forbidden placeholder SHA-256`);
}

async function sha256File(filePath) {
  const digest = createHash("sha256");
  for await (const chunk of createReadStream(filePath)) digest.update(chunk);
  return digest.digest("hex");
}

async function readPngDimensions(filePath) {
  const handle = await open(filePath, "r");
  try {
    const header = Buffer.alloc(24);
    const { bytesRead } = await handle.read(header, 0, header.length, 0);
    if (
      bytesRead !== header.length ||
      !header.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex")) ||
      header.toString("ascii", 12, 16) !== "IHDR"
    ) {
      return null;
    }
    return { width: header.readUInt32BE(16), height: header.readUInt32BE(20) };
  } finally {
    await handle.close();
  }
}

function isInside(root, candidate) {
  const relative = path.relative(root, candidate);
  return (
    relative === "" ||
    (!relative.startsWith("..") && !path.isAbsolute(relative))
  );
}

function result(failures, manifestPath, rootRealPath, manifest) {
  return {
    schema: 1,
    accepted: failures.length === 0,
    failures,
    manifestPath,
    rootRealPath,
    suite: manifest?.suite ?? null,
    fixtureCount: Array.isArray(manifest?.fixtures)
      ? manifest.fixtures.length
      : 0,
    sourceBytes: 0,
    referenceBytes: 0,
  };
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}
