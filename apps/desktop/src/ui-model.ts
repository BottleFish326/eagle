import type { AssetKind, QueryAssetsError } from "./asset-query";
import type { AssetIssue, AssetRecord } from "./scanner";

export type TagFilterState = "neutral" | "include" | "exclude";

export type TagFilterMap = Readonly<Record<string, TagFilterState>>;

export interface TagSummary {
  tag: string;
  count: number;
  state: TagFilterState;
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
    return error.kind === "parse" ? error.error.message : error.message;
  }
  if (error instanceof Error) return error.message;
  return typeof error === "string" ? error : "查询失败，请重试";
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
    } else if (!asset.tags.includes(token)) {
      return false;
    }
  }
  return true;
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
