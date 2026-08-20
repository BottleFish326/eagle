import { invoke } from "@tauri-apps/api/core";
import type { QueryParseErrorKind } from "./asset-query";

export interface SavedFilterFileVersion {
  exists: boolean;
  size: number;
  modifiedUnixMs: number | null;
  sha256: string | null;
}

export type SavedFilterFileIssueKind =
  "invalid-file" | "file-too-large" | "unsupported-schema";

export interface SavedFilterFileIssue {
  kind: SavedFilterFileIssueKind;
}

export type SavedFilterEntryIssueKind =
  | "invalid-entry"
  | "duplicate-id"
  | "duplicate-name"
  | "invalid-query"
  | "unknown-sort";

export interface SavedFilterQueryIssue {
  kind: QueryParseErrorKind;
  offset: number;
}

export interface InvalidSavedFilterEntry {
  index: number;
  id: string | null;
  issues: SavedFilterEntryIssueKind[];
  queryIssue: SavedFilterQueryIssue | null;
}

export type SavedFilterScope =
  { kind: "all-enabled-roots" } | { kind: "selected-roots"; rootIds: string[] };

export type SavedFilterSortField =
  | "file-name"
  | "modified-at"
  | "created-at"
  | "file-size"
  | "rating"
  | "asset-kind";

export type SavedFilterSortDirection = "ascending" | "descending";

export interface SavedFilterSort {
  field: SavedFilterSortField;
  direction: SavedFilterSortDirection;
}

export interface SavedFilter {
  id: string;
  name: string;
  query: string;
  scope: SavedFilterScope;
  sort: SavedFilterSort;
  createdAt: string;
  updatedAt: string;
}

export interface UnavailableSavedFilter {
  filter: SavedFilter;
  missingRootIds: string[];
}

export interface SavedFilterCatalog {
  fileVersion: SavedFilterFileVersion;
  validFilters: SavedFilter[];
  unavailableFilters: UnavailableSavedFilter[];
  invalidEntries: InvalidSavedFilterEntry[];
  fileIssues: SavedFilterFileIssue[];
}

export interface SavedFilterInput {
  name: string;
  query: string;
  scope: SavedFilterScope;
  sort: SavedFilterSort;
}

export interface SavedFilterMutation {
  fileVersion: SavedFilterFileVersion;
  filter: SavedFilter | null;
}

export interface SavedFilterExecution {
  filterId: string;
  expression: string;
  orderedKeys: string[];
  totalAssets: number;
  scopedAssets: number;
  matchedAssets: number;
  effectiveRootIds: string[];
  missingRootIds: string[];
  sort: SavedFilterSort;
  catalogRevision: number;
}

export type SavedFilterCommandErrorKind =
  | "invalid-file"
  | "file-too-large"
  | "unsupported-schema"
  | "invalid-entry"
  | "duplicate-id"
  | "duplicate-name"
  | "invalid-query"
  | "unknown-sort"
  | "external-change"
  | "not-found"
  | "internal";

export interface SavedFilterCommandError {
  kind: SavedFilterCommandErrorKind;
  message: string;
  actualVersion?: SavedFilterFileVersion;
  queryKind?: QueryParseErrorKind;
  queryOffset?: number;
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function listSavedFilters(
  call: Invoke = invoke,
): Promise<SavedFilterCatalog> {
  return call<SavedFilterCatalog>("list_saved_filters");
}

export function createSavedFilter(
  expectedVersion: SavedFilterFileVersion,
  input: SavedFilterInput,
  call: Invoke = invoke,
): Promise<SavedFilterMutation> {
  return call<SavedFilterMutation>("create_saved_filter", {
    expectedVersion,
    input,
  });
}

export function updateSavedFilter(
  expectedVersion: SavedFilterFileVersion,
  id: string,
  input: SavedFilterInput,
  call: Invoke = invoke,
): Promise<SavedFilterMutation> {
  return call<SavedFilterMutation>("update_saved_filter", {
    expectedVersion,
    id,
    input,
  });
}

export function renameSavedFilter(
  expectedVersion: SavedFilterFileVersion,
  id: string,
  name: string,
  call: Invoke = invoke,
): Promise<SavedFilterMutation> {
  return call<SavedFilterMutation>("rename_saved_filter", {
    expectedVersion,
    id,
    name,
  });
}

export function deleteSavedFilter(
  expectedVersion: SavedFilterFileVersion,
  id: string,
  call: Invoke = invoke,
): Promise<SavedFilterMutation> {
  return call<SavedFilterMutation>("delete_saved_filter", {
    expectedVersion,
    id,
  });
}

export function executeSavedFilter(
  id: string,
  call: Invoke = invoke,
): Promise<SavedFilterExecution> {
  return call<SavedFilterExecution>("execute_saved_filter", { id });
}
