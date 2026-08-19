export interface ObjectUrlSnapshot {
  active: number;
  peakActive: number;
  created: number;
  revoked: number;
}

const activeUrls = new Set<string>();
let peakActive = 0;
let created = 0;
let revoked = 0;

export function createTrackedObjectUrl(
  blob: Blob,
  create: (value: Blob) => string = URL.createObjectURL.bind(URL),
): string {
  const url = create(blob);
  activeUrls.add(url);
  created += 1;
  peakActive = Math.max(peakActive, activeUrls.size);
  return url;
}

export function revokeTrackedObjectUrl(
  url: string,
  revoke: (value: string) => void = URL.revokeObjectURL.bind(URL),
): void {
  if (!activeUrls.delete(url)) return;
  revoke(url);
  revoked += 1;
}

export function objectUrlSnapshot(): ObjectUrlSnapshot {
  return {
    active: activeUrls.size,
    peakActive,
    created,
    revoked,
  };
}

export function resetObjectUrlRegistryForTests(): void {
  activeUrls.clear();
  peakActive = 0;
  created = 0;
  revoked = 0;
}
