import { invoke } from "@tauri-apps/api/core";

export type AssetKind = "image" | "video" | "audio" | "pdf" | "other";

export interface AssetQuery {
  allTags: string[];
  anyTagGroups: string[][];
  excludedTags: string[];
  kinds: AssetKind[];
  extensions: string[];
  favorite: boolean | null;
}

export interface QueryAssetsInput {
  expression: string;
}

export interface QueryAssetsResult {
  expression: string;
  query: AssetQuery;
  keys: string[];
  totalAssets: number;
}

export type QueryParseErrorKind =
  | "unclosed-quote"
  | "trailing-escape"
  | "empty-tag"
  | "tag-too-long"
  | "invalid-wildcard"
  | "invalid-or-group"
  | "unknown-filter"
  | "invalid-type"
  | "invalid-extension"
  | "invalid-favorite"
  | "conflicting-favorite";

export interface QueryParseError {
  kind: QueryParseErrorKind;
  offset: number;
  token: string | null;
  message: string;
}

export type QueryAssetsError =
  | { kind: "parse"; error: QueryParseError }
  | { kind: "internal"; message: string };

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function queryAssets(
  input: QueryAssetsInput,
  call: Invoke = invoke,
): Promise<QueryAssetsResult> {
  return call<QueryAssetsResult>("query_assets", { input });
}
