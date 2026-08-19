import { createHash } from "node:crypto";
import { lstat, readFile, readdir } from "node:fs/promises";
import path from "node:path";

export async function readPlatformMatrixBundle(inputDirectory) {
  const root = await lstat(inputDirectory);
  if (!root.isDirectory()) throw new Error("P2-A12 input is not a directory");
  const files = [];
  await walkEvidenceTree(inputDirectory, files);
  if (files.length !== 4)
    throw new Error(
      `P2-A12 bundle contains ${String(files.length)} files, expected exactly 4`,
    );
  const matrixFiles = files.filter(
    (file) => path.basename(file) === "p2-08-platform-matrix.json",
  );
  const sourceFiles = files.filter(
    (file) => path.basename(file) === "p2-a12-platform-paths.json",
  );
  if (matrixFiles.length !== 1)
    throw new Error(
      `found ${String(matrixFiles.length)} matrix files, expected exactly 1`,
    );
  if (sourceFiles.length !== 3)
    throw new Error(
      `found ${String(sourceFiles.length)} source files, expected exactly 3`,
    );

  const matrix = await readArtifactFile(inputDirectory, matrixFiles[0]);
  const sources = [];
  for (const sourcePath of sourceFiles.toSorted()) {
    const source = await readArtifactFile(inputDirectory, sourcePath);
    sources.push({
      artifactName: source.artifactName,
      fileName: source.fileName,
      sha256: sha256(source.bytes),
      report: source.report,
      bytes: source.bytes,
    });
  }
  return {
    matrixArtifactName: matrix.artifactName,
    matrixReport: matrix.report,
    matrixBytes: matrix.bytes,
    sources,
  };
}

export function platformMatrixBundleEntries(bundle) {
  return [
    {
      relativePath: path.join(
        bundle.matrixArtifactName,
        "p2-08-platform-matrix.json",
      ),
      bytes: bundle.matrixBytes,
    },
    ...bundle.sources.map((source) => ({
      relativePath: path.join(source.artifactName, source.fileName),
      bytes: source.bytes,
    })),
  ].toSorted((left, right) =>
    compareText(left.relativePath, right.relativePath),
  );
}

export async function walkEvidenceTree(directory, files) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isSymbolicLink())
      throw new Error("P2-A12 bundle must not contain symbolic links");
    if (entry.isDirectory()) await walkEvidenceTree(entryPath, files);
    else if (entry.isFile()) files.push(entryPath);
  }
}

async function readArtifactFile(inputDirectory, filePath) {
  const relative = path.relative(inputDirectory, filePath);
  const components = relative.split(path.sep);
  if (components.length !== 2)
    throw new Error(
      "P2-A12 files must be directly inside artifact directories",
    );
  const bytes = await readFile(filePath);
  if (bytes.length > 4 * 1024 * 1024)
    throw new Error(`P2-A12 file exceeds 4 MiB: ${components[1]}`);
  return {
    artifactName: components[0],
    fileName: components[1],
    bytes,
    report: JSON.parse(bytes.toString("utf8")),
  };
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}
