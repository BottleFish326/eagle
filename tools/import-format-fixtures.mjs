import { createHash, randomUUID } from "node:crypto";
import { lstat, mkdir, open, readFile, rename, unlink } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repository = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const fixtureRoot = path.join(repository, "fixtures", "formats", "sources");
const sourceDirectory = parseSourceDirectory(process.argv.slice(2));

const sources = [
  {
    path: "avif/libheif-example.avif",
    fileName: "example.avif",
    url: "https://raw.githubusercontent.com/strukturag/libheif/3a6997e8c4d4df7c20dfcb2937484630e05f5570/examples/example.avif",
    size: 113_604,
    sha256: "54a0dc31d02b6f5d9d4b66027d4787861b7af15ffd8fab8eab963d10c5411469",
  },
  {
    path: "heic/libheif-example.heic",
    fileName: "example.heic",
    url: "https://raw.githubusercontent.com/strukturag/libheif/b97d0c2b2353c8c132f334729fc75e2d47d3763d/examples/example.heic",
    size: 718_114,
    sha256: "7f8b363e4936c0666a25f64f3a92fda10bd8e5453be4592530b65a55dd98f3f2",
  },
];

for (const source of sources) {
  await importSource(source);
}

console.log(`Imported ${sources.length} pinned libheif fixtures.`);

async function importSource(source) {
  const target = path.resolve(fixtureRoot, source.path);
  if (!target.startsWith(`${fixtureRoot}${path.sep}`)) {
    throw new Error(`Fixture path escapes source root: ${source.path}`);
  }

  await mkdir(path.dirname(target), { recursive: true });
  await assertRealDirectoryChain(path.dirname(target));

  try {
    const existing = await readFile(target);
    verifyBytes(source, existing);
    return;
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }

  const bytes = sourceDirectory
    ? await readFile(path.join(sourceDirectory, source.fileName))
    : await download(source.url);
  verifyBytes(source, bytes);

  const temporary = `${target}.import-${process.pid}-${randomUUID()}.tmp`;
  let temporaryOwned = false;
  try {
    const handle = await open(temporary, "wx", 0o644);
    temporaryOwned = true;
    try {
      await handle.writeFile(bytes);
      await handle.sync();
    } finally {
      await handle.close();
    }
    await rename(temporary, target);
    temporaryOwned = false;
  } finally {
    if (temporaryOwned) await unlink(temporary).catch(() => {});
  }
}

async function download(url) {
  const response = await fetch(url, {
    redirect: "error",
    signal: AbortSignal.timeout(30_000),
  });
  if (!response.ok) {
    throw new Error(`Download failed (${response.status}): ${url}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

function parseSourceDirectory(arguments_) {
  if (arguments_.length === 0) return undefined;
  if (arguments_.length !== 2 || arguments_[0] !== "--source-dir") {
    throw new Error(
      "Usage: node tools/import-format-fixtures.mjs [--source-dir DIRECTORY]",
    );
  }
  return path.resolve(arguments_[1]);
}

function verifyBytes(source, bytes) {
  const digest = createHash("sha256").update(bytes).digest("hex");
  if (bytes.length !== source.size || digest !== source.sha256) {
    throw new Error(
      `Pinned fixture mismatch for ${source.path}: size=${bytes.length}, sha256=${digest}`,
    );
  }
}

async function assertRealDirectoryChain(directory) {
  let current = directory;
  while (current.startsWith(repository) && current !== repository) {
    const metadata = await lstat(current);
    if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
      throw new Error(`Fixture directory is not a real directory: ${current}`);
    }
    current = path.dirname(current);
  }
}
