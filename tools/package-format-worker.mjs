import { createHash } from "node:crypto";
import {
  chmod,
  copyFile,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  realpath,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import process from "node:process";
import { pathToFileURL } from "node:url";

const providerId = "bundled-libheif";
const providerVersion = "libheif-1.23.1-r1";
const schema = 1;
const supportedPlatforms = new Set(["linux", "macos", "windows"]);
const supportedArchitectures = new Set(["x86_64", "aarch64"]);

function parseArguments(arguments_) {
  const values = new Map();
  for (let index = 0; index < arguments_.length; index += 2) {
    const name = arguments_[index];
    const value = arguments_[index + 1];
    if (!name?.startsWith("--") || value === undefined) {
      throw new Error("expected --name value arguments");
    }
    if (values.has(name)) {
      throw new Error(`duplicate argument: ${name}`);
    }
    values.set(name, value);
  }
  for (const name of [
    "--binary",
    "--output-directory",
    "--platform",
    "--architecture",
  ]) {
    if (!values.has(name)) {
      throw new Error(`missing required argument: ${name}`);
    }
  }
  return {
    binary: resolve(values.get("--binary")),
    outputDirectory: resolve(values.get("--output-directory")),
    platform: values.get("--platform"),
    architecture: values.get("--architecture"),
  };
}

async function sha256(path) {
  return createHash("sha256")
    .update(await readFile(path))
    .digest("hex");
}

async function assertRegularNonSymlink(path, label) {
  const metadata = await lstat(path);
  if (metadata.isSymbolicLink() || !metadata.isFile()) {
    throw new Error(`${label} must be a non-symbolic-link regular file`);
  }
}

async function packageWorker(options) {
  if (!supportedPlatforms.has(options.platform)) {
    throw new Error(`unsupported platform: ${options.platform}`);
  }
  if (!supportedArchitectures.has(options.architecture)) {
    throw new Error(`unsupported architecture: ${options.architecture}`);
  }
  if (!isAbsolute(options.binary) || !isAbsolute(options.outputDirectory)) {
    throw new Error(
      "binary and output directory must resolve to absolute paths",
    );
  }
  await assertRegularNonSymlink(options.binary, "worker binary");
  const canonicalBinary = await realpath(options.binary);

  const outputParent = dirname(options.outputDirectory);
  await mkdir(outputParent, { recursive: true });
  const canonicalParent = await realpath(outputParent);
  const outputDirectory = resolve(
    canonicalParent,
    basename(options.outputDirectory),
  );
  try {
    await lstat(outputDirectory);
    throw new Error("output directory already exists");
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }

  const temporary = await mkdtemp(`${outputDirectory}.tmp-`);
  try {
    const executable =
      options.platform === "windows"
        ? "material-eagle-format-worker.exe"
        : "material-eagle-format-worker";
    const packagedBinary = join(temporary, executable);
    await copyFile(canonicalBinary, packagedBinary);
    if (options.platform !== "windows") {
      await chmod(packagedBinary, 0o755);
    }
    const sourceDigest = await sha256(canonicalBinary);
    const packagedDigest = await sha256(packagedBinary);
    if (sourceDigest !== packagedDigest) {
      throw new Error("packaged worker digest does not match the build output");
    }
    const manifest = {
      schema,
      platform: options.platform,
      architecture: options.architecture,
      providerId,
      providerVersion,
      executable,
      sha256: packagedDigest,
    };
    await writeFile(
      join(temporary, "manifest.json"),
      `${JSON.stringify(manifest, null, 2)}\n`,
      { encoding: "utf8", flag: "wx" },
    );
    await rename(temporary, outputDirectory);
    return manifest;
  } catch (error) {
    await rm(temporary, { recursive: true, force: true });
    throw error;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const manifest = await packageWorker(parseArguments(process.argv.slice(2)));
    process.stdout.write(`${JSON.stringify(manifest)}\n`);
  } catch (error) {
    process.stderr.write(
      `${error instanceof Error ? error.message : String(error)}\n`,
    );
    process.exitCode = 1;
  }
}

export { packageWorker, parseArguments };
