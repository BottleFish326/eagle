import { invoke } from "@tauri-apps/api/core";

import type { AssetRecord } from "./scanner";

export type MetadataConflictField =
  "tags" | "rating" | "favorite" | "note" | "aliases";

export interface UserMetadataSnapshot {
  tags: string[];
  rating: number;
  favorite: boolean;
  note: string;
  aliases: string[];
}

export interface MetadataConflict {
  id: string;
  key: string;
  fileName: string;
  source: "external-edit";
  sidecarModifiedUnixMs: number;
  identityChanged: boolean;
  base: UserMetadataSnapshot;
  current: UserMetadataSnapshot;
  proposed: UserMetadataSnapshot;
  externallyChangedFields: MetadataConflictField[];
  conflictingFields: MetadataConflictField[];
}

export type TagConflictResolution = "merge" | "keep-external" | "use-mine";
export type FieldConflictResolution = "keep-external" | "use-mine";

export interface MetadataConflictResolution {
  tags?: TagConflictResolution;
  fields: Partial<Record<MetadataConflictField, FieldConflictResolution>>;
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function resolveMetadataConflict(
  conflictId: string,
  resolution: MetadataConflictResolution,
  call: Invoke = invoke,
): Promise<AssetRecord> {
  return call<AssetRecord>("resolve_metadata_conflict", {
    input: { conflictId, resolution },
  });
}

export function dismissMetadataConflict(
  conflictId: string,
  call: Invoke = invoke,
): Promise<void> {
  return call<void>("dismiss_metadata_conflict", { conflictId });
}
