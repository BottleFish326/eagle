import type { AssetQuery, QueryAssetsResult } from "./asset-query";
import type {
  ApplicationConfig,
  RuntimeRecoveryStatus,
  RuntimeResourceStatus,
} from "./application-runtime";
import type { DesktopApi } from "./desktop-api";
import type { LibraryRootStatus } from "./library-roots";
import type { BatchMetadataEditResult } from "./metadata-editor";
import type {
  ObsidianVaultStatus,
  ResolveVaultReferencesResult,
} from "./obsidian-vaults";
import type { AssetRecord, LibraryScanEvent } from "./scanner";
import type {
  SavedFilter,
  SavedFilterCatalog,
  SavedFilterFileVersion,
} from "./saved-filters";
import type {
  QuerySelectionInput,
  SelectionSort,
  SelectionSnapshotSummary,
} from "./selection-snapshots";
import type {
  AssetTraceReport,
  LibraryConsistencyReport,
} from "./support-tools";
import type { ThumbnailOutcome } from "./thumbnail";
import { matchesDemoExpression } from "./ui-model";

const DEMO_ROOT_ID = "0198a9b2-43c0-7cb0-a733-6dc58f829814";
const DEMO_VAULT_ID = "0198a9b2-43c0-7cb0-a733-6dc58f829815";
const DEFAULT_DEMO_ASSET_COUNT = 16;
const MEDIUM_DEMO_ASSET_COUNT = 10_000;

export interface DemoDesktopApiOptions {
  assetCount?: number;
}

export function createDemoDesktopApi(
  options: DemoDesktopApiOptions = {},
): DesktopApi {
  const assetCount = normalizeDemoAssetCount(options.assetCount);
  let roots: LibraryRootStatus[] = [demoRoot()];
  let assets = demoAssets(assetCount);
  let assetsByKey = new Map(assets.map((asset) => [asset.key, asset]));
  let catalogRevision = 1;
  let selectionSequence = 5_000;
  const selectionSnapshots = new Map<
    string,
    { summary: SelectionSnapshotSummary; keys: string[]; expiresUnixMs: number }
  >();
  let vaults: ObsidianVaultStatus[] = [demoVault()];
  let applicationConfig: ApplicationConfig = {
    schema: 1,
    ui: { query: "", tagFilters: {}, activeVaultId: DEMO_VAULT_ID },
  };
  let savedFilterRevision = 0;
  let savedFilters: SavedFilter[] = [];
  let savedFilterVersion: SavedFilterFileVersion = {
    exists: false,
    size: 0,
    modifiedUnixMs: null,
    sha256: null,
  };
  const recoveryStatus: RuntimeRecoveryStatus = {
    paths: {
      configDirectory: "/Users/demo/Library/Application Support/Material Eagle",
      cacheDirectory: "/Users/demo/Library/Caches/Material Eagle",
      logDirectory: "/Users/demo/Library/Logs/Material Eagle",
    },
    cacheStartup: {
      disposition: "reused",
      removedFiles: 0,
      removedBytes: 0,
    },
    cacheStats: {
      layoutVersion: 3,
      fileCount: 0,
      entryCount: 0,
      byteCount: 0,
      maxEntries: 20_000,
      maxBytes: 1_073_741_824,
      retentionDays: 30,
      decoderVersion: "demo-preview-v1",
    },
  };
  const previewKeys = new Map<string, string>();
  const previewBytes = new Map<string, Promise<ArrayBuffer>>();
  const timers = new Map<string, number[]>();
  const watchTimers = new Map<string, number>();

  const materializeSelection = (input: QuerySelectionInput): string[] => {
    if (input.expectedCatalogRevision !== catalogRevision) {
      throw {
        kind: "catalog-changed",
        message: "演示目录已变化，请刷新选择",
        actualRevision: catalogRevision,
      };
    }
    const allowed = new Set(
      roots
        .filter((root) => root.enabled && root.accessStatus === "available")
        .map((root) => root.id),
    );
    for (const rootId of input.scopeRootIds) {
      if (!allowed.has(rootId)) {
        throw { kind: "root-offline", message: "演示素材根当前不可用", rootId };
      }
    }
    return assets
      .filter(
        (asset) =>
          asset.rootId !== null &&
          input.scopeRootIds.includes(asset.rootId) &&
          matchesDemoExpression(asset, input.expression),
      )
      .sort((left, right) =>
        demoSavedFilterCompare(left, right, { sort: input.sort }),
      )
      .map((asset) => asset.key);
  };

  const createSelectionSnapshot = (keys: string[]): SelectionSnapshotSummary => {
    if (keys.length === 0) {
      throw { kind: "invalid-operation", message: "选择不能为空" };
    }
    const now = Date.now();
    const summary = {
      id: demoUuid(selectionSequence++),
      catalogRevision,
      itemCount: keys.length,
      createdAt: new Date(now).toISOString(),
      expiresAt: new Date(now + 15 * 60_000).toISOString(),
    };
    selectionSnapshots.set(summary.id, {
      summary,
      keys: [...new Set(keys)],
      expiresUnixMs: now + 15 * 60_000,
    });
    return structuredClone(summary);
  };

  const pruneSelectionSnapshots = () => {
    const now = Date.now();
    for (const [id, snapshot] of selectionSnapshots) {
      if (snapshot.expiresUnixMs <= now) selectionSnapshots.delete(id);
    }
  };

  return {
    async getApplicationConfig() {
      return structuredClone(applicationConfig);
    },
    async updateApplicationConfig(input) {
      applicationConfig = {
        schema: 1,
        ui: structuredClone(input),
      };
      return structuredClone(applicationConfig);
    },
    async getRuntimeRecoveryStatus() {
      recoveryStatus.cacheStats.entryCount = previewKeys.size;
      recoveryStatus.cacheStats.fileCount = previewKeys.size * 2;
      recoveryStatus.cacheStats.byteCount = previewKeys.size * 4096;
      return structuredClone(recoveryStatus);
    },
    async getRuntimeResourceStatus() {
      const idleWork = {
        active: 0,
        waiting: 0,
        peakActive: 0,
        peakWaiting: 0,
        completed: 0,
        rejected: 0,
        timedOut: 0,
        cancelled: 0,
      };
      const status: RuntimeResourceStatus = {
        scheduler: {
          mode:
            document.visibilityState === "hidden" ? "background" : "foreground",
          activeTotal: 0,
          waitingTotal: 0,
          peakActiveTotal: 0,
          peakWaitingTotal: 0,
          foregroundLimit: 4,
          backgroundLimit: 2,
          maxWaiters: 256,
          scan: { ...idleWork },
          hash: { ...idleWork },
          decode: { ...idleWork },
        },
        activeScans: 0,
        activeWatches: watchTimers.size,
        cache: structuredClone(recoveryStatus.cacheStats),
        scanBatchQueueCapacity: 8,
        pendingScanBatches: 0,
        maxActiveScans: 8,
        maxActiveWatches: 64,
      };
      return status;
    },
    async resetDerivedState() {
      const removedFiles = previewKeys.size;
      previewKeys.clear();
      previewBytes.clear();
      return {
        cache: { removedFiles, removedBytes: removedFiles * 4096 },
        catalogAssetsRemoved: assets.length,
      };
    },
    async exportDiagnostics() {
      return {
        path: "/Users/demo/Library/Logs/Material Eagle/diagnostics/material-eagle-diagnostic-demo.json",
        generatedAt: new Date().toISOString(),
        eventCount: 6,
        sizeBytes: 2048,
      };
    },
    async inspectLibraryConsistency() {
      const findings = assets
        .flatMap((asset) =>
          asset.issues.map((issue) => ({
            severity: "warning" as const,
            code: issue.type,
            rootId: asset.rootId,
            assetId: asset.id,
            relativePath: asset.relativePath,
            pathFingerprint: demoFingerprint(asset.key),
            message: "演示素材包含解析提示",
          })),
        )
        .slice(0, 512);
      return {
        generatedUnixMs: Date.now(),
        authoritative: true,
        summary: {
          configuredRoots: roots.length,
          catalogAssets: assets.length,
          findings: findings.length,
          warnings: findings.length,
          errors: 0,
        },
        roots: roots.map((root) => ({
          rootId: root.id,
          enabled: root.enabled,
          accessStatus: root.accessStatus,
          catalogAssets: assets.filter((asset) => asset.rootId === root.id)
            .length,
          warnings: findings.filter((finding) => finding.rootId === root.id)
            .length,
          errors: 0,
        })),
        findings,
        truncated: false,
      } satisfies LibraryConsistencyReport;
    },
    async traceAssetSupport(assetId) {
      const matched = assets.filter((asset) => asset.id === assetId);
      const report: AssetTraceReport = {
        generatedUnixMs: Date.now(),
        assetId,
        matchCount: matched.length,
        matches: matched.map((asset) => ({
          rootId: asset.rootId,
          rootAccessStatus: "available",
          relativePath: asset.relativePath,
          pathFingerprint: demoFingerprint(asset.key),
          assetPresent: true,
          sidecarPresent: asset.sidecarPath !== null,
          sidecarIdMatches: asset.sidecarPath === null ? null : true,
          mime: asset.mime,
          issueCodes: asset.issues.map((issue) => issue.type),
        })),
        steps: [
          {
            matchIndex: null,
            stage: "catalog-lookup",
            outcome: matched.length === 1 ? "passed" : "error",
            code: matched.length === 1 ? "unique-match" : "not-found",
            message: `演示目录匹配 ${String(matched.length)} 条记录`,
          },
          ...matched.flatMap((asset, index) => [
            {
              matchIndex: index,
              stage: "root-association",
              outcome: "passed" as const,
              code: "configured-root",
              message: "素材属于可用演示根目录",
            },
            {
              matchIndex: index,
              stage: "sidecar-parse",
              outcome: "passed" as const,
              code: "id-matched",
              message: "相邻 Sidecar 提供相同稳定 ID",
            },
            {
              matchIndex: index,
              stage: "parser-outcome",
              outcome:
                asset.issues.length === 0
                  ? ("passed" as const)
                  : ("warning" as const),
              code: asset.issues.length === 0 ? "clean" : "issues-present",
              message: `目录记录包含 ${String(asset.issues.length)} 个解析提示`,
            },
          ]),
        ],
      };
      return structuredClone(report);
    },
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
      assetsByKey = new Map(assets.map((asset) => [asset.key, asset]));
      catalogRevision += 1;
      return structuredClone(current);
    },
    async startLibraryScan(rootId, receive) {
      const scanId = demoUuid(Date.now() % 1000);
      const root = roots.find((candidate) => candidate.id === rootId);
      if (root === undefined) throw new Error("素材根不存在");
      const matching = assets.filter((asset) => asset.rootId === rootId);
      const batchSize = matching.length > DEFAULT_DEMO_ASSET_COUNT ? 128 : 8;
      const batches = Array.from(
        { length: Math.ceil(matching.length / batchSize) },
        (_, sequence) =>
          matching.slice(sequence * batchSize, (sequence + 1) * batchSize),
      );
      const eventTimers = [
        window.setTimeout(
          () =>
            receive({
              event: "started",
              data: { scanId, rootId, root: root.path },
            }),
          40,
        ),
        ...batches.map((batch, sequence) =>
          window.setTimeout(
            () =>
              receive(
                batchEvent(
                  scanId,
                  batch,
                  sequence,
                  matching.length,
                  Math.min((sequence + 1) * batchSize, matching.length),
                ),
              ),
            80 + sequence * 30,
          ),
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
                reconciliation: {
                  removedKeys: [],
                  movedAssets: [],
                  restoredRecords: [],
                },
              },
            }),
          120 + batches.length * 30,
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
    async inspectLibraryReconciliation(rootId) {
      return {
        rootId,
        orphanSidecars: [],
        missingAssets: [],
        pendingMoves: [],
        syncConflictCopies: [],
      };
    },
    async confirmLibraryRelink() {
      throw new Error("演示数据没有可确认的重新关联候选");
    },
    async startLibraryWatch(rootId, receive) {
      if (!roots.some((root) => root.id === rootId)) {
        throw new Error("素材根不存在");
      }
      const watchId = demoUuid((Date.now() % 1000) + 2_000);
      const timer = window.setTimeout(() => {
        watchTimers.delete(watchId);
        receive({ event: "started", data: { watchId, rootId } });
      }, 20);
      watchTimers.set(watchId, timer);
      return watchId;
    },
    async stopLibraryWatch(watchId) {
      const timer = watchTimers.get(watchId);
      if (timer !== undefined) window.clearTimeout(timer);
      watchTimers.delete(watchId);
      return timer !== undefined;
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
        catalogRevision,
      } satisfies QueryAssetsResult;
    },
    async createQuerySelectionSnapshot(input) {
      return createSelectionSnapshot(materializeSelection(input));
    },
    async createRangeSelectionSnapshot(input) {
      const keys = materializeSelection(input);
      const anchor = keys.indexOf(input.anchorKey);
      const target = keys.indexOf(input.targetKey);
      if (anchor < 0 || target < 0) {
        throw { kind: "invalid-operation", message: "范围端点已离开当前结果" };
      }
      return createSelectionSnapshot(
        keys.slice(Math.min(anchor, target), Math.max(anchor, target) + 1),
      );
    },
    async createExplicitSelectionSnapshot(input) {
      if (input.expectedCatalogRevision !== catalogRevision) {
        throw {
          kind: "catalog-changed",
          message: "演示目录已变化，请刷新选择",
          actualRevision: catalogRevision,
        };
      }
      const keys = [...new Set(input.keys)];
      if (keys.some((key) => !assetsByKey.has(key))) {
        throw { kind: "asset-missing", message: "素材已离开演示目录" };
      }
      return createSelectionSnapshot(keys);
    },
    async releaseSelectionSnapshot(snapshotId) {
      return selectionSnapshots.delete(snapshotId);
    },
    async getSelectionSessionStats() {
      pruneSelectionSnapshots();
      return {
        snapshotCount: selectionSnapshots.size,
        totalItemCount: [...selectionSnapshots.values()].reduce(
          (total, snapshot) => total + snapshot.keys.length,
          0,
        ),
        maximumSnapshotCount: 32,
        maximumItemCount: 100_000,
        maximumTotalItemCount: 200_000,
      };
    },
    async listSavedFilters() {
      return demoSavedFilterCatalog(savedFilters, savedFilterVersion, roots);
    },
    async createSavedFilter(expectedVersion, input) {
      assertDemoSavedFilterVersion(expectedVersion, savedFilterVersion);
      const duplicate = savedFilters.some(
        (filter) =>
          filter.name.normalize("NFC").toLocaleLowerCase() ===
          input.name.normalize("NFC").toLocaleLowerCase(),
      );
      if (duplicate) throw demoSavedFilterError("duplicate-name");
      const now = new Date().toISOString();
      const filter: SavedFilter = {
        id: demoUuid(savedFilterRevision + 9_000),
        ...structuredClone(input),
        createdAt: now,
        updatedAt: now,
      };
      savedFilters = [...savedFilters, filter];
      savedFilterRevision += 1;
      savedFilterVersion = nextDemoSavedFilterVersion(
        savedFilterRevision,
        savedFilters,
      );
      return {
        fileVersion: structuredClone(savedFilterVersion),
        filter: structuredClone(filter),
      };
    },
    async updateSavedFilter(expectedVersion, id, input) {
      assertDemoSavedFilterVersion(expectedVersion, savedFilterVersion);
      const current = savedFilters.find((filter) => filter.id === id);
      if (current === undefined) throw demoSavedFilterError("not-found");
      const updated: SavedFilter = {
        ...current,
        ...structuredClone(input),
        updatedAt: new Date().toISOString(),
      };
      savedFilters = savedFilters.map((filter) =>
        filter.id === id ? updated : filter,
      );
      savedFilterRevision += 1;
      savedFilterVersion = nextDemoSavedFilterVersion(
        savedFilterRevision,
        savedFilters,
      );
      return {
        fileVersion: structuredClone(savedFilterVersion),
        filter: structuredClone(updated),
      };
    },
    async renameSavedFilter(expectedVersion, id, name) {
      const current = savedFilters.find((filter) => filter.id === id);
      if (current === undefined) throw demoSavedFilterError("not-found");
      return this.updateSavedFilter(expectedVersion, id, {
        name,
        query: current.query,
        scope: current.scope,
        sort: current.sort,
      });
    },
    async deleteSavedFilter(expectedVersion, id) {
      assertDemoSavedFilterVersion(expectedVersion, savedFilterVersion);
      if (!savedFilters.some((filter) => filter.id === id)) {
        throw demoSavedFilterError("not-found");
      }
      savedFilters = savedFilters.filter((filter) => filter.id !== id);
      savedFilterRevision += 1;
      savedFilterVersion = nextDemoSavedFilterVersion(
        savedFilterRevision,
        savedFilters,
      );
      return {
        fileVersion: structuredClone(savedFilterVersion),
        filter: null,
      };
    },
    async executeSavedFilter(id) {
      const filter = savedFilters.find((candidate) => candidate.id === id);
      if (filter === undefined) throw demoSavedFilterError("not-found");
      const enabledRootIds = new Set(
        roots.filter((root) => root.enabled).map((root) => root.id),
      );
      const availableRootIds = new Set(
        roots
          .filter((root) => root.enabled && root.accessStatus === "available")
          .map((root) => root.id),
      );
      const requestedRootIds =
        filter.scope.kind === "all-enabled-roots"
          ? [...enabledRootIds]
          : filter.scope.rootIds;
      const effectiveRootIds = requestedRootIds
        .filter((rootId) => availableRootIds.has(rootId))
        .sort();
      const missingRootIds = requestedRootIds
        .filter((rootId) => !availableRootIds.has(rootId))
        .sort();
      const scoped = assets.filter(
        (asset) =>
          asset.rootId !== null && effectiveRootIds.includes(asset.rootId),
      );
      const matched = scoped.filter((asset) =>
        matchesDemoExpression(asset, filter.query),
      );
      matched.sort((left, right) =>
        demoSavedFilterCompare(left, right, filter),
      );
      return {
        filterId: filter.id,
        expression: filter.query,
        orderedKeys: matched.map((asset) => asset.key),
        totalAssets: assets.length,
        scopedAssets: scoped.length,
        matchedAssets: matched.length,
        effectiveRootIds,
        missingRootIds,
        sort: structuredClone(filter.sort),
        catalogRevision,
      };
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
            size: 0,
            modifiedUnixMs: Date.now(),
            updatedAt: new Date().toISOString(),
          },
        };
        assets[index] = next;
        assetsByKey.set(next.key, next);
        updated.push(structuredClone(next));
      }
      if (updated.length > 0) catalogRevision += 1;
      return {
        updated,
        failures: [],
        transaction: null,
        conflicts: [],
      } satisfies BatchMetadataEditResult;
    },
    async resolveMetadataConflict() {
      throw new Error("演示数据没有并发编辑冲突");
    },
    async dismissMetadataConflict() {
      return undefined;
    },
    async listMetadataTransactions() {
      return [];
    },
    async continueMetadataTransaction() {
      throw new Error("演示数据没有待继续事务");
    },
    async restoreMetadataTransaction() {
      throw new Error("演示数据没有可恢复事务");
    },
    async dismissMetadataTransaction() {
      return undefined;
    },
    async requestThumbnail(input) {
      const asset = assetsByKey.get(input.assetKey);
      if (asset === undefined) throw new Error("素材不存在");
      if (
        asset.kind !== "image" ||
        asset.issues.some((issue) => issue.type === "invalid-image-metadata")
      ) {
        return {
          status: "placeholder",
          assetKey: asset.key,
          reason:
            asset.kind === "image" ? "invalid-content" : "preview-unavailable",
          message:
            asset.kind === "image"
              ? "图片内容损坏，无法生成预览"
              : "此格式暂不生成缩略图",
        } satisfies ThumbnailOutcome;
      }
      const cacheKey = demoCacheKey(asset.key);
      const variant = demoThumbnailVariant(asset);
      const cacheHit = previewBytes.has(variant);
      previewKeys.set(cacheKey, variant);
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
          cacheHit,
          providerId: "demo-raster",
          providerVersion: "demo-preview-v1",
          decoderVersion: "demo-preview-v1",
        },
      } satisfies ThumbnailOutcome;
    },
    async readThumbnail(cacheKey) {
      const variant = previewKeys.get(cacheKey) ?? cacheKey;
      let bytes = previewBytes.get(variant);
      if (bytes === undefined) {
        bytes = renderDemoThumbnail(variant);
        previewBytes.set(variant, bytes);
      }
      return bytes;
    },
    async clearThumbnailCache() {
      const removedFiles = previewKeys.size;
      previewKeys.clear();
      previewBytes.clear();
      return { removedFiles, removedBytes: removedFiles * 4096 };
    },
    async maintainThumbnailCache() {
      recoveryStatus.cacheStats.entryCount = previewKeys.size;
      recoveryStatus.cacheStats.fileCount = previewKeys.size * 2;
      recoveryStatus.cacheStats.byteCount = previewKeys.size * 4096;
      return {
        removedEntries: 0,
        removedFiles: 0,
        removedBytes: 0,
        incompatibleEntries: 0,
        orphanEntries: 0,
        expiredEntries: 0,
        capacityEntries: 0,
        stats: structuredClone(recoveryStatus.cacheStats),
      };
    },
    async listObsidianVaults() {
      return structuredClone(vaults);
    },
    async addObsidianVault(input) {
      const vault: ObsidianVaultStatus = {
        id: demoUuid(vaults.length + 70),
        path: input.path,
        name: input.name,
        enabled: true,
        accessStatus: "available",
      };
      vaults = [...vaults, vault];
      return structuredClone(vault);
    },
    async updateObsidianVault(input) {
      const current = vaults.find((vault) => vault.id === input.id);
      if (current === undefined) throw new Error("Vault 不存在");
      const updated: ObsidianVaultStatus = {
        ...current,
        name: input.name ?? current.name,
        enabled: input.enabled ?? current.enabled,
      };
      vaults = vaults.map((vault) =>
        vault.id === updated.id ? updated : vault,
      );
      return structuredClone(updated);
    },
    async removeObsidianVault(id) {
      const current = vaults.find((vault) => vault.id === id);
      if (current === undefined) throw new Error("Vault 不存在");
      vaults = vaults.filter((vault) => vault.id !== id);
      return structuredClone(current);
    },
    async resolveObsidianVaultReferences(input) {
      const vault = vaults.find((candidate) => candidate.id === input.vaultId);
      if (vault === undefined) throw new Error("Vault 不存在");
      const result: ResolveVaultReferencesResult = {
        resolved: [],
        failures: [],
      };
      for (const assetKey of [...new Set(input.assetKeys)]) {
        const asset = assetsByKey.get(assetKey);
        if (asset === undefined) {
          result.failures.push({
            assetKey,
            kind: "asset-not-found",
            message: "素材不存在",
          });
          continue;
        }
        const prefix = `${vault.path.replace(/\/$/u, "")}/`;
        if (!vault.enabled || vault.accessStatus !== "available") {
          result.failures.push({
            assetKey,
            kind: vault.enabled ? "vault-unavailable" : "vault-disabled",
            message: "Vault 当前不可用",
          });
          continue;
        }
        if (!asset.path.startsWith(prefix)) {
          result.failures.push({
            assetKey,
            kind: "outside-vault",
            message: "素材位于 Vault 外",
          });
          continue;
        }
        const relativePath = asset.path.slice(prefix.length);
        result.resolved.push({
          assetKey,
          vaultId: vault.id,
          vaultName: vault.name,
          assetPath: asset.path,
          relativePath,
          urlEncodedPath: relativePath
            .split("/")
            .map((component) => encodeURIComponent(component))
            .join("/"),
          markdown: `![[${relativePath}]]`,
        });
      }
      return structuredClone(result);
    },
  };
}

function demoVault(): ObsidianVaultStatus {
  return {
    id: DEMO_VAULT_ID,
    path: "/Users/demo/Pictures/Design Archive",
    name: "Design Notes",
    enabled: true,
    accessStatus: "available",
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

function demoAssets(count = DEFAULT_DEMO_ASSET_COUNT): AssetRecord[] {
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
  const now = Date.now();
  return Array.from({ length: count }, (_, index) => {
    const row = rows[index % rows.length];
    const extension = row.name.split(".").at(-1) ?? "png";
    const stem = row.name.slice(0, -(extension.length + 1));
    const fileName =
      count === rows.length
        ? row.name
        : `${stem}-${index.toString().padStart(5, "0")}.${extension}`;
    const relativePath =
      count === rows.length
        ? fileName
        : `group-${Math.floor(index / 500)
            .toString()
            .padStart(3, "0")}/${fileName}`;
    const path = `/Users/demo/Pictures/Design Archive/${relativePath}`;
    return {
      key: path,
      id: demoUuid(index + 1),
      rootId: DEMO_ROOT_ID,
      path,
      relativePath,
      sidecarPath: `${path}.asset.yml`,
      sidecarState: {
        schema: 1,
        digest: `demo-sidecar-${index}`,
        size: 0,
        modifiedUnixMs: now - index * 7_200_000,
        updatedAt: new Date(Date.now() - index * 3600_000).toISOString(),
      },
      fileName,
      extension,
      mime: extension === "jpg" ? "image/jpeg" : `image/${extension}`,
      kind: "image",
      size: 420_000 + index * 73_129,
      createdUnixMs: now - (index + 30) * 86_400_000,
      modifiedUnixMs: now - index * 7_200_000,
      fileReadOnly: false,
      dimensions: row.dimensions
        ? { width: row.dimensions[0], height: row.dimensions[1] }
        : null,
      nativeMetadata: null,
      media: null,
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

function demoFingerprint(value: string): string {
  let hash = 2166136261;
  for (const character of value) {
    hash ^= character.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 16777619);
  }
  return Math.abs(hash).toString(16).padStart(16, "0").slice(0, 16);
}

function batchEvent(
  scanId: string,
  records: AssetRecord[],
  sequence: number,
  total: number,
  visitedFiles: number,
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
        visitedFiles: Math.min(visitedFiles, total),
      },
    },
  };
}

function normalizeDemoAssetCount(value: number | undefined): number {
  return value === MEDIUM_DEMO_ASSET_COUNT
    ? MEDIUM_DEMO_ASSET_COUNT
    : DEFAULT_DEMO_ASSET_COUNT;
}

export function demoAssetCountFromSearch(
  search: string,
  development: boolean,
): number {
  if (!development) return DEFAULT_DEMO_ASSET_COUNT;
  return new URLSearchParams(search).get("demoDataset") === "medium"
    ? MEDIUM_DEMO_ASSET_COUNT
    : DEFAULT_DEMO_ASSET_COUNT;
}

function emptyQuery(): AssetQuery {
  return {
    allTags: [],
    anyTagGroups: [],
    excludedTags: [],
    kinds: [],
    extensions: [],
    favorite: null,
    integerRanges: {},
    instantRanges: {},
    ratioRanges: {},
    unknownFields: [],
    orientations: [],
    rootIds: [],
    pathContains: [],
    colorSpaces: [],
    hasNote: null,
    hasAlpha: null,
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

function demoThumbnailVariant(asset: AssetRecord): string {
  return asset.tags.join("|");
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

function demoSavedFilterCatalog(
  filters: SavedFilter[],
  fileVersion: SavedFilterFileVersion,
  roots: LibraryRootStatus[],
): SavedFilterCatalog {
  const availableRootIds = new Set(
    roots
      .filter((root) => root.enabled && root.accessStatus === "available")
      .map((root) => root.id),
  );
  const validFilters: SavedFilter[] = [];
  const unavailableFilters: SavedFilterCatalog["unavailableFilters"] = [];
  for (const filter of filters) {
    const requested =
      filter.scope.kind === "selected-roots" ? filter.scope.rootIds : [];
    const missingRootIds = requested.filter(
      (rootId) => !availableRootIds.has(rootId),
    );
    if (missingRootIds.length === 0) validFilters.push(filter);
    else unavailableFilters.push({ filter, missingRootIds });
  }
  return structuredClone({
    fileVersion,
    validFilters,
    unavailableFilters,
    invalidEntries: [],
    fileIssues: [],
  });
}

function assertDemoSavedFilterVersion(
  expected: SavedFilterFileVersion,
  actual: SavedFilterFileVersion,
): void {
  if (JSON.stringify(expected) === JSON.stringify(actual)) return;
  throw {
    kind: "external-change",
    message: "保存过滤器已被外部修改，请刷新后重试",
    actualVersion: structuredClone(actual),
    queryKind: null,
    queryOffset: null,
  };
}

function demoSavedFilterError(kind: "duplicate-name" | "not-found") {
  return {
    kind,
    message: kind === "not-found" ? "保存过滤器不存在" : "名称已经存在",
    actualVersion: null,
    queryKind: null,
    queryOffset: null,
  };
}

function nextDemoSavedFilterVersion(
  revision: number,
  filters: SavedFilter[],
): SavedFilterFileVersion {
  const serialized = JSON.stringify(filters);
  return {
    exists: true,
    size: new TextEncoder().encode(serialized).byteLength,
    modifiedUnixMs: Date.now(),
    sha256: demoCacheKey(`${String(revision)}:${serialized}`),
  };
}

function demoSavedFilterCompare(
  left: AssetRecord,
  right: AssetRecord,
  filter: Pick<SavedFilter, "sort"> | { sort: SelectionSort },
): number {
  const value = (asset: AssetRecord): string | number | null => {
    switch (filter.sort.field) {
      case "asset-key":
        return asset.key.normalize("NFC");
      case "file-name":
        return asset.fileName.normalize("NFC");
      case "modified-at":
        return asset.modifiedUnixMs;
      case "created-at":
        return asset.createdUnixMs;
      case "file-size":
        return asset.size;
      case "rating":
        return asset.rating;
      case "asset-kind":
        return ["image", "video", "audio", "pdf", "other"].indexOf(asset.kind);
    }
  };
  const leftValue = value(left);
  const rightValue = value(right);
  let order = 0;
  if (leftValue === null) order = rightValue === null ? 0 : 1;
  else if (rightValue === null) order = -1;
  else if (leftValue < rightValue) order = -1;
  else if (leftValue > rightValue) order = 1;
  if (
    leftValue !== null &&
    rightValue !== null &&
    order !== 0 &&
    filter.sort.direction === "descending"
  ) {
    order *= -1;
  }
  return order === 0 ? left.key.localeCompare(right.key) : order;
}
