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
  decoderVersion: string;
}

export type ThumbnailPlaceholderReason =
  | "missing-asset"
  | "unsupported-format"
  | "unreadable"
  | "decode-failed"
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

export type ThumbnailCommandError =
  | { kind: "asset-not-found"; assetKey: string }
  | { kind: "invalid-request"; message: string }
  | { kind: "cache"; message: string }
  | { kind: "internal"; message: string };

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
