import { invoke } from "@tauri-apps/api/core";

export interface ThumbnailRequest {
  assetKey: string;
  maxEdge: number;
}

export interface ThumbnailReady {
  assetKey: string;
  cacheKey: string;
  mime: "image/png";
  width: number;
  height: number;
  sourceSize: number;
  sourceModifiedUnixMs: number;
  cacheHit: boolean;
  providerId: string;
  providerVersion: string;
  decoderVersion: string;
}

export type ThumbnailPlaceholderReason =
  | "missing-asset"
  | "codec-unavailable"
  | "preview-unavailable"
  | "unsupported-format"
  | "unreadable"
  | "invalid-content"
  | "decode-failed"
  | "resource-limited"
  | "timed-out"
  | "source-changed";

export type ThumbnailOutcome =
  | { status: "ready"; thumbnail: ThumbnailReady }
  | {
      status: "placeholder";
      assetKey: string;
      reason: ThumbnailPlaceholderReason;
      message: string;
    };

export interface CacheClearReport {
  removedFiles: number;
  removedBytes: number;
}

export interface CacheStats {
  layoutVersion: number;
  fileCount: number;
  entryCount: number;
  byteCount: number;
  maxEntries: number;
  maxBytes: number;
  retentionDays: number;
  decoderVersion: string;
}

export interface CacheMaintenanceReport {
  removedEntries: number;
  removedFiles: number;
  removedBytes: number;
  incompatibleEntries: number;
  orphanEntries: number;
  expiredEntries: number;
  capacityEntries: number;
  stats: CacheStats;
}

export type ThumbnailCommandError =
  | { kind: "asset-not-found"; assetKey: string }
  | { kind: "invalid-request"; message: string }
  | { kind: "cache"; message: string }
  | { kind: "internal"; message: string }
  | { kind: "recovery-busy"; activeScans: number; message: string }
  | { kind: "recovery-incomplete"; pendingRoots: number; message: string };

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

/**
 * Generates a thumbnail only when the view asks for one. P1-07's viewport
 * observer should call this for visible cards instead of during library scan.
 */
export function requestThumbnail(
  input: ThumbnailRequest,
  call: Invoke = invoke,
): Promise<ThumbnailOutcome> {
  return call<ThumbnailOutcome>("request_thumbnail", { input });
}

/** Reads cached PNG bytes through Tauri's raw IPC response path. */
export function readThumbnail(
  cacheKey: string,
  call: Invoke = invoke,
): Promise<ArrayBuffer> {
  return call<ArrayBuffer>("read_thumbnail", { cacheKey });
}

export function clearThumbnailCache(
  call: Invoke = invoke,
): Promise<CacheClearReport> {
  return call<CacheClearReport>("clear_thumbnail_cache");
}

export function maintainThumbnailCache(
  call: Invoke = invoke,
): Promise<CacheMaintenanceReport> {
  return call<CacheMaintenanceReport>("maintain_thumbnail_cache");
}
