import path from 'node:path';

import { isUuid, resolveInsideAuthorizedRoot } from './security';

const MATERIAL_PREFIX = 'material://';

export function parseMaterialUri(uri: string): string | undefined {
  if (!uri.startsWith(MATERIAL_PREFIX)) {
    return undefined;
  }
  const id = uri.slice(MATERIAL_PREFIX.length);
  return isUuid(id) ? id.toLowerCase() : undefined;
}

export function materialMarkdown(id: string, alias = ''): string {
  if (!isUuid(id)) {
    throw new Error('material reference requires a UUID');
  }
  const safeAlias = alias.replaceAll('[', '\\[').replaceAll(']', '\\]');
  return `![${safeAlias}](${MATERIAL_PREFIX}${id.toLowerCase()})`;
}

export async function vaultEmbedMarkdown(vaultRoot: string, assetPath: string): Promise<string> {
  const resolved = await resolveInsideAuthorizedRoot(vaultRoot, assetPath);
  const relative = path
    .relative(resolved.rootPath, resolved.assetPath)
    .split(path.sep)
    .join('/');
  if (/[\[\]|#^]/u.test(relative)) {
    throw new Error('asset path contains characters that are unsafe in an Obsidian wikilink');
  }
  return `![[${relative}]]`;
}
