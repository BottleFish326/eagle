import type { AssetKind, QueryAssetsError } from "./asset-query";
import type { AssetIssue, AssetRecord } from "./scanner";

export type TagFilterState = "neutral" | "include" | "exclude";

export type TagFilterMap = Readonly<Record<string, TagFilterState>>;

export interface TagSummary {
  tag: string;
  count: number;
  state: TagFilterState;
}

export interface QueryViewState {
  visibleKeys: string[];
  error?: string;
}

export function cycleTagFilter(state: TagFilterState): TagFilterState {
  if (state === "neutral") return "include";
  if (state === "include") return "exclude";
  return "neutral";
}

export function composeAssetQuery(
  expression: string,
  filters: TagFilterMap,
): string {
  const tagTerms = Object.entries(filters)
    .filter((entry): entry is [string, Exclude<TagFilterState, "neutral">] =>
      ["include", "exclude"].includes(entry[1]),
    )
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([tag, state]) => `${state === "exclude" ? "-" : ""}${tagTerm(tag)}`);
  return [expression.trim(), ...tagTerms].filter(Boolean).join(" ");
}

export function summarizeTags(
  assets: readonly AssetRecord[],
  filters: TagFilterMap,
): TagSummary[] {
  const counts = new Map<string, number>();
  for (const asset of assets) {
    for (const tag of asset.tags) {
      counts.set(tag, (counts.get(tag) ?? 0) + 1);
    }
  }
  return [...counts]
    .map(([tag, count]) => ({
      tag,
      count,
      state: filters[tag] ?? "neutral",
    }))
    .sort(
      (left, right) =>
        right.count - left.count || left.tag.localeCompare(right.tag),
    );
}

export function upsertAssets(
  current: ReadonlyMap<string, AssetRecord>,
  records: readonly AssetRecord[],
): Map<string, AssetRecord> {
  const next = new Map(current);
  for (const record of records) next.set(record.key, record);
  return next;
}

export function removeRootAssets(
  current: ReadonlyMap<string, AssetRecord>,
  rootId: string,
): Map<string, AssetRecord> {
  return new Map([...current].filter(([, asset]) => asset.rootId !== rootId));
}

export function reconcileSelectedKeys(
  selected: ReadonlySet<string>,
  moves: readonly { fromKey: string; toKey: string }[],
  removedKeys: readonly string[],
): Set<string> {
  const moved = new Map(moves.map((move) => [move.fromKey, move.toKey]));
  const removed = new Set(removedKeys);
  const next = new Set<string>();
  for (const key of selected) {
    const replacement = moved.get(key);
    if (replacement !== undefined) next.add(replacement);
    else if (!removed.has(key)) next.add(key);
  }
  return next;
}

export function reconcileSelectionAnchor(
  anchor: string | undefined,
  moves: readonly { fromKey: string; toKey: string }[],
  removedKeys: readonly string[],
): string | undefined {
  if (anchor === undefined) return undefined;
  const replacement = moves.find((move) => move.fromKey === anchor)?.toKey;
  if (replacement !== undefined) return replacement;
  return removedKeys.includes(anchor) ? undefined : anchor;
}

export function nextGridIndex(
  current: number,
  itemCount: number,
  columns: number,
  key: "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End",
): number {
  if (itemCount === 0) return -1;
  if (key === "Home") return 0;
  if (key === "End") return itemCount - 1;
  const delta =
    key === "ArrowLeft"
      ? -1
      : key === "ArrowRight"
        ? 1
        : key === "ArrowUp"
          ? -Math.max(columns, 1)
          : Math.max(columns, 1);
  return Math.min(itemCount - 1, Math.max(0, current + delta));
}

export function issueLabel(issue: AssetIssue): string {
  switch (issue.type) {
    case "invalid-sidecar":
      return "Sidecar 无法解析";
    case "mismatched-sidecar":
      return "Sidecar 与素材指纹不匹配";
    case "unreadable-file":
      return "文件不可读";
    case "invalid-image-metadata":
      return "图片已损坏";
    case "invalid-native-metadata":
      return "原生元数据异常";
    case "mime-mismatch":
      return "文件内容与扩展名不一致";
    case "unsafe-embedded-content":
      return "已隔离活动或外部内容";
    case "resource-limited":
      return "已限制高开销解析";
    case "missing-asset":
      return "文件已丢失";
    case "unsupported-format":
      return "暂不支持预览";
  }
}

export function issueDetails(issue: AssetIssue): string | undefined {
  return "message" in issue ? issue.message : undefined;
}

export function formatBytes(bytes: number | null): string {
  if (bytes === null) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

export function formatQueryError(error: unknown): string {
  if (isQueryAssetsError(error)) {
    if (error.kind === "internal") return error.message;
    const token = error.error.token ? `，Token：${error.error.token}` : "";
    return `${error.error.message}（${error.error.kind}，UTF-8 字节 ${error.error.offset}${token}）`;
  }
  if (error instanceof Error) return error.message;
  return typeof error === "string" ? error : "查询失败，请重试";
}

export function settleSuccessfulQuery(keys: readonly string[]): QueryViewState {
  return { visibleKeys: [...keys] };
}

export function settleFailedQuery(
  current: QueryViewState,
  error: unknown,
): QueryViewState {
  return { ...current, error: formatQueryError(error) };
}

export function matchesDemoExpression(
  asset: AssetRecord,
  expression: string,
): boolean {
  for (const token of tokenize(expression)) {
    if (token.startsWith("-tag:")) {
      if (asset.tags.includes(token.slice(5))) return false;
    } else if (token.startsWith("tag:")) {
      if (!asset.tags.includes(token.slice(4))) return false;
    } else if (token.startsWith("type:")) {
      const kinds = token.slice(5).toLowerCase().split("|") as AssetKind[];
      if (!kinds.includes(asset.kind)) return false;
    } else if (token.startsWith("ext:")) {
      const extensions = token
        .slice(4)
        .toLowerCase()
        .split("|")
        .map((value) => value.replace(/^\./, ""));
      if (
        asset.extension === null ||
        !extensions.includes(asset.extension.toLowerCase())
      ) {
        return false;
      }
    } else if (token.startsWith("favorite:")) {
      if (asset.favorite !== (token.slice(9).toLowerCase() === "true"))
        return false;
    } else if (token.startsWith("-")) {
      if (asset.tags.includes(token.slice(1))) return false;
    } else if (token.startsWith("any:(") && token.endsWith(")")) {
      const tags = token.slice(5, -1).split("|");
      if (!tags.some((tag) => asset.tags.includes(tag))) return false;
    } else {
      const advancedMatch = matchesAdvancedDemoToken(asset, token);
      if (advancedMatch === false) return false;
      if (advancedMatch === undefined && !asset.tags.includes(token))
        return false;
    }
  }
  return true;
}

function matchesAdvancedDemoToken(
  asset: AssetRecord,
  token: string,
): boolean | undefined {
  const match = /^([a-z-]+):((?:<=|>=|<|>)?)(.*)$/u.exec(token);
  if (!match) return undefined;
  const [, field, operator = "", value = ""] = match;
  if (
    ![
      "rating",
      "size",
      "width",
      "height",
      "aspect",
      "created",
      "modified",
      "duration",
      "pages",
      "orientation",
      "root",
      "path",
      "color-space",
      "has-note",
      "has-alpha",
    ].includes(field)
  ) {
    return undefined;
  }
  const dimensions = effectiveDemoDimensions(asset);
  if (field === "orientation") {
    const orientation =
      dimensions === null
        ? null
        : dimensions.width > dimensions.height
          ? "landscape"
          : dimensions.width < dimensions.height
            ? "portrait"
            : "square";
    return matchesDemoEnum(orientation, value);
  }
  if (field === "root") return matchesDemoEnum(asset.rootId, value);
  if (field === "color-space") {
    return matchesDemoEnum(asset.media?.colorSpace ?? null, value);
  }
  if (field === "has-note") {
    return asset.note.trim().length > 0 === (value === "true");
  }
  if (field === "has-alpha") {
    const alpha = asset.media?.hasAlpha ?? null;
    return value === "unknown"
      ? alpha === null
      : alpha !== null && alpha === (value === "true");
  }
  if (field === "path") {
    return asset.relativePath.normalize("NFC").includes(value.normalize("NFC"));
  }

  const known =
    field === "rating"
      ? asset.rating
      : field === "size"
        ? asset.size
        : field === "width"
          ? (dimensions?.width ?? null)
          : field === "height"
            ? (dimensions?.height ?? null)
            : field === "created"
              ? asset.createdUnixMs
              : field === "modified"
                ? asset.modifiedUnixMs
                : field === "duration"
                  ? (asset.media?.durationMs ?? null)
                  : field === "pages"
                    ? (asset.media?.pageCount ?? null)
                    : dimensions === null
                      ? null
                      : dimensions.width / dimensions.height;
  if (value === "unknown") return known === null;
  if (known === null) return false;
  const expected =
    field === "size"
      ? parseDemoUnit(value, "size")
      : field === "duration"
        ? parseDemoUnit(value, "duration")
        : field === "created" || field === "modified"
          ? Date.parse(value)
          : field === "aspect"
            ? parseDemoRatio(value)
            : Number(value);
  if (!Number.isFinite(expected)) return false;
  return compareDemoNumber(known, expected, operator);
}

function effectiveDemoDimensions(
  asset: AssetRecord,
): { width: number; height: number } | null {
  if (asset.dimensions === null) return null;
  const nativeQuarterTurn =
    asset.nativeMetadata?.orientation !== null &&
    asset.nativeMetadata?.orientation !== undefined &&
    asset.nativeMetadata.orientation >= 5 &&
    asset.nativeMetadata.orientation <= 8;
  const videoQuarterTurn =
    asset.media?.displayQuarterTurns !== null &&
    asset.media?.displayQuarterTurns !== undefined &&
    Math.abs(asset.media.displayQuarterTurns) % 2 === 1;
  return nativeQuarterTurn || videoQuarterTurn
    ? { width: asset.dimensions.height, height: asset.dimensions.width }
    : asset.dimensions;
}

function matchesDemoEnum(known: string | null, value: string): boolean {
  if (value === "unknown") return known === null;
  return known !== null && value.split("|").includes(known);
}

function parseDemoUnit(value: string, kind: "size" | "duration"): number {
  const units: Readonly<Record<string, number>> =
    kind === "size"
      ? { TiB: 1024 ** 4, GiB: 1024 ** 3, MiB: 1024 ** 2, KiB: 1024, B: 1 }
      : { min: 60_000, ms: 1, s: 1_000, h: 3_600_000 };
  const unit = Object.keys(units).find((candidate) =>
    value.endsWith(candidate),
  );
  const number = unit === undefined ? value : value.slice(0, -unit.length);
  return (
    Number(number) * (unit === undefined ? 1 : (units[unit] ?? Number.NaN))
  );
}

function parseDemoRatio(value: string): number {
  const [numerator, denominator, extra] = value.split("/");
  if (extra !== undefined) return Number.NaN;
  return Number(numerator) / Number(denominator);
}

function compareDemoNumber(
  actual: number,
  expected: number,
  operator: string,
): boolean {
  if (operator === "<") return actual < expected;
  if (operator === "<=") return actual <= expected;
  if (operator === ">") return actual > expected;
  if (operator === ">=") return actual >= expected;
  return actual === expected;
}

function tagTerm(tag: string): string {
  return `tag:"${tag.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`;
}

function tokenize(expression: string): string[] {
  const tokens: string[] = [];
  let current = "";
  let quoted = false;
  let escaped = false;
  for (const character of expression) {
    if (escaped) {
      current += character;
      escaped = false;
    } else if (character === "\\") {
      escaped = true;
    } else if (character === '"') {
      quoted = !quoted;
    } else if (/\s/u.test(character) && !quoted) {
      if (current.length > 0) tokens.push(current);
      current = "";
    } else {
      current += character;
    }
  }
  if (current.length > 0) tokens.push(current);
  return tokens;
}

function isQueryAssetsError(error: unknown): error is QueryAssetsError {
  return (
    typeof error === "object" &&
    error !== null &&
    "kind" in error &&
    ((error as { kind?: unknown }).kind === "parse" ||
      (error as { kind?: unknown }).kind === "internal")
  );
}
