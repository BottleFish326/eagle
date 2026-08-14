import type { AssetQuery, QueryAssetsResult } from "./asset-query";
import type { DesktopApi } from "./desktop-api";
import type { LibraryRootStatus } from "./library-roots";
import type { BatchMetadataEditResult } from "./metadata-editor";
import type { AssetRecord, LibraryScanEvent } from "./scanner";
import type { ThumbnailOutcome } from "./thumbnail";
import { matchesDemoExpression } from "./ui-model";

const DEMO_ROOT_ID = "0198a9b2-43c0-7cb0-a733-6dc58f829814";

export function createDemoDesktopApi(): DesktopApi {
  let roots: LibraryRootStatus[] = [demoRoot()];
  let assets = demoAssets();
  const previewKeys = new Map<string, string>();
  const timers = new Map<string, number[]>();

  return {
    async listLibraryRoots() {
      return structuredClone(roots);
    },
    async addLibraryRoot(input) {
      const root: LibraryRootStatus = {
        id: demoUuid(roots.length + 20),
        path: input.path,
        name: input.name,
        enabled: true,
        scan: {
          recursive: true,
          followSymlinks: false,
          ignore: input.ignore ?? [],
        },
        accessStatus: "available",
      };
      roots = [...roots, root];
      return structuredClone(root);
    },
    async updateLibraryRoot(input) {
      const current = roots.find((root) => root.id === input.id);
      if (current === undefined) throw new Error("素材根不存在");
      const updated: LibraryRootStatus = {
        ...current,
        name: input.name ?? current.name,
        enabled: input.enabled ?? current.enabled,
        scan: {
          ...current.scan,
          ignore: input.ignore ?? current.scan.ignore,
        },
      };
      roots = roots.map((root) => (root.id === updated.id ? updated : root));
      return structuredClone(updated);
    },
    async removeLibraryRoot(id) {
      const current = roots.find((root) => root.id === id);
      if (current === undefined) throw new Error("素材根不存在");
      roots = roots.filter((root) => root.id !== id);
      assets = assets.filter((asset) => asset.rootId !== id);
      return structuredClone(current);
    },
    async startLibraryScan(rootId, receive) {
      const scanId = demoUuid(Date.now() % 1000);
      const root = roots.find((candidate) => candidate.id === rootId);
      if (root === undefined) throw new Error("素材根不存在");
      const matching = assets.filter((asset) => asset.rootId === rootId);
      const eventTimers = [
        window.setTimeout(
          () =>
            receive({
              event: "started",
              data: { scanId, rootId, root: root.path },
            }),
          40,
        ),
        window.setTimeout(
          () => receive(batchEvent(scanId, matching.slice(0, 8), 0)),
          180,
        ),
        window.setTimeout(
          () => receive(batchEvent(scanId, matching.slice(8), 1)),
          360,
        ),
        window.setTimeout(
          () =>
            receive({
              event: "finished",
              data: {
                scanId,
                summary: {
                  rootId,
                  root: root.path,
                  completion: "completed",
                  visitedFiles: matching.length,
                  assetCount: matching.length,
                  problemCount: 1,
                  elapsedMs: 412,
                },
              },
            }),
          500,
        ),
      ];
      timers.set(scanId, eventTimers);
      return scanId;
    },
    async cancelLibraryScan(scanId) {
      const active = timers.get(scanId);
      if (active === undefined) return false;
      active.forEach(window.clearTimeout);
      timers.delete(scanId);
      return true;
    },
    async queryAssets(input) {
      if (/\bkind:/u.test(input.expression)) {
        throw {
          kind: "parse",
          error: {
            kind: "unknown-filter",
            offset: input.expression.indexOf("kind:"),
            token: "kind:image",
            message: "未知过滤器，请使用 type:image",
          },
        };
      }
      const keys = assets
        .filter((asset) => matchesDemoExpression(asset, input.expression))
        .map((asset) => asset.key)
        .sort();
      return {
        expression: input.expression,
        query: emptyQuery(),
        keys,
        totalAssets: assets.length,
      } satisfies QueryAssetsResult;
    },
    async editAssetMetadata(input) {
      const updated: AssetRecord[] = [];
      for (const target of input.targets) {
        const index = assets.findIndex((asset) => asset.key === target.key);
        if (index < 0) continue;
        const current = assets[index];
        const tags = new Set(input.patch.setTags ?? current.tags);
        for (const tag of input.patch.addTags ?? []) tags.add(tag);
        for (const tag of input.patch.removeTags ?? []) tags.delete(tag);
        const next: AssetRecord = {
          ...current,
          tags: [...tags].sort(),
          rating: input.patch.rating ?? current.rating,
          favorite: input.patch.favorite ?? current.favorite,
          note: input.patch.note ?? current.note,
          aliases: input.patch.aliases ?? current.aliases,
          sidecarPath: `${current.path}.asset.yml`,
          sidecarState: {
            schema: 1,
            digest: `demo-${Date.now()}`,
            updatedAt: new Date().toISOString(),
          },
        };
        assets[index] = next;
        updated.push(structuredClone(next));
      }
      return { updated, failures: [] } satisfies BatchMetadataEditResult;
    },
    async requestThumbnail(input) {
      const asset = assets.find(
        (candidate) => candidate.key === input.assetKey,
      );
      if (asset === undefined) throw new Error("素材不存在");
      if (
        asset.kind !== "image" ||
        asset.issues.some((issue) => issue.type === "invalid-image-metadata")
      ) {
        return {
          status: "placeholder",
          assetKey: asset.key,
          reason:
            asset.kind === "image" ? "decode-failed" : "unsupported-format",
          message:
            asset.kind === "image"
              ? "图片内容损坏，无法生成预览"
              : "此格式暂不生成缩略图",
        } satisfies ThumbnailOutcome;
      }
      const cacheKey = demoCacheKey(asset.key);
      previewKeys.set(cacheKey, asset.key);
      return {
        status: "ready",
        thumbnail: {
          assetKey: asset.key,
          cacheKey,
          mime: "image/png",
          width: Math.min(asset.dimensions?.width ?? 640, input.maxEdge),
          height: Math.min(asset.dimensions?.height ?? 480, input.maxEdge),
          sourceSize: asset.size ?? 0,
          sourceModifiedUnixMs: asset.modifiedUnixMs ?? 0,
          cacheHit: false,
          decoderVersion: "demo-preview-v1",
        },
      } satisfies ThumbnailOutcome;
    },
    async readThumbnail(cacheKey) {
      return renderDemoThumbnail(previewKeys.get(cacheKey) ?? cacheKey);
    },
    async clearThumbnailCache() {
      const removedFiles = previewKeys.size;
      previewKeys.clear();
      return { removedFiles, removedBytes: removedFiles * 4096 };
    },
  };
}

function demoRoot(): LibraryRootStatus {
  return {
    id: DEMO_ROOT_ID,
    path: "/Users/demo/Pictures/Design Archive",
    name: "Design Archive",
    enabled: true,
    scan: {
      recursive: true,
      followSymlinks: false,
      ignore: ["exports/**", ".DS_Store"],
    },
    accessStatus: "available",
  };
}

function demoAssets(): AssetRecord[] {
  const rows: Array<{
    name: string;
    tags: string[];
    dimensions?: [number, number];
    favorite?: boolean;
    rating?: number;
    issue?: AssetRecord["issues"][number];
  }> = [
    {
      name: "alpine-wayfinding.png",
      tags: ["brand/alpine", "ui/system", "color/green"],
      dimensions: [1800, 1200],
      favorite: true,
      rating: 5,
    },
    {
      name: "ceramic-study.webp",
      tags: ["reference/material", "color/neutral", "photo/still-life"],
      dimensions: [1600, 2000],
      rating: 4,
    },
    {
      name: "signal-icons.png",
      tags: ["ui/icon", "ui/system", "color/blue"],
      dimensions: [2200, 1400],
      favorite: true,
      rating: 5,
    },
    {
      name: "paper-grid.jpg",
      tags: ["texture/paper", "color/neutral", "reference/material"],
      dimensions: [2400, 1600],
    },
    {
      name: "museum-poster.jpg",
      tags: ["graphic/poster", "type/editorial", "color/red"],
      dimensions: [1400, 2000],
      favorite: true,
      rating: 4,
    },
    {
      name: "glyph-motion.gif",
      tags: ["motion/loop", "ui/icon", "color/orange"],
      dimensions: [1200, 1200],
      rating: 3,
    },
    {
      name: "quiet-interface.png",
      tags: ["ui/interface", "style/minimal", "color/neutral"],
      dimensions: [1920, 1280],
      favorite: true,
    },
    {
      name: "botanical-atlas.jpg",
      tags: ["reference/botanical", "type/editorial", "color/green"],
      dimensions: [1500, 2100],
      rating: 5,
    },
    {
      name: "modular-shapes.webp",
      tags: ["graphic/shape", "style/geometric", "color/blue"],
      dimensions: [1800, 1800],
    },
    {
      name: "night-transit.jpg",
      tags: ["photo/urban", "reference/light", "color/blue"],
      dimensions: [2200, 1467],
      favorite: true,
      rating: 4,
    },
    {
      name: "folded-type.png",
      tags: ["type/display", "graphic/poster", "color/red"],
      dimensions: [1600, 2000],
      rating: 3,
    },
    {
      name: "studio-objects.jpg",
      tags: ["photo/still-life", "reference/material", "color/orange"],
      dimensions: [1800, 1200],
    },
    {
      name: "soft-panels.webp",
      tags: ["ui/interface", "style/minimal", "color/green"],
      dimensions: [1920, 1200],
      rating: 4,
    },
    {
      name: "archive-labels.png",
      tags: ["brand/system", "type/editorial", "color/neutral"],
      dimensions: [2000, 1400],
    },
    {
      name: "coastal-map.jpg",
      tags: ["reference/map", "graphic/line", "color/blue"],
      dimensions: [1900, 1400],
      favorite: true,
    },
    {
      name: "damaged-export.png",
      tags: ["state/review", "exports/broken"],
      issue: {
        type: "invalid-image-metadata",
        message: "unexpected end of PNG stream",
      },
    },
  ];
  return rows.map((row, index) => {
    const path = `/Users/demo/Pictures/Design Archive/${row.name}`;
    const extension = row.name.split(".").at(-1) ?? "png";
    return {
      key: path,
      id: demoUuid(index + 1),
      rootId: DEMO_ROOT_ID,
      path,
      relativePath: row.name,
      sidecarPath: `${path}.asset.yml`,
      sidecarState: {
        schema: 1,
        digest: `demo-sidecar-${index}`,
        updatedAt: new Date(Date.now() - index * 3600_000).toISOString(),
      },
      fileName: row.name,
      extension,
      mime: extension === "jpg" ? "image/jpeg" : `image/${extension}`,
      kind: "image",
      size: 420_000 + index * 73_129,
      createdUnixMs: Date.now() - (index + 30) * 86_400_000,
      modifiedUnixMs: Date.now() - index * 7_200_000,
      fileReadOnly: false,
      dimensions: row.dimensions
        ? { width: row.dimensions[0], height: row.dimensions[1] }
        : null,
      nativeMetadata: null,
      tags: row.tags,
      rating: row.rating ?? 0,
      favorite: row.favorite ?? false,
      note:
        index % 5 === 0 ? "保留构图与色彩节奏，后续可作为系统视觉参考。" : "",
      aliases: [],
      issues: row.issue ? [row.issue] : [],
    } satisfies AssetRecord;
  });
}

function batchEvent(
  scanId: string,
  records: AssetRecord[],
  sequence: number,
): LibraryScanEvent {
  return {
    event: "batch",
    data: {
      scanId,
      batch: {
        sequence,
        assets: structuredClone(records),
        problems:
          sequence === 1
            ? [
                {
                  path: "damaged-export.png",
                  message: "preview metadata unavailable",
                },
              ]
            : [],
        visitedFiles: sequence === 0 ? records.length : demoAssets().length,
      },
    },
  };
}

function emptyQuery(): AssetQuery {
  return {
    allTags: [],
    anyTagGroups: [],
    excludedTags: [],
    kinds: [],
    extensions: [],
    favorite: null,
  };
}

function demoCacheKey(value: string): string {
  let hash = 2166136261;
  for (const character of value) {
    hash ^= character.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 16777619);
  }
  return Math.abs(hash).toString(16).padStart(8, "0").repeat(8).slice(0, 64);
}

async function renderDemoThumbnail(seed: string): Promise<ArrayBuffer> {
  const canvas = document.createElement("canvas");
  canvas.width = 720;
  canvas.height = 540;
  const context = canvas.getContext("2d");
  if (context === null) throw new Error("Canvas unavailable");
  const hash = [...seed].reduce(
    (value, character) => value + character.charCodeAt(0),
    0,
  );
  const hue = hash % 360;
  const gradient = context.createLinearGradient(0, 0, 720, 540);
  gradient.addColorStop(0, `hsl(${hue} 34% 82%)`);
  gradient.addColorStop(1, `hsl(${(hue + 62) % 360} 28% 48%)`);
  context.fillStyle = gradient;
  context.fillRect(0, 0, 720, 540);
  context.globalAlpha = 0.72;
  for (let index = 0; index < 5; index += 1) {
    context.fillStyle =
      index % 2 === 0 ? "#f8f4e9" : `hsl(${(hue + 140) % 360} 42% 30%)`;
    context.beginPath();
    context.roundRect(70 + index * 87, 72 + ((index * 83) % 190), 240, 92, 46);
    context.fill();
  }
  context.globalAlpha = 1;
  const blob = await new Promise<Blob>((resolve, reject) =>
    canvas.toBlob(
      (value) =>
        value === null
          ? reject(new Error("PNG preview failed"))
          : resolve(value),
      "image/png",
    ),
  );
  return blob.arrayBuffer();
}

function demoUuid(value: number): string {
  const suffix = Math.abs(value).toString(16).padStart(12, "0").slice(-12);
  return `0198a9b2-43c0-7cb0-a733-${suffix}`;
}
