import { invoke } from "@tauri-apps/api/core";

import type { AssetRecord } from "./scanner";

export interface MetadataPatch {
  setTags?: string[];
  addTags?: string[];
  removeTags?: string[];
  rating?: number;
  favorite?: boolean;
  note?: string;
  aliases?: string[];
}

export interface AssetEditTarget {
  key: string;
  expectedSidecarDigest: string | null;
}

export interface BatchMetadataEdit {
  targets: AssetEditTarget[];
  patch: MetadataPatch;
}

export interface MetadataEditFailure {
  key: string;
  kind: "not-found" | "conflict" | "invalid-input" | "write-failed";
  message: string;
}

export interface BatchMetadataEditResult {
  updated: AssetRecord[];
  failures: MetadataEditFailure[];
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function editAssetMetadata(
  input: BatchMetadataEdit,
  call: Invoke = invoke,
): Promise<BatchMetadataEditResult> {
  return call<BatchMetadataEditResult>("edit_asset_metadata", { input });
}
