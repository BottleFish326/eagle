import { invoke } from "@tauri-apps/api/core";

import type { AssetRecord } from "./scanner";
import type { MetadataTransactionSummary } from "./metadata-transactions";
import type { MetadataConflict } from "./metadata-conflicts";

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
  expectedSidecarSize: number | null;
  expectedSidecarModifiedUnixMs: number | null;
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
  transaction: MetadataTransactionSummary | null;
  conflicts: MetadataConflict[];
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
