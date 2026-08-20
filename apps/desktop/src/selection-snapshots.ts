import { invoke } from "@tauri-apps/api/core";

import type { QueryParseErrorKind } from "./asset-query";
import type {
  SavedFilterSortDirection,
  SavedFilterSortField,
} from "./saved-filters";

export interface SelectionSort {
  field: SavedFilterSortField | "asset-key";
  direction: SavedFilterSortDirection;
}

export interface QuerySelectionInput {
  expectedCatalogRevision: number;
  expression: string;
  scopeRootIds: string[];
  sort: SelectionSort;
}

export interface RangeSelectionInput extends QuerySelectionInput {
  anchorKey: string;
  targetKey: string;
}

export interface ExplicitSelectionInput {
  expectedCatalogRevision: number;
  keys: string[];
}

export interface SelectionSnapshotSummary {
  id: string;
  catalogRevision: number;
  itemCount: number;
  createdAt: string;
  expiresAt: string;
}

export interface SelectionSessionStats {
  snapshotCount: number;
  totalItemCount: number;
  maximumSnapshotCount: number;
  maximumItemCount: number;
  maximumTotalItemCount: number;
}

export type SelectionCommandErrorKind =
  | "snapshot-not-found"
  | "snapshot-expired"
  | "catalog-changed"
  | "asset-missing"
  | "root-disabled"
  | "root-offline"
  | "authorization-lost"
  | "invalid-operation"
  | "output-too-large"
  | "internal";

export interface SelectionCommandError {
  kind: SelectionCommandErrorKind;
  message: string;
  actualRevision?: number;
  queryKind?: QueryParseErrorKind;
  queryOffset?: number;
  rootId?: string;
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function createQuerySelectionSnapshot(
  input: QuerySelectionInput,
  call: Invoke = invoke,
): Promise<SelectionSnapshotSummary> {
  return call("create_query_selection_snapshot", { input });
}

export function createRangeSelectionSnapshot(
  input: RangeSelectionInput,
  call: Invoke = invoke,
): Promise<SelectionSnapshotSummary> {
  return call("create_range_selection_snapshot", { input });
}

export function createExplicitSelectionSnapshot(
  input: ExplicitSelectionInput,
  call: Invoke = invoke,
): Promise<SelectionSnapshotSummary> {
  return call("create_explicit_selection_snapshot", { input });
}

export function releaseSelectionSnapshot(
  snapshotId: string,
  call: Invoke = invoke,
): Promise<boolean> {
  return call("release_selection_snapshot", { snapshotId });
}

export function getSelectionSessionStats(
  call: Invoke = invoke,
): Promise<SelectionSessionStats> {
  return call("selection_session_stats");
}
