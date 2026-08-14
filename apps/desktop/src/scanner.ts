import { Channel, invoke } from "@tauri-apps/api/core";

export type AssetIssue =
  | { type: "invalid-sidecar"; message: string }
  | { type: "unreadable-file"; message: string }
  | { type: "invalid-image-metadata"; message: string }
  | { type: "invalid-native-metadata"; message: string }
  | { type: "missing-asset" }
  | { type: "unsupported-format" };

export interface AssetDimensions {
  width: number;
  height: number;
}

export interface NativeImageMetadata {
  orientation: number | null;
  capturedAt: string | null;
  cameraMake: string | null;
  cameraModel: string | null;
  lensModel: string | null;
  software: string | null;
  artist: string | null;
  copyright: string | null;
}

export interface SidecarState {
  schema: number;
  digest: string;
  updatedAt: string;
}

export interface AssetRecord {
  key: string;
  id: string | null;
  rootId: string | null;
  path: string;
  relativePath: string;
  sidecarPath: string | null;
  sidecarState: SidecarState | null;
  fileName: string;
  extension: string | null;
  mime: string;
  kind: "image" | "video" | "audio" | "pdf" | "other";
  size: number | null;
  createdUnixMs: number | null;
  modifiedUnixMs: number | null;
  fileReadOnly: boolean | null;
  dimensions: AssetDimensions | null;
  nativeMetadata: NativeImageMetadata | null;
  tags: string[];
  rating: number;
  favorite: boolean;
  note: string;
  aliases: string[];
  issues: AssetIssue[];
}

export interface ScanProblem {
  path: string;
  message: string;
}

export interface ScanBatch {
  sequence: number;
  assets: AssetRecord[];
  problems: ScanProblem[];
  visitedFiles: number;
}

export interface ScanSummary {
  rootId: string | null;
  root: string;
  completion: "completed" | "cancelled";
  visitedFiles: number;
  assetCount: number;
  problemCount: number;
  elapsedMs: number;
}

export type LibraryScanEvent =
  | { event: "started"; data: { scanId: string; rootId: string; root: string } }
  | { event: "batch"; data: { scanId: string; batch: ScanBatch } }
  | { event: "finished"; data: { scanId: string; summary: ScanSummary } }
  | { event: "failed"; data: { scanId: string; message: string } };

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

interface ScanChannel {
  onmessage: (message: LibraryScanEvent) => void;
}

type ChannelFactory = () => ScanChannel;

export function startLibraryScan(
  rootId: string,
  receive: (event: LibraryScanEvent) => void,
  call: Invoke = invoke,
  createChannel: ChannelFactory = () => new Channel<LibraryScanEvent>(),
): Promise<string> {
  const onEvent = createChannel();
  onEvent.onmessage = receive;
  return call<string>("start_library_scan", { rootId, onEvent });
}

export function cancelLibraryScan(
  scanId: string,
  call: Invoke = invoke,
): Promise<boolean> {
  return call<boolean>("cancel_library_scan", { scanId });
}
