import { isUuid } from './security';

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
