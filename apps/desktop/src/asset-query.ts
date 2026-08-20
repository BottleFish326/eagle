import { invoke } from "@tauri-apps/api/core";

export type AssetKind = "image" | "video" | "audio" | "pdf" | "other";
export type IntegerField =
  "rating" | "size" | "width" | "height" | "duration" | "pages";
export type InstantField = "created" | "modified";
export type RatioField = "aspect";
export type UnknownField =
  | "size"
  | "width"
  | "height"
  | "aspect"
  | "created"
  | "modified"
  | "duration"
  | "pages"
  | "orientation"
  | "root"
  | "color-space"
  | "has-alpha";
export type Orientation = "landscape" | "portrait" | "square";

export interface RangeBound<T> {
  value: T;
  inclusive: boolean;
}

export interface RangeConstraint<T> {
  lower: RangeBound<T> | null;
  upper: RangeBound<T> | null;
}

export interface Ratio {
  numerator: number;
  denominator: number;
}

export type NullableBoolean =
  { kind: "known"; value: boolean } | { kind: "unknown" };

export interface AssetQuery {
  allTags: string[];
  anyTagGroups: string[][];
  excludedTags: string[];
  kinds: AssetKind[];
  extensions: string[];
  favorite: boolean | null;
  integerRanges: Partial<Record<IntegerField, RangeConstraint<number>>>;
  instantRanges: Partial<Record<InstantField, RangeConstraint<number>>>;
  ratioRanges: Partial<Record<RatioField, RangeConstraint<Ratio>>>;
  unknownFields: UnknownField[];
  orientations: Orientation[];
  rootIds: string[];
  pathContains: string[];
  colorSpaces: string[];
  hasNote: boolean | null;
  hasAlpha: NullableBoolean | null;
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
  | "conflicting-favorite"
  | "invalid-operator"
  | "invalid-integer"
  | "invalid-unit"
  | "numeric-overflow"
  | "invalid-ratio"
  | "invalid-date"
  | "invalid-enum"
  | "invalid-root-id"
  | "invalid-path"
  | "unsupported-unknown"
  | "conflicting-range"
  | "conflicting-value";

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
