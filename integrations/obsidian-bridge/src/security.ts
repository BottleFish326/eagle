import { realpath, stat } from 'node:fs/promises';
import path from 'node:path';

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

const ALLOWED_MIME_BY_EXTENSION: Readonly<Record<string, string>> = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.avif': 'image/avif',
};

export function isUuid(value: string): boolean {
  return UUID_PATTERN.test(value);
}

export function safeMimeForPath(assetPath: string): string | undefined {
  return ALLOWED_MIME_BY_EXTENSION[path.extname(assetPath).toLowerCase()];
}

export async function resolveInsideAuthorizedRoot(
  rootPath: string,
  assetPath: string,
): Promise<{ rootPath: string; assetPath: string }> {
  const [canonicalRoot, canonicalAsset] = await Promise.all([
    realpath(rootPath),
    realpath(assetPath),
  ]);
  const relative = path.relative(canonicalRoot, canonicalAsset);
  if (relative === '' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error('asset escapes its authorized root');
  }
  const assetStat = await stat(canonicalAsset);
  if (!assetStat.isFile()) {
    throw new Error('resolved asset is not a file');
  }
  return { rootPath: canonicalRoot, assetPath: canonicalAsset };
}

export function assetPathFromSidecar(sidecarPath: string): string {
  const suffix = '.asset.yml';
  if (!sidecarPath.endsWith(suffix)) {
    throw new Error('not an asset sidecar path');
  }
  return sidecarPath.slice(0, -suffix.length);
}
