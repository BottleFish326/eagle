import { opendir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { parse } from 'yaml';

import {
  assetPathFromSidecar,
  isUuid,
  resolveInsideAuthorizedRoot,
  safeMimeForPath,
} from './security';
import type { AssetLocation, AssetSidecar, IndexResult } from './types';

export async function buildAssetIndex(rootPaths: readonly string[]): Promise<IndexResult> {
  const assets = new Map<string, AssetLocation>();
  const problems: IndexResult['problems'] = [];

  for (const rootPath of rootPaths) {
    try {
      for await (const sidecarPath of walkSidecars(rootPath)) {
        try {
          const sidecar = await readSidecar(sidecarPath);
          const assetPath = assetPathFromSidecar(sidecarPath);
          const resolved = await resolveInsideAuthorizedRoot(rootPath, assetPath);
          const mime = safeMimeForPath(resolved.assetPath);
          if (mime === undefined) {
            throw new Error('asset MIME type is not allowed by the prototype');
          }
          if (assets.has(sidecar.id)) {
            throw new Error(`duplicate asset ID: ${sidecar.id}`);
          }
          assets.set(sidecar.id, {
            id: sidecar.id,
            rootPath: resolved.rootPath,
            assetPath: resolved.assetPath,
            sidecarPath,
            mime,
            tags: sidecar.tags,
          });
        } catch (error) {
          problems.push({
            path: sidecarPath,
            message: error instanceof Error ? error.message : String(error),
          });
        }
      }
    } catch (error) {
      problems.push({
        path: rootPath,
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }

  return { assets, problems };
}

async function* walkSidecars(rootPath: string): AsyncGenerator<string> {
  const directory = await opendir(rootPath);
  for await (const entry of directory) {
    if (entry.name.startsWith('.') || entry.isSymbolicLink()) {
      continue;
    }
    const entryPath = path.join(rootPath, entry.name);
    if (entry.isDirectory()) {
      yield* walkSidecars(entryPath);
    } else if (entry.isFile() && entry.name.endsWith('.asset.yml')) {
      yield entryPath;
    }
  }
}

async function readSidecar(sidecarPath: string): Promise<AssetSidecar> {
  const document = parse(await readFile(sidecarPath, 'utf8')) as unknown;
  if (!isRecord(document)) {
    throw new Error('sidecar must be a YAML mapping');
  }
  if (document.schema !== 1) {
    throw new Error('unsupported sidecar schema');
  }
  if (typeof document.id !== 'string' || !isUuid(document.id)) {
    throw new Error('sidecar ID must be a UUID');
  }
  if (!Array.isArray(document.tags) || document.tags.some((tag) => typeof tag !== 'string')) {
    throw new Error('sidecar tags must be an array of strings');
  }
  if (typeof document.updatedAt !== 'string') {
    throw new Error('sidecar updatedAt must be a string');
  }
  return document as AssetSidecar;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
