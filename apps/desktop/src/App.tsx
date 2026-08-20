import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  AdvancedFilterBuilder,
  appendAdvancedPredicate,
} from "./AdvancedFilterBuilder";
import { AssetGrid, type AssetSelectionIntent } from "./AssetGrid";
import {
  copyLocalPath,
  type ApplicationConfig,
  type DerivedStateResetReport,
  type DiagnosticExportReport,
  type RuntimeRecoveryStatus,
  type SavedTagFilterState,
} from "./application-runtime";
import { type BuildInfo, loadBuildInfo } from "./build-info";
import {
  preflightConfirmation,
  type BatchExecutionProgress,
  type BatchPreflightSummary,
} from "./batch-workflows";
import { createDemoDesktopApi, demoAssetCountFromSearch } from "./demo-api";
import {
  type DesktopApi,
  isTauriRuntime,
  tauriDesktopApi,
} from "./desktop-api";
import { Icon } from "./Icon";
import { Inspector } from "./Inspector";
import {
  markLibraryRootAccessFailure,
  type LibraryRootStatus,
} from "./library-roots";
import type { MetadataPatch } from "./metadata-editor";
import type {
  MetadataConflict,
  MetadataConflictResolution,
} from "./metadata-conflicts";
import type { MetadataTransactionSummary } from "./metadata-transactions";
import {
  copyVaultReference,
  referenceResolutionKeys,
  type ObsidianVaultStatus,
  type VaultReference,
  type VaultReferenceFailure,
} from "./obsidian-vaults";
import { RootManager } from "./RootManager";
import { SavedFilterManager } from "./SavedFilterManager";
import type {
  SavedFilter,
  SavedFilterCatalog,
  SavedFilterCommandError,
  SavedFilterInput,
} from "./saved-filters";
import type {
  QuerySelectionInput,
  SelectionSnapshotSummary,
} from "./selection-snapshots";
import type { ReconciliationReport, RelinkCandidate } from "./reconciliation";
import { SettingsManager } from "./SettingsManager";
import type { AssetRecord, LibraryScanEvent } from "./scanner";
import type {
  AssetTraceReport,
  LibraryConsistencyReport,
} from "./support-tools";
import type { CacheMaintenanceReport } from "./thumbnail";
import {
  composeAssetQuery,
  cycleTagFilter,
  reconcileSelectedKeys,
  reconcileSelectionAnchor,
  removeRootAssets,
  settleFailedQuery,
  settleSuccessfulQuery,
  summarizeTags,
  type QueryViewState,
  type TagFilterMap,
  type TagFilterState,
  upsertAssets,
} from "./ui-model";
import { VaultManager } from "./VaultManager";

const defaultApi = isTauriRuntime()
  ? tauriDesktopApi
  : createDemoDesktopApi({
      assetCount: demoAssetCountFromSearch(
        typeof window === "undefined" ? "" : window.location.search,
        import.meta.env.DEV,
      ),
    });

type ScanPhase = "starting" | "scanning" | "completed" | "cancelled" | "failed";

interface ScanUiState {
  scanId?: string;
  phase: ScanPhase;
  visitedFiles: number;
  assetCount: number;
  problemCount: number;
  message?: string;
}

interface Notice {
  tone: "info" | "error";
  message: string;
}

const EMPTY_SAVED_FILTER_CATALOG: SavedFilterCatalog = {
  fileVersion: {
    exists: false,
    size: 0,
    modifiedUnixMs: null,
    sha256: null,
  },
  validFilters: [],
  unavailableFilters: [],
  invalidEntries: [],
  fileIssues: [],
};

export function App({ api = defaultApi }: { api?: DesktopApi }) {
  const search = useRef<HTMLInputElement>(null);
  const activeScanRoots = useRef<Set<string>>(new Set());
  const rootsById = useRef<Map<string, LibraryRootStatus>>(new Map());
  const watchIds = useRef<Map<string, string | null>>(new Map());
  const watchRescanTimers = useRef<Map<string, number>>(new Map());
  const appMounted = useRef(true);
  const [buildInfo, setBuildInfo] = useState<BuildInfo>();
  const [roots, setRoots] = useState<LibraryRootStatus[]>([]);
  const [vaults, setVaults] = useState<ObsidianVaultStatus[]>([]);
  const [activeVaultId, setActiveVaultId] = useState<string>();
  const [assets, setAssets] = useState<Map<string, AssetRecord>>(
    () => new Map(),
  );
  const [queryView, setQueryView] = useState<QueryViewState>(() => ({
    visibleKeys: [] as string[],
  }));
  const visibleKeys = queryView.visibleKeys;
  const queryError = queryView.error;
  const [gridWindowKeys, setGridWindowKeys] = useState<string[]>([]);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [selectionAnchor, setSelectionAnchor] = useState<string>();
  const selectionSnapshotRef = useRef<SelectionSnapshotSummary | undefined>(
    undefined,
  );
  const [selectionSnapshot, setSelectionSnapshot] =
    useState<SelectionSnapshotSummary>();
  const [selectionContext, setSelectionContext] =
    useState<QuerySelectionInput>();
  const [expression, setExpression] = useState("");
  const [tagFilters, setTagFilters] = useState<TagFilterMap>({});
  const [queryPending, setQueryPending] = useState(false);
  const [scans, setScans] = useState<Record<string, ScanUiState>>({});
  const [reconciliationReports, setReconciliationReports] = useState<
    Record<string, ReconciliationReport>
  >({});
  const [relinkBusy, setRelinkBusy] = useState<string>();
  const [rootManagerOpen, setRootManagerOpen] = useState(false);
  const [savedFilterManagerOpen, setSavedFilterManagerOpen] = useState(false);
  const [savedFilters, setSavedFilters] = useState<SavedFilterCatalog>(
    EMPTY_SAVED_FILTER_CATALOG,
  );
  const [activeSavedFilter, setActiveSavedFilter] = useState<{
    id: string;
    expression: string;
  }>();
  const [savedFilterBusy, setSavedFilterBusy] = useState(false);
  const [vaultManagerOpen, setVaultManagerOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [rootBusy, setRootBusy] = useState(false);
  const [vaultBusy, setVaultBusy] = useState(false);
  const [resetBusy, setResetBusy] = useState(false);
  const [cacheMaintenanceBusy, setCacheMaintenanceBusy] = useState(false);
  const [diagnosticBusy, setDiagnosticBusy] = useState(false);
  const [supportBusy, setSupportBusy] = useState<"consistency" | "trace">();
  const [metadataTransactions, setMetadataTransactions] = useState<
    MetadataTransactionSummary[]
  >([]);
  const [transactionBusy, setTransactionBusy] = useState<string>();
  const [metadataConflicts, setMetadataConflicts] = useState<
    MetadataConflict[]
  >([]);
  const [metadataConflictBusy, setMetadataConflictBusy] = useState<string>();
  const [vaultReferences, setVaultReferences] = useState<
    Map<string, VaultReference>
  >(() => new Map());
  const [vaultReferenceFailures, setVaultReferenceFailures] = useState<
    Map<string, VaultReferenceFailure>
  >(() => new Map());
  const [vaultReferencesPending, setVaultReferencesPending] = useState(false);
  const [editBusy, setEditBusy] = useState(false);
  const [pendingBatch, setPendingBatch] = useState<BatchPreflightSummary>();
  const [batchProgress, setBatchProgress] = useState<BatchExecutionProgress>();
  const [batchExecutionOperation, setBatchExecutionOperation] = useState<string>();
  const [applicationConfig, setApplicationConfig] =
    useState<ApplicationConfig>();
  const [recoveryStatus, setRecoveryStatus] = useState<RuntimeRecoveryStatus>();
  const [resetReport, setResetReport] = useState<DerivedStateResetReport>();
  const [cacheMaintenanceReport, setCacheMaintenanceReport] =
    useState<CacheMaintenanceReport>();
  const [diagnosticReport, setDiagnosticReport] =
    useState<DiagnosticExportReport>();
  const [consistencyReport, setConsistencyReport] =
    useState<LibraryConsistencyReport>();
  const [assetTraceReport, setAssetTraceReport] = useState<AssetTraceReport>();
  const [preferencesReady, setPreferencesReady] = useState(false);
  const [booting, setBooting] = useState(true);
  const [notice, setNotice] = useState<Notice>();

  const runScan = useCallback(
    async (root: LibraryRootStatus) => {
      activeScanRoots.current.add(root.id);
      setScans((current) => ({
        ...current,
        [root.id]: {
          phase: "starting",
          visitedFiles: 0,
          assetCount: 0,
          problemCount: 0,
        },
      }));
      const receive = (event: LibraryScanEvent) => {
        handleScanEvent(
          root.id,
          event,
          setAssets,
          setScans,
          setSelected,
          setSelectionAnchor,
        );
        if (event.event === "finished" || event.event === "failed") {
          activeScanRoots.current.delete(root.id);
        }
        if (event.event === "failed" && event.data.rootAccessStatus !== null) {
          const accessStatus = event.data.rootAccessStatus;
          setRoots((current) =>
            markLibraryRootAccessFailure(
              current,
              root.id,
              accessStatus,
              event.data.message,
            ),
          );
          setNotice({
            tone: "error",
            message: `${root.name} 已离线或无权限；本次扫描结果已放弃，原有素材视图已保留。`,
          });
        }
        if (
          event.event === "finished" &&
          event.data.summary.completion === "completed"
        ) {
          void api
            .inspectLibraryReconciliation(root.id)
            .then((report) =>
              setReconciliationReports((current) => ({
                ...current,
                [root.id]: report,
              })),
            )
            .catch((error: unknown) =>
              setNotice({
                tone: "error",
                message: `移动诊断失败：${errorMessage(error)}`,
              }),
            );
        }
      };
      try {
        const scanId = await api.startLibraryScan(root.id, receive);
        setScans((current) => ({
          ...current,
          [root.id]: {
            ...(current[root.id] ?? {
              phase: "scanning",
              visitedFiles: 0,
              assetCount: 0,
              problemCount: 0,
            }),
            scanId,
          },
        }));
      } catch (error) {
        activeScanRoots.current.delete(root.id);
        setScans((current) => ({
          ...current,
          [root.id]: {
            phase: "failed",
            visitedFiles: 0,
            assetCount: 0,
            problemCount: 0,
            message: errorMessage(error),
          },
        }));
        setNotice({
          tone: "error",
          message: `无法扫描 ${root.name}：${errorMessage(error)}`,
        });
        void api
          .listLibraryRoots()
          .then(setRoots)
          .catch(() => undefined);
      }
    },
    [api],
  );

  const confirmRelink = useCallback(
    async (candidate: RelinkCandidate) => {
      const root = rootsById.current.get(candidate.rootId);
      if (root === undefined) return;
      setRelinkBusy(candidate.candidateId);
      try {
        await api.confirmLibraryRelink(candidate.candidateId);
        setNotice({
          tone: "info",
          message: "已按确认计划移动 Sidecar；正在重新解释素材身份。",
        });
        await runScan(root);
      } catch (error) {
        setNotice({
          tone: "error",
          message: `重新关联失败：${errorMessage(error)}`,
        });
      } finally {
        setRelinkBusy(undefined);
      }
    },
    [api, runScan],
  );

  const scheduleWatchRescan = useCallback(
    (rootId: string) => {
      const existing = watchRescanTimers.current.get(rootId);
      if (existing !== undefined) window.clearTimeout(existing);
      const attempt = () => {
        watchRescanTimers.current.delete(rootId);
        const root = rootsById.current.get(rootId);
        if (
          !appMounted.current ||
          root === undefined ||
          !root.enabled ||
          root.accessStatus !== "available"
        )
          return;
        if (activeScanRoots.current.has(rootId)) {
          watchRescanTimers.current.set(
            rootId,
            window.setTimeout(attempt, 500),
          );
          return;
        }
        void runScan(root);
      };
      watchRescanTimers.current.set(rootId, window.setTimeout(attempt, 350));
    },
    [runScan],
  );

  useEffect(() => {
    let active = true;
    void loadBuildInfo().then((value) => {
      if (active) setBuildInfo(value);
    });
    void Promise.all([
      api.getApplicationConfig(),
      api.listLibraryRoots(),
      api.listObsidianVaults(),
      api.getRuntimeRecoveryStatus(),
      api.listMetadataTransactions(),
      api
        .listSavedFilters()
        .then((value) => ({ value, error: undefined }))
        .catch((error: unknown) => ({ value: undefined, error })),
    ])
      .then(
        ([config, nextRoots, nextVaults, recovery, transactions, filters]) => {
          if (!active) return;
          const preferredVault = nextVaults.find(
            (vault) =>
              vault.id === config.ui.activeVaultId &&
              vault.enabled &&
              vault.accessStatus === "available",
          );
          const fallbackVault = nextVaults.find(
            (vault) => vault.enabled && vault.accessStatus === "available",
          );
          setApplicationConfig(config);
          setExpression(config.ui.query);
          setTagFilters(config.ui.tagFilters);
          setActiveVaultId(preferredVault?.id ?? fallbackVault?.id);
          setRoots(nextRoots);
          setVaults(nextVaults);
          setRecoveryStatus(recovery);
          setMetadataTransactions(transactions);
          if (filters.value !== undefined) setSavedFilters(filters.value);
          const recoverableCount = transactions.filter(
            (transaction) =>
              transaction.state === "active" ||
              transaction.state === "conflict",
          ).length;
          if (recoverableCount > 0) {
            setNotice({
              tone: "info",
              message: `检测到 ${recoverableCount} 个待处理的批量事务，请在“设置与恢复”中选择继续或安全恢复。`,
            });
          } else if (filters.error !== undefined) {
            setNotice({
              tone: "error",
              message: `保存过滤器读取失败，素材库仍可正常使用：${errorMessage(filters.error)}`,
            });
          }
          setPreferencesReady(true);
          setBooting(false);
          for (const root of nextRoots) {
            if (root.enabled && root.accessStatus === "available")
              void runScan(root);
          }
        },
      )
      .catch((error: unknown) => {
        if (!active) return;
        setBooting(false);
        setNotice({
          tone: "error",
          message: `应用启动状态读取失败：${errorMessage(error)}`,
        });
      });
    return () => {
      active = false;
    };
  }, [api, runScan]);

  useEffect(() => {
    rootsById.current = new Map(roots.map((root) => [root.id, root]));
    const desired = new Set(
      roots
        .filter((root) => root.enabled && root.accessStatus === "available")
        .map((root) => root.id),
    );
    for (const [rootId, watchId] of watchIds.current) {
      if (desired.has(rootId)) continue;
      watchIds.current.delete(rootId);
      if (watchId !== null) void api.stopLibraryWatch(watchId);
      const timer = watchRescanTimers.current.get(rootId);
      if (timer !== undefined) window.clearTimeout(timer);
      watchRescanTimers.current.delete(rootId);
    }
    for (const root of roots) {
      if (!desired.has(root.id) || watchIds.current.has(root.id)) continue;
      watchIds.current.set(root.id, null);
      void api
        .startLibraryWatch(root.id, (event) => {
          if (!appMounted.current) return;
          if (event.event === "changes") {
            scheduleWatchRescan(event.data.rootId);
          } else if (event.event === "failed") {
            watchIds.current.delete(event.data.rootId);
            if (event.data.rootAccessStatus !== null) {
              const accessStatus = event.data.rootAccessStatus;
              setRoots((current) =>
                markLibraryRootAccessFailure(
                  current,
                  event.data.rootId,
                  accessStatus,
                  event.data.message,
                ),
              );
            }
            setNotice({
              tone: "error",
              message: `${root.name} 的文件监听已停止：${event.data.message}`,
            });
          } else if (event.event === "stopped") {
            if (
              watchIds.current.get(event.data.rootId) === event.data.watchId
            ) {
              watchIds.current.delete(event.data.rootId);
            }
          }
        })
        .then((watchId) => {
          const current = rootsById.current.get(root.id);
          if (
            !appMounted.current ||
            current === undefined ||
            !current.enabled ||
            current.accessStatus !== "available" ||
            watchIds.current.get(root.id) !== null
          ) {
            watchIds.current.delete(root.id);
            void api.stopLibraryWatch(watchId);
            return;
          }
          watchIds.current.set(root.id, watchId);
        })
        .catch((error: unknown) => {
          watchIds.current.delete(root.id);
          if (!appMounted.current) return;
          setNotice({
            tone: "error",
            message: `无法监听 ${root.name}：${errorMessage(error)}`,
          });
        });
    }
  }, [api, roots, scheduleWatchRescan]);

  useEffect(() => {
    appMounted.current = true;
    return () => {
      appMounted.current = false;
      for (const watchId of watchIds.current.values()) {
        if (watchId !== null) void api.stopLibraryWatch(watchId);
      }
      watchIds.current.clear();
      for (const timer of watchRescanTimers.current.values()) {
        window.clearTimeout(timer);
      }
      watchRescanTimers.current.clear();
    };
  }, [api]);

  useEffect(() => {
    if (!preferencesReady) return;
    let active = true;
    const timer = window.setTimeout(() => {
      const savedTagFilters = Object.fromEntries(
        Object.entries(tagFilters).filter(
          (entry): entry is [string, SavedTagFilterState] =>
            entry[1] === "include" || entry[1] === "exclude",
        ),
      );
      void api
        .updateApplicationConfig({
          query: expression,
          tagFilters: savedTagFilters,
          activeVaultId: activeVaultId ?? null,
        })
        .then((config) => {
          if (active) setApplicationConfig(config);
        })
        .catch((error: unknown) => {
          if (!active) return;
          setNotice({
            tone: "error",
            message: `视图偏好保存失败：${errorMessage(error)}`,
          });
        });
    }, 500);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [activeVaultId, api, expression, preferencesReady, tagFilters]);

  const effectiveQuery = useMemo(
    () => composeAssetQuery(expression, tagFilters),
    [expression, tagFilters],
  );

  useEffect(() => {
    let active = true;
    const timer = window.setTimeout(() => {
      setQueryPending(true);
      const savedExecution =
        activeSavedFilter !== undefined &&
        activeSavedFilter.expression === effectiveQuery
          ? api.executeSavedFilter(activeSavedFilter.id)
          : undefined;
      if (
        activeSavedFilter !== undefined &&
        activeSavedFilter.expression !== effectiveQuery
      ) {
        setActiveSavedFilter(undefined);
      }
      void (savedExecution ?? api.queryAssets({ expression: effectiveQuery }))
        .then((result) => {
          if (!active) return;
          const keys =
            "orderedKeys" in result ? result.orderedKeys : result.keys;
          setSelectionContext({
            expectedCatalogRevision: result.catalogRevision,
            expression: effectiveQuery,
            scopeRootIds:
              "effectiveRootIds" in result
                ? result.effectiveRootIds
                : roots
                    .filter(
                      (root) =>
                        root.enabled && root.accessStatus === "available",
                    )
                    .map((root) => root.id),
            sort:
              "sort" in result
                ? result.sort
                : { field: "asset-key", direction: "ascending" },
          });
          setQueryView(
            settleSuccessfulQuery(keys.filter((key) => assets.has(key))),
          );
        })
        .catch((error: unknown) => {
          if (!active) return;
          setQueryView((current) => settleFailedQuery(current, error));
        })
        .finally(() => {
          if (active) setQueryPending(false);
        });
    }, 120);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [activeSavedFilter, api, assets, effectiveQuery, roots]);

  const allAssets = useMemo(() => [...assets.values()], [assets]);
  const visibleAssets = useMemo(
    () =>
      visibleKeys.flatMap((key) => (assets.get(key) ? [assets.get(key)!] : [])),
    [assets, visibleKeys],
  );
  const selectedAssets = useMemo(
    () =>
      [...selected].flatMap((key) =>
        assets.get(key) ? [assets.get(key)!] : [],
      ),
    [assets, selected],
  );
  const vaultReferenceKeys = useMemo(
    () => referenceResolutionKeys(gridWindowKeys, selected),
    [gridWindowKeys, selected],
  );
  const tags = useMemo(
    () => summarizeTags(allAssets, tagFilters),
    [allAssets, tagFilters],
  );
  const activeTagFilters = tags.filter((tag) => tag.state !== "neutral");
  const activeScans = Object.entries(scans).filter(
    ([, scan]) => scan.phase === "starting" || scan.phase === "scanning",
  );
  const failedScans = Object.entries(scans).filter(
    ([, scan]) => scan.phase === "failed",
  );
  const inaccessibleRoots = roots.filter(
    (root) => root.enabled && root.accessStatus !== "available",
  );
  const activeVault = vaults.find((vault) => vault.id === activeVaultId);
  const displayedApplicationConfig = useMemo<ApplicationConfig | undefined>(
    () =>
      applicationConfig
        ? {
            ...applicationConfig,
            ui: {
              query: expression,
              tagFilters: Object.fromEntries(
                Object.entries(tagFilters).filter(
                  (entry): entry is [string, SavedTagFilterState] =>
                    entry[1] === "include" || entry[1] === "exclude",
                ),
              ),
              activeVaultId: activeVaultId ?? null,
            },
          }
        : undefined,
    [activeVaultId, applicationConfig, expression, tagFilters],
  );

  useEffect(() => {
    let active = true;
    if (activeVaultId === undefined || vaultReferenceKeys.length === 0) {
      setVaultReferences(new Map());
      setVaultReferenceFailures(new Map());
      setVaultReferencesPending(false);
      return () => {
        active = false;
      };
    }
    const timer = window.setTimeout(() => {
      setVaultReferencesPending(true);
      void api
        .resolveObsidianVaultReferences({
          vaultId: activeVaultId,
          assetKeys: vaultReferenceKeys,
        })
        .then((result) => {
          if (!active) return;
          setVaultReferences(
            new Map(
              result.resolved.map((reference) => [
                reference.assetKey,
                reference,
              ]),
            ),
          );
          setVaultReferenceFailures(
            new Map(
              result.failures.map((failure) => [failure.assetKey, failure]),
            ),
          );
        })
        .catch((error: unknown) => {
          if (!active) return;
          setVaultReferences(new Map());
          setVaultReferenceFailures(new Map());
          setNotice({
            tone: "error",
            message: `Obsidian 引用解析失败：${errorMessage(error)}`,
          });
        })
        .finally(() => {
          if (active) setVaultReferencesPending(false);
        });
    }, 80);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [activeVaultId, api, vaultReferenceKeys]);

  useEffect(() => {
    setSelected(
      (current) => new Set([...current].filter((key) => assets.has(key))),
    );
  }, [assets]);

  useEffect(() => {
    const visible = new Set(visibleKeys);
    setSelected((current) => {
      const retained = new Set([...current].filter((key) => visible.has(key)));
      return retained.size === current.size ? current : retained;
    });
    setSelectionAnchor((current) =>
      current && visible.has(current) ? current : undefined,
    );
  }, [visibleKeys]);

  useEffect(() => {
    selectionSnapshotRef.current = selectionSnapshot;
  }, [selectionSnapshot]);

  useEffect(
    () => () => {
      const snapshot = selectionSnapshotRef.current;
      if (snapshot !== undefined) void api.releaseSelectionSnapshot(snapshot.id);
    },
    [api],
  );

  const replaceSelectionSnapshot = async (
    next: SelectionSnapshotSummary | undefined,
  ) => {
    const previous = selectionSnapshotRef.current;
    selectionSnapshotRef.current = next;
    setSelectionSnapshot(next);
    if (previous !== undefined && previous.id !== next?.id) {
      await api.releaseSelectionSnapshot(previous.id).catch(() => false);
    }
  };

  useEffect(() => {
    const handleGlobalKey = (event: globalThis.KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const editing =
        target?.matches("input, textarea, [contenteditable=true]") ?? false;
      if (event.key === "/" && !editing) {
        event.preventDefault();
        search.current?.focus();
      } else if (event.key === "Escape") {
        if (settingsOpen) setSettingsOpen(false);
        else if (vaultManagerOpen) setVaultManagerOpen(false);
        else if (rootManagerOpen) setRootManagerOpen(false);
        else if (savedFilterManagerOpen) setSavedFilterManagerOpen(false);
        else if (!editing) {
          setSelected(new Set());
          void replaceSelectionSnapshot(undefined);
        }
      }
    };
    window.addEventListener("keydown", handleGlobalKey);
    return () => window.removeEventListener("keydown", handleGlobalKey);
  }, [rootManagerOpen, savedFilterManagerOpen, settingsOpen, vaultManagerOpen]);

  const selectAsset = (key: string, intent: AssetSelectionIntent) => {
    if (intent.range && selectionAnchor && selectionContext !== undefined) {
      const keys = visibleAssets.map((asset) => asset.key);
      const start = keys.indexOf(selectionAnchor);
      const end = keys.indexOf(key);
      if (start >= 0 && end >= 0) {
        void api
          .createRangeSelectionSnapshot({
            ...selectionContext,
            anchorKey: selectionAnchor,
            targetKey: key,
          })
          .then(async (snapshot) => {
            await replaceSelectionSnapshot(snapshot);
            setSelected(
              new Set(keys.slice(Math.min(start, end), Math.max(start, end) + 1)),
            );
            setSelectionAnchor(key);
          })
          .catch((error: unknown) => {
            setNotice({
              tone: "error",
              message: `范围选择失败：${errorMessage(error)}`,
            });
          });
        return;
      }
    }
    void replaceSelectionSnapshot(undefined);
    setSelected((current) => {
      if (intent.toggle) {
        const next = new Set(current);
        if (next.has(key)) next.delete(key);
        else next.add(key);
        return next;
      }
      return new Set([key]);
    });
    setSelectionAnchor(key);
  };

  const editSelection = async (patch: MetadataPatch) => {
    if (selectedAssets.length === 0) return;
    setEditBusy(true);
    try {
      if (selectionContext === undefined) {
        throw new Error("目录 revision 尚未就绪，请等待当前视图刷新");
      }
      const currentSnapshot = selectionSnapshotRef.current;
      const snapshot =
        currentSnapshot !== undefined &&
        currentSnapshot.itemCount === selectedAssets.length &&
        currentSnapshot.catalogRevision ===
          selectionContext.expectedCatalogRevision
          ? currentSnapshot
          : await api.createExplicitSelectionSnapshot({
              expectedCatalogRevision: selectionContext.expectedCatalogRevision,
              keys: selectedAssets.map((asset) => asset.key),
            });
      if (snapshot.id !== currentSnapshot?.id) {
        await replaceSelectionSnapshot(snapshot);
      }
      if (selectedAssets.length > 1) {
        const preflight = await api.prepareMetadataBatch({
          snapshotId: snapshot.id,
          patch,
        });
        setPendingBatch(preflight);
        setBatchProgress(undefined);
        setNotice({
          tone: preflight.failureCount > 0 ? "error" : "info",
          message:
            preflight.failureCount > 0
              ? `预检完成：${preflight.executableCount}/${preflight.requestedCount} 项可执行，${preflight.failureCount} 项需跳过。请检查后确认。`
              : `预检完成：${preflight.executableCount} 项可执行，请确认批量写入。`,
        });
        return;
      }
      const result = await api.editAssetMetadata({
        targets: selectedAssets.map((asset) => ({
          key: asset.key,
          expectedSidecarDigest: asset.sidecarState?.digest ?? null,
          expectedSidecarSize: asset.sidecarState?.size ?? null,
          expectedSidecarModifiedUnixMs:
            asset.sidecarState?.modifiedUnixMs ?? null,
        })),
        patch,
      });
      setAssets((current) => upsertAssets(current, result.updated));
      if (result.transaction !== null) {
        const transaction = result.transaction;
        setMetadataTransactions((current) => [
          transaction,
          ...current.filter((item) => item.id !== transaction.id),
        ]);
      }
      if (result.updated.length > 0 || result.conflicts.length > 0) {
        setMetadataConflicts((current) => {
          const incoming = new Set(result.conflicts.map((item) => item.key));
          const updated = new Set(result.updated.map((item) => item.key));
          return [
            ...result.conflicts,
            ...current.filter(
              (item) => !incoming.has(item.key) && !updated.has(item.key),
            ),
          ];
        });
      }
      if (result.failures.length > 0) {
        setNotice({
          tone: "error",
          message:
            result.conflicts.length > 0
              ? `${result.updated.length} 项已更新，${result.conflicts.length} 项需要显式解决并发冲突。`
              : `${result.updated.length} 项已更新，${result.failures.length} 项失败：${result.failures[0].message}`,
        });
      } else {
        setNotice({
          tone: "info",
          message: `已更新 ${result.updated.length} 项素材的 Sidecar`,
        });
      }
    } catch (error) {
      await replaceSelectionSnapshot(undefined);
      setNotice({
        tone: "error",
        message: `元数据写入失败：${errorMessage(error)}`,
      });
    } finally {
      setEditBusy(false);
    }
  };

  const confirmPendingBatch = async () => {
    if (pendingBatch === undefined || pendingBatch.executableCount === 0) return;
    setEditBusy(true);
    setBatchExecutionOperation(pendingBatch.operationId);
    try {
      const result = await api.executeMetadataBatch(
        preflightConfirmation(pendingBatch),
        (event) => setBatchProgress(event.data.progress),
      );
      selectionSnapshotRef.current = undefined;
      setSelectionSnapshot(undefined);
      setPendingBatch(undefined);
      setAssets((current) => upsertAssets(current, result.updated));
      setMetadataTransactions((current) => [
        result.transaction,
        ...current.filter((item) => item.id !== result.transaction.id),
      ]);
      setNotice({
        tone: result.failures.length > 0 ? "error" : "info",
        message: result.stopped
          ? `批量写入已停止：${result.transaction.appliedCount} 项完成，${result.transaction.plannedCount} 项待继续。`
          : `批量写入完成：${result.transaction.appliedCount} 项成功，${result.failures.length} 项失败。`,
      });
    } catch (error) {
      await api
        .releaseBatchPreflight(pendingBatch.operationId)
        .catch(() => false);
      setPendingBatch(undefined);
      await replaceSelectionSnapshot(undefined);
      void api
        .listMetadataTransactions()
        .then(setMetadataTransactions)
        .catch(() => undefined);
      setNotice({
        tone: "error",
        message: `批量写入失败：${errorMessage(error)}`,
      });
    } finally {
      setBatchExecutionOperation(undefined);
      setEditBusy(false);
    }
  };

  const cancelPendingBatch = async () => {
    if (pendingBatch === undefined) return;
    if (batchExecutionOperation !== undefined) {
      await api.cancelMetadataBatch(batchExecutionOperation);
      return;
    }
    await api.releaseBatchPreflight(pendingBatch.operationId).catch(() => false);
    setPendingBatch(undefined);
    setBatchProgress(undefined);
    await replaceSelectionSnapshot(undefined);
  };

  const resolveConflict = async (
    conflict: MetadataConflict,
    resolution: MetadataConflictResolution,
  ) => {
    setMetadataConflictBusy(conflict.id);
    try {
      const updated = await api.resolveMetadataConflict(
        conflict.id,
        resolution,
      );
      setAssets((current) => upsertAssets(current, [updated]));
      setMetadataConflicts((current) =>
        current.filter((item) => item.id !== conflict.id),
      );
      setNotice({ tone: "info", message: "并发冲突已按所选版本安全解决。" });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `冲突解决失败：${errorMessage(error)}`,
      });
    } finally {
      setMetadataConflictBusy(undefined);
    }
  };

  const dismissConflict = async (conflict: MetadataConflict) => {
    setMetadataConflictBusy(conflict.id);
    try {
      await api.dismissMetadataConflict(conflict.id);
      setMetadataConflicts((current) =>
        current.filter((item) => item.id !== conflict.id),
      );
      const root = rootsById.current.get(
        assets.get(conflict.key)?.rootId ?? "",
      );
      if (root !== undefined) await runScan(root);
      setNotice({
        tone: "info",
        message: "已放弃本次修改并重新扫描外部版本。",
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `无法关闭冲突：${errorMessage(error)}`,
      });
    } finally {
      setMetadataConflictBusy(undefined);
    }
  };

  const recoverMetadataTransaction = async (
    transaction: MetadataTransactionSummary,
    action: "continue" | "restore",
  ) => {
    setTransactionBusy(transaction.id);
    try {
      const result =
        action === "continue"
          ? await api.continueMetadataTransaction(transaction.id)
          : await api.restoreMetadataTransaction(transaction.id);
      setMetadataTransactions((current) =>
        current.map((item) =>
          item.id === result.summary.id ? result.summary : item,
        ),
      );
      const affectedRoots = result.summary.rootIds
        .map((rootId) => rootsById.current.get(rootId))
        .filter(
          (root): root is LibraryRootStatus =>
            root !== undefined &&
            root.enabled &&
            root.accessStatus === "available",
        );
      await Promise.all(affectedRoots.map(runScan));
      setNotice({
        tone: result.failures.length === 0 ? "info" : "error",
        message:
          result.failures.length === 0
            ? action === "continue"
              ? "批量事务已安全继续，正在重新扫描受影响目录。"
              : "批量事务已恢复，正在重新扫描受影响目录。"
            : `${result.failures.length} 项因外部修改或写入条件不满足而保持未覆盖。`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `批量事务操作失败：${errorMessage(error)}`,
      });
    } finally {
      setTransactionBusy(undefined);
    }
  };

  const dismissMetadataTransaction = async (
    transaction: MetadataTransactionSummary,
  ) => {
    setTransactionBusy(transaction.id);
    try {
      await api.dismissMetadataTransaction(transaction.id);
      setMetadataTransactions((current) =>
        current.filter((item) => item.id !== transaction.id),
      );
    } catch (error) {
      setNotice({
        tone: "error",
        message: `无法移除事务日志：${errorMessage(error)}`,
      });
    } finally {
      setTransactionBusy(undefined);
    }
  };

  const addRoot = async (path: string, name: string) => {
    setRootBusy(true);
    try {
      const root = await api.addLibraryRoot({ path, name });
      setRoots((current) => [...current, root]);
      setNotice({ tone: "info", message: `已添加 ${root.name}，正在扫描` });
      await runScan(root);
    } catch (error) {
      setNotice({
        tone: "error",
        message: `添加素材根失败：${errorMessage(error)}`,
      });
      throw error;
    } finally {
      setRootBusy(false);
    }
  };

  const toggleRoot = async (root: LibraryRootStatus) => {
    setRootBusy(true);
    try {
      const updated = await api.updateLibraryRoot({
        id: root.id,
        enabled: !root.enabled,
      });
      setRoots((current) =>
        current.map((candidate) =>
          candidate.id === root.id ? updated : candidate,
        ),
      );
      if (updated.enabled && updated.accessStatus === "available") {
        await runScan(updated);
      } else {
        setAssets((current) => removeRootAssets(current, root.id));
      }
    } catch (error) {
      setNotice({
        tone: "error",
        message: `素材根更新失败：${errorMessage(error)}`,
      });
    } finally {
      setRootBusy(false);
    }
  };

  const removeRoot = async (root: LibraryRootStatus) => {
    setRootBusy(true);
    try {
      await api.removeLibraryRoot(root.id);
      setRoots((current) =>
        current.filter((candidate) => candidate.id !== root.id),
      );
      setAssets((current) => removeRootAssets(current, root.id));
      setNotice({
        tone: "info",
        message: `已移除 ${root.name} 的配置，磁盘文件保持不变`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `移除素材根失败：${errorMessage(error)}`,
      });
      throw error;
    } finally {
      setRootBusy(false);
    }
  };

  const changeTagFilter = (tag: string, state: TagFilterState) => {
    setTagFilters((current) => {
      const next = { ...current };
      const nextState = cycleTagFilter(state);
      if (nextState === "neutral") delete next[tag];
      else next[tag] = nextState;
      return next;
    });
  };

  const openRootManager = () => {
    setRootManagerOpen(true);
    void api
      .listLibraryRoots()
      .then(setRoots)
      .catch((error: unknown) =>
        setNotice({
          tone: "error",
          message: `素材位置状态刷新失败：${errorMessage(error)}`,
        }),
      );
  };

  const refreshSavedFilters = async () => {
    try {
      setSavedFilters(await api.listSavedFilters());
    } catch (error) {
      setNotice({
        tone: "error",
        message: `保存过滤器读取失败：${errorMessage(error)}`,
      });
      throw error;
    }
  };

  const openSavedFilterManager = () => {
    setSavedFilterManagerOpen(true);
    void refreshSavedFilters().catch(() => undefined);
  };

  const runSavedFilterMutation = async (
    action: () => Promise<unknown>,
    successMessage: string,
  ) => {
    setSavedFilterBusy(true);
    try {
      await action();
      await refreshSavedFilters();
      setNotice({ tone: "info", message: successMessage });
    } catch (error) {
      if (savedFilterErrorKind(error) === "external-change") {
        await refreshSavedFilters().catch(() => undefined);
      }
      setNotice({
        tone: "error",
        message: `保存过滤器操作失败：${errorMessage(error)}`,
      });
      throw error;
    } finally {
      setSavedFilterBusy(false);
    }
  };

  const createSavedFilter = async (input: SavedFilterInput) =>
    runSavedFilterMutation(
      () => api.createSavedFilter(savedFilters.fileVersion, input),
      `已保存过滤器“${input.name}”`,
    );

  const updateSavedFilter = async (
    filter: SavedFilter,
    input: SavedFilterInput,
  ) =>
    runSavedFilterMutation(
      () => api.updateSavedFilter(savedFilters.fileVersion, filter.id, input),
      `已从当前视图更新“${filter.name}”`,
    );

  const renameSavedFilter = async (filter: SavedFilter, name: string) =>
    runSavedFilterMutation(
      () =>
        api.renameSavedFilter(savedFilters.fileVersion, filter.id, name.trim()),
      `已将过滤器重命名为“${name.trim()}”`,
    );

  const deleteSavedFilter = async (filter: SavedFilter) =>
    runSavedFilterMutation(
      () => api.deleteSavedFilter(savedFilters.fileVersion, filter.id),
      `已删除过滤器“${filter.name}”`,
    ).then(() => {
      if (activeSavedFilter?.id === filter.id) setActiveSavedFilter(undefined);
    });

  const activateSavedFilter = async (filter: SavedFilter) => {
    setSavedFilterBusy(true);
    try {
      const execution = await api.executeSavedFilter(filter.id);
      setTagFilters({});
      setExpression(execution.expression);
      setActiveSavedFilter({
        id: filter.id,
        expression: execution.expression,
      });
      setQueryView(
        settleSuccessfulQuery(
          execution.orderedKeys.filter((key) => assets.has(key)),
        ),
      );
      setNotice({
        tone: execution.missingRootIds.length === 0 ? "info" : "error",
        message:
          execution.missingRootIds.length === 0
            ? `已应用“${filter.name}”，从当前文件匹配 ${execution.matchedAssets} 项`
            : `已应用可用范围；${execution.missingRootIds.length} 个素材位置当前不可用`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `保存过滤器执行失败：${errorMessage(error)}`,
      });
      throw error;
    } finally {
      setSavedFilterBusy(false);
    }
  };

  const openVaultManager = () => {
    setVaultManagerOpen(true);
    void api
      .listObsidianVaults()
      .then((value) => {
        setVaults(value);
        setActiveVaultId((current) =>
          current && value.some((vault) => vault.id === current)
            ? current
            : value.find(
                (vault) => vault.enabled && vault.accessStatus === "available",
              )?.id,
        );
      })
      .catch((error: unknown) =>
        setNotice({
          tone: "error",
          message: `Vault 状态刷新失败：${errorMessage(error)}`,
        }),
      );
  };

  const openSettingsManager = () => {
    setSettingsOpen(true);
    void api
      .getRuntimeRecoveryStatus()
      .then(setRecoveryStatus)
      .catch((error: unknown) =>
        setNotice({
          tone: "error",
          message: `恢复状态刷新失败：${errorMessage(error)}`,
        }),
      );
  };

  const resetSavedView = () => {
    setExpression("");
    setTagFilters({});
    setNotice({ tone: "info", message: "已清除保存的查询与 Tag 条件" });
  };

  const rebuildDerivedState = async () => {
    if (activeScans.length > 0) return;
    setResetBusy(true);
    try {
      const report = await api.resetDerivedState();
      setResetReport(report);
      setAssets(new Map());
      setQueryView(settleSuccessfulQuery([]));
      setSelected(new Set());
      setSelectionAnchor(undefined);
      setScans({});
      const availableRoots = roots.filter(
        (root) => root.enabled && root.accessStatus === "available",
      );
      await Promise.all(availableRoots.map(runScan));
      setNotice({
        tone: "info",
        message: `已清理 ${report.cache.removedFiles} 个缓存文件，并从 ${availableRoots.length} 个素材位置开始重建`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `派生数据重建失败：${errorMessage(error)}`,
      });
    } finally {
      setResetBusy(false);
    }
  };

  const reclaimThumbnailCache = async () => {
    if (activeScans.length > 0) return;
    setCacheMaintenanceBusy(true);
    try {
      const report = await api.maintainThumbnailCache();
      setCacheMaintenanceReport(report);
      setRecoveryStatus((current) =>
        current ? { ...current, cacheStats: report.stats } : current,
      );
      setNotice({
        tone: "info",
        message: `已回收 ${report.removedEntries} 个无效缓存条目，当前保留 ${report.stats.entryCount} 项`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `缓存回收失败：${errorMessage(error)}`,
      });
    } finally {
      setCacheMaintenanceBusy(false);
    }
  };

  const createDiagnosticExport = async () => {
    setDiagnosticBusy(true);
    try {
      const report = await api.exportDiagnostics();
      setDiagnosticReport(report);
      setNotice({
        tone: "info",
        message: `诊断日志已导出，共 ${report.eventCount} 条事件`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `诊断日志导出失败：${errorMessage(error)}`,
      });
    } finally {
      setDiagnosticBusy(false);
    }
  };

  const copyDiagnosticPath = async () => {
    if (diagnosticReport === undefined) return;
    try {
      await copyLocalPath(diagnosticReport.path);
      setNotice({ tone: "info", message: "诊断日志路径已复制" });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `复制诊断日志路径失败：${errorMessage(error)}`,
      });
    }
  };

  const inspectConsistency = async () => {
    setSupportBusy("consistency");
    try {
      const report = await api.inspectLibraryConsistency();
      setConsistencyReport(report);
      setNotice({
        tone: report.summary.errors === 0 ? "info" : "error",
        message: report.authoritative
          ? `一致性检查完成：${String(report.summary.errors)} 个错误，${String(report.summary.warnings)} 个警告`
          : "一致性检查完成，但仍有素材根尚未完成权威扫描",
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `一致性检查失败：${errorMessage(error)}`,
      });
    } finally {
      setSupportBusy(undefined);
    }
  };

  const traceAsset = async (assetId: string) => {
    setSupportBusy("trace");
    try {
      const report = await api.traceAssetSupport(assetId);
      setAssetTraceReport(report);
      setNotice({
        tone: report.matchCount === 1 ? "info" : "error",
        message: `素材追踪完成：匹配 ${String(report.matchCount)} 条目录记录`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `素材追踪失败：${errorMessage(error)}`,
      });
    } finally {
      setSupportBusy(undefined);
    }
  };

  const addVault = async (path: string, name: string) => {
    setVaultBusy(true);
    try {
      const vault = await api.addObsidianVault({ path, name });
      setVaults((current) => [...current, vault]);
      setActiveVaultId(vault.id);
      setNotice({
        tone: "info",
        message: `已添加 ${vault.name} 并设为 Obsidian 目标 Vault`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `添加 Vault 失败：${errorMessage(error)}`,
      });
      throw error;
    } finally {
      setVaultBusy(false);
    }
  };

  const toggleVault = async (vault: ObsidianVaultStatus) => {
    setVaultBusy(true);
    try {
      const updated = await api.updateObsidianVault({
        id: vault.id,
        enabled: !vault.enabled,
      });
      const next = vaults.map((candidate) =>
        candidate.id === vault.id ? updated : candidate,
      );
      setVaults(next);
      if (!updated.enabled && activeVaultId === updated.id) {
        setActiveVaultId(
          next.find(
            (candidate) =>
              candidate.enabled && candidate.accessStatus === "available",
          )?.id,
        );
      } else if (
        updated.enabled &&
        updated.accessStatus === "available" &&
        activeVaultId === undefined
      ) {
        setActiveVaultId(updated.id);
      }
    } catch (error) {
      setNotice({
        tone: "error",
        message: `更新 Vault 失败：${errorMessage(error)}`,
      });
    } finally {
      setVaultBusy(false);
    }
  };

  const removeVault = async (vault: ObsidianVaultStatus) => {
    setVaultBusy(true);
    try {
      await api.removeObsidianVault(vault.id);
      const next = vaults.filter((candidate) => candidate.id !== vault.id);
      setVaults(next);
      if (activeVaultId === vault.id) {
        setActiveVaultId(
          next.find(
            (candidate) =>
              candidate.enabled && candidate.accessStatus === "available",
          )?.id,
        );
      }
      setNotice({
        tone: "info",
        message: `已移除 ${vault.name} 的授权配置，Vault 文件保持不变`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `移除 Vault 失败：${errorMessage(error)}`,
      });
      throw error;
    } finally {
      setVaultBusy(false);
    }
  };

  const copySelectedVaultReference = async () => {
    const asset = selectedAssets.length === 1 ? selectedAssets[0] : undefined;
    const reference = asset ? vaultReferences.get(asset.key) : undefined;
    if (reference === undefined) return;
    try {
      await copyVaultReference(reference);
      setNotice({
        tone: "info",
        message: `已复制 ${reference.markdown}`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `复制 Obsidian 引用失败：${errorMessage(error)}`,
      });
    }
  };

  const cancelScans = async () => {
    await Promise.all(
      activeScans.flatMap(([rootId, scan]) =>
        scan.scanId
          ? [
              api.cancelLibraryScan(scan.scanId).then(() => {
                setScans((current) => ({
                  ...current,
                  [rootId]: { ...current[rootId], phase: "cancelled" },
                }));
              }),
            ]
          : [],
      ),
    );
  };

  return (
    <div className="desktop-shell">
      <header className="topbar">
        <a className="brand" href="#library" aria-label="Material Eagle 素材库">
          <span className="brand-mark">
            <Icon name="grid" size={17} />
          </span>
          <span>
            <strong>Material</strong>
            <small>Filesystem library</small>
          </span>
        </a>

        <div className="search-area">
          <div className={`search-box${queryError ? " has-error" : ""}`}>
            <Icon name="search" size={17} />
            <input
              aria-describedby={queryError ? "query-error" : undefined}
              aria-invalid={queryError ? true : undefined}
              aria-label="搜索和过滤素材"
              onChange={(event) => setExpression(event.target.value)}
              placeholder="搜索 Tag，或输入 type:image  /  favorite:true"
              ref={search}
              spellCheck={false}
              value={expression}
            />
            <kbd>/</kbd>
            <AdvancedFilterBuilder
              onAdd={(predicate) =>
                setExpression((current) =>
                  appendAdvancedPredicate(current, predicate),
                )
              }
            />
          </div>
        </div>

        <div className="topbar-actions">
          <button
            className="library-button saved-filter-button"
            onClick={openSavedFilterManager}
            type="button"
          >
            <Icon name="search" size={16} />
            <span>
              {activeSavedFilter
                ? (savedFilters.validFilters
                    .concat(
                      savedFilters.unavailableFilters.map(
                        (entry) => entry.filter,
                      ),
                    )
                    .find((filter) => filter.id === activeSavedFilter.id)
                    ?.name ?? "保存过滤器")
                : "保存过滤器"}
            </span>
            <Icon name="chevron" size={13} />
          </button>
          <button
            className="library-button vault-button"
            onClick={openVaultManager}
            type="button"
          >
            <Icon name="link" size={16} />
            <span>{activeVault?.name ?? "配置 Vault"}</span>
            <Icon name="chevron" size={13} />
          </button>
          <button
            className="library-button"
            onClick={openRootManager}
            type="button"
          >
            <Icon name="library" size={16} />
            <span>{roots.length} 个素材位置</span>
            <Icon name="chevron" size={13} />
          </button>
          <button
            aria-label="应用设置与恢复"
            className="icon-button topbar-settings"
            onClick={openSettingsManager}
            type="button"
          >
            <Icon name="settings" size={16} />
          </button>
        </div>
      </header>

      <div className="workspace" id="library">
        <aside className="filter-sidebar" aria-label="素材过滤器">
          <div className="sidebar-heading">
            <span className="overline">Collections</span>
            <h2>全部素材</h2>
            <p>{allAssets.length.toLocaleString()} 项已发现</p>
          </div>

          <nav className="quick-filters" aria-label="快速过滤">
            <button
              className={expression === "" ? "is-active" : ""}
              onClick={() => setExpression("")}
              type="button"
            >
              <Icon name="grid" size={15} /> 全部{" "}
              <span>{allAssets.length}</span>
            </button>
            <button
              className={expression === "favorite:true" ? "is-active" : ""}
              onClick={() => setExpression("favorite:true")}
              type="button"
            >
              <Icon name="star" size={15} /> 已收藏{" "}
              <span>{allAssets.filter((asset) => asset.favorite).length}</span>
            </button>
            <button
              className={expression === "type:image" ? "is-active" : ""}
              onClick={() => setExpression("type:image")}
              type="button"
            >
              <Icon name="image" size={15} /> 图片{" "}
              <span>
                {allAssets.filter((asset) => asset.kind === "image").length}
              </span>
            </button>
          </nav>

          <section
            className="tag-filter-panel"
            aria-labelledby="tag-filter-title"
          >
            <header>
              <div>
                <span className="overline">Tags</span>
                <h2 id="tag-filter-title">三态筛选</h2>
              </div>
              {activeTagFilters.length > 0 ? (
                <button
                  aria-label="清除 Tag 筛选"
                  onClick={() => setTagFilters({})}
                  type="button"
                >
                  清除
                </button>
              ) : null}
            </header>
            <p className="tag-legend">
              <span>点击：包含</span>
              <span>再次：排除</span>
            </p>
            <div className="tag-list">
              {tags.map((summary) => (
                <button
                  aria-label={`${summary.tag}，${summary.state === "neutral" ? "未筛选" : summary.state === "include" ? "包含" : "排除"}`}
                  className={`tag-filter tag-filter--${summary.state}`}
                  key={summary.tag}
                  onClick={() => changeTagFilter(summary.tag, summary.state)}
                  type="button"
                >
                  <span className="tag-state">
                    {summary.state === "include" ? (
                      <Icon name="check" size={11} />
                    ) : summary.state === "exclude" ? (
                      <Icon name="minus" size={11} />
                    ) : null}
                  </span>
                  <span title={summary.tag}>{summary.tag}</span>
                  <small>{summary.count}</small>
                </button>
              ))}
              {tags.length === 0 ? (
                <p className="muted-copy">扫描完成后，Tag 会显示在这里。</p>
              ) : null}
            </div>
          </section>

          <footer className="sidebar-footer">
            <span className="runtime-dot" />
            <span>
              v{buildInfo?.version ?? "0.1.0"} ·{" "}
              {isTauriRuntime() ? "LOCAL" : "PREVIEW"}
            </span>
          </footer>
        </aside>

        <main className="library-main">
          <div className="library-heading">
            <div>
              <span className="overline">Flat library</span>
              <h1>素材视图</h1>
            </div>
            <div className="library-actions">
              {selected.size > 0 ? (
                <span className="selection-count">
                  已选择 {selected.size} 项
                </span>
              ) : null}
              <button
                className="text-button"
                disabled={visibleAssets.length === 0 || selectionContext === undefined}
                onClick={() => {
                  if (selectionContext === undefined) return;
                  void api
                    .createQuerySelectionSnapshot(selectionContext)
                    .then(async (snapshot) => {
                      await replaceSelectionSnapshot(snapshot);
                      setSelected(
                        new Set(visibleAssets.map((asset) => asset.key)),
                      );
                    })
                    .catch((error: unknown) => {
                      setNotice({
                        tone: "error",
                        message: `全选失败：${errorMessage(error)}`,
                      });
                    });
                }}
                type="button"
              >
                全选当前结果
              </button>
              {selected.size > 0 ? (
                <button
                  className="icon-button"
                  aria-label="清除选择"
                  onClick={() => {
                    setSelected(new Set());
                    void replaceSelectionSnapshot(undefined);
                  }}
                  type="button"
                >
                  <Icon name="close" size={15} />
                </button>
              ) : null}
            </div>
          </div>

          {activeTagFilters.length > 0 || expression.trim().length > 0 ? (
            <div className="active-filter-row" aria-label="当前过滤条件">
              <span>当前条件</span>
              {expression.trim().length > 0 ? (
                <button onClick={() => setExpression("")} type="button">
                  {expression}
                  <Icon name="close" size={11} />
                </button>
              ) : null}
              {activeTagFilters.map((filter) => (
                <button
                  key={filter.tag}
                  onClick={() =>
                    setTagFilters((current) => {
                      const next = { ...current };
                      delete next[filter.tag];
                      return next;
                    })
                  }
                  type="button"
                >
                  {filter.state === "exclude" ? "排除 " : "包含 "}
                  {filter.tag}
                  <Icon name="close" size={11} />
                </button>
              ))}
            </div>
          ) : null}

          {queryError ? (
            <div className="query-error" id="query-error" role="alert">
              <Icon name="alert" size={15} />
              {queryError}
            </div>
          ) : null}
          {notice ? (
            <div className={`notice notice--${notice.tone}`} role="status">
              <Icon
                name={notice.tone === "error" ? "alert" : "check"}
                size={15}
              />
              <span>{notice.message}</span>
              <button
                aria-label="关闭通知"
                onClick={() => setNotice(undefined)}
                type="button"
              >
                <Icon name="close" size={13} />
              </button>
            </div>
          ) : null}
          {pendingBatch ? (
            <div className="batch-confirmation" role="region" aria-label="批量操作确认">
              <div>
                <strong>批量预检</strong>
                <span>
                  {pendingBatch.executableCount}/{pendingBatch.requestedCount} 项可执行
                  {pendingBatch.failureCount > 0
                    ? `，${pendingBatch.failureCount} 项失败`
                    : ""}
                </span>
                {batchProgress ? (
                  <span>
                    已提交 {batchProgress.appliedCount}，待处理 {batchProgress.plannedCount}
                  </span>
                ) : null}
                {pendingBatch.failures[0] ? (
                  <small>
                    首项：{pendingBatch.failures[0].key} · {pendingBatch.failures[0].message}
                  </small>
                ) : null}
              </div>
              <div className="batch-confirmation__actions">
                <button
                  disabled={
                    pendingBatch.executableCount === 0 ||
                    batchExecutionOperation !== undefined
                  }
                  onClick={() => void confirmPendingBatch()}
                  type="button"
                >
                  确认写入
                </button>
                <button onClick={() => void cancelPendingBatch()} type="button">
                  {batchExecutionOperation === undefined ? "取消" : "停止后续写入"}
                </button>
              </div>
            </div>
          ) : null}

          <div className="result-meta" aria-live="polite">
            <span>
              {queryPending ? "正在过滤…" : `${visibleAssets.length} 项结果`}
            </span>
            {activeScans.length > 0 ? (
              <span className="scan-progress">
                <i /> 正在扫描，已发现{" "}
                {activeScans.reduce(
                  (sum, [, scan]) => sum + scan.assetCount,
                  0,
                )}{" "}
                项
                <button
                  disabled={activeScans.some(([, scan]) => !scan.scanId)}
                  onClick={() => void cancelScans()}
                  type="button"
                >
                  停止
                </button>
              </span>
            ) : failedScans.length > 0 ? (
              <span className="scan-failure" role="alert">
                <Icon name="alert" size={13} />
                扫描失败：{failedScans[0][1].message ?? "请检查素材位置"}
              </span>
            ) : null}
          </div>

          {booting ? (
            <EmptyState
              icon="refresh"
              title="正在读取素材配置"
              copy="所有内容都从本地文件系统解释，不读取远程数据库。"
            />
          ) : roots.length === 0 ? (
            <EmptyState
              icon="folder"
              title="从一个素材文件夹开始"
              copy="添加现有目录后，文件会原地保留；应用只建立可重建视图。"
              action="添加素材位置"
              onAction={openRootManager}
            />
          ) : inaccessibleRoots.length > 0 &&
            allAssets.length === 0 &&
            activeScans.length === 0 ? (
            <EmptyState
              icon="alert"
              title="素材位置当前不可访问"
              copy={`${inaccessibleRoots[0].name}：${inaccessibleRoots[0].accessMessage ?? "请检查权限或磁盘连接"}`}
              action="检查素材位置"
              onAction={openRootManager}
            />
          ) : activeScans.length > 0 && allAssets.length === 0 ? (
            <EmptyState
              icon="refresh"
              title="正在解释文件与 Sidecar"
              copy="素材会按批次出现，缩略图只在进入视口后生成。"
            />
          ) : visibleAssets.length === 0 ? (
            <EmptyState
              icon="search"
              title="没有符合条件的素材"
              copy="尝试清除排除条件，或检查查询语法。"
              action="清除全部筛选"
              onAction={() => {
                setExpression("");
                setTagFilters({});
              }}
            />
          ) : (
            <AssetGrid
              api={api}
              assets={visibleAssets}
              onSelect={selectAsset}
              onWindowChange={setGridWindowKeys}
              selected={selected}
              vaultReferences={vaultReferences}
            />
          )}
        </main>

        <Inspector
          assets={selectedAssets}
          busy={editBusy || metadataConflictBusy !== undefined}
          conflictBusy={metadataConflictBusy}
          conflicts={metadataConflicts}
          obsidian={{
            vault: activeVault,
            reference:
              selectedAssets.length === 1
                ? vaultReferences.get(selectedAssets[0].key)
                : undefined,
            failure:
              selectedAssets.length === 1
                ? vaultReferenceFailures.get(selectedAssets[0].key)
                : undefined,
            pending: vaultReferencesPending,
            onCopy: copySelectedVaultReference,
            onConfigure: openVaultManager,
          }}
          onDismissConflict={dismissConflict}
          onEdit={editSelection}
          onResolveConflict={resolveConflict}
        />
      </div>

      <RootManager
        busy={rootBusy}
        onAdd={addRoot}
        onClose={() => setRootManagerOpen(false)}
        onRemove={removeRoot}
        onScan={runScan}
        onToggle={toggleRoot}
        open={rootManagerOpen}
        onConfirmRelink={confirmRelink}
        reconciliationReports={reconciliationReports}
        relinkBusy={relinkBusy}
        roots={roots}
        scanningRootIds={new Set(activeScans.map(([rootId]) => rootId))}
      />
      <SavedFilterManager
        activeFilterId={activeSavedFilter?.id}
        busy={savedFilterBusy}
        catalog={savedFilters}
        currentQuery={effectiveQuery}
        onActivate={activateSavedFilter}
        onClose={() => setSavedFilterManagerOpen(false)}
        onCreate={createSavedFilter}
        onDelete={deleteSavedFilter}
        onRefresh={refreshSavedFilters}
        onRename={renameSavedFilter}
        onUpdate={updateSavedFilter}
        open={savedFilterManagerOpen}
        roots={roots}
      />
      <VaultManager
        activeVaultId={activeVaultId}
        busy={vaultBusy}
        onAdd={addVault}
        onClose={() => setVaultManagerOpen(false)}
        onRemove={removeVault}
        onSelect={setActiveVaultId}
        onToggle={toggleVault}
        open={vaultManagerOpen}
        vaults={vaults}
      />
      <SettingsManager
        assetTraceReport={assetTraceReport}
        cacheMaintenanceBusy={cacheMaintenanceBusy}
        cacheMaintenanceReport={cacheMaintenanceReport}
        config={displayedApplicationConfig}
        diagnosticBusy={diagnosticBusy}
        diagnosticReport={diagnosticReport}
        consistencyReport={consistencyReport}
        onClose={() => setSettingsOpen(false)}
        onCopyDiagnosticPath={copyDiagnosticPath}
        onContinueTransaction={(transaction) =>
          recoverMetadataTransaction(transaction, "continue")
        }
        onDismissTransaction={dismissMetadataTransaction}
        onExportDiagnostics={createDiagnosticExport}
        onInspectConsistency={inspectConsistency}
        onMaintainCache={reclaimThumbnailCache}
        onResetDerived={rebuildDerivedState}
        onResetView={resetSavedView}
        onRestoreTransaction={(transaction) =>
          recoverMetadataTransaction(transaction, "restore")
        }
        onTraceAsset={traceAsset}
        open={settingsOpen}
        recovery={recoveryStatus}
        resetBusy={resetBusy}
        resetReport={resetReport}
        scanActive={activeScans.length > 0}
        supportBusy={supportBusy}
        transactionBusy={transactionBusy}
        transactions={metadataTransactions}
      />
    </div>
  );
}

function handleScanEvent(
  rootId: string,
  event: LibraryScanEvent,
  setAssets: React.Dispatch<React.SetStateAction<Map<string, AssetRecord>>>,
  setScans: React.Dispatch<React.SetStateAction<Record<string, ScanUiState>>>,
  setSelected: React.Dispatch<React.SetStateAction<Set<string>>>,
  setSelectionAnchor: React.Dispatch<React.SetStateAction<string | undefined>>,
) {
  if (event.event === "started") {
    setScans((current) => ({
      ...current,
      [rootId]: {
        scanId: event.data.scanId,
        phase: "scanning",
        visitedFiles: 0,
        assetCount: 0,
        problemCount: 0,
      },
    }));
  } else if (event.event === "batch") {
    setAssets((current) => upsertAssets(current, event.data.batch.assets));
    setScans((current) => ({
      ...current,
      [rootId]: {
        ...(current[rootId] ?? {
          phase: "scanning",
          assetCount: 0,
          problemCount: 0,
        }),
        phase: "scanning",
        visitedFiles: event.data.batch.visitedFiles,
        assetCount:
          (current[rootId]?.assetCount ?? 0) + event.data.batch.assets.length,
        problemCount:
          (current[rootId]?.problemCount ?? 0) +
          event.data.batch.problems.length,
      },
    }));
  } else if (event.event === "finished") {
    const { movedAssets, removedKeys } = event.data.reconciliation;
    setAssets((current) =>
      removeAssetKeys(
        upsertAssets(current, event.data.reconciliation.restoredRecords),
        removedKeys,
      ),
    );
    setSelected((current) =>
      reconcileSelectedKeys(current, movedAssets, removedKeys),
    );
    setSelectionAnchor((current) =>
      reconcileSelectionAnchor(current, movedAssets, removedKeys),
    );
    setScans((current) => ({
      ...current,
      [rootId]: {
        scanId: event.data.scanId,
        phase:
          event.data.summary.completion === "cancelled"
            ? "cancelled"
            : "completed",
        visitedFiles: event.data.summary.visitedFiles,
        assetCount: event.data.summary.assetCount,
        problemCount: event.data.summary.problemCount,
      },
    }));
  } else {
    setAssets((current) =>
      removeAssetKeys(
        upsertAssets(current, event.data.restoredRecords),
        event.data.removedKeys,
      ),
    );
    setScans((current) => ({
      ...current,
      [rootId]: {
        scanId: event.data.scanId,
        phase: "failed",
        visitedFiles: current[rootId]?.visitedFiles ?? 0,
        assetCount: current[rootId]?.assetCount ?? 0,
        problemCount: current[rootId]?.problemCount ?? 0,
        message: event.data.message,
      },
    }));
  }
}

function removeAssetKeys(
  assets: Map<string, AssetRecord>,
  removedKeys: readonly string[],
): Map<string, AssetRecord> {
  if (removedKeys.length === 0) return assets;
  const next = new Map(assets);
  for (const key of removedKeys) next.delete(key);
  return next;
}

function EmptyState({
  icon,
  title,
  copy,
  action,
  onAction,
}: {
  icon: "alert" | "folder" | "refresh" | "search";
  title: string;
  copy: string;
  action?: string;
  onAction?: () => void;
}) {
  return (
    <section className="empty-state">
      <span>
        <Icon name={icon} size={22} />
      </span>
      <h2>{title}</h2>
      <p>{copy}</p>
      {action && onAction ? (
        <button className="primary-button" onClick={onAction} type="button">
          {action}
        </button>
      ) : null}
    </section>
  );
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (typeof error === "object" && error !== null && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return "未知错误";
}

function savedFilterErrorKind(
  error: unknown,
): SavedFilterCommandError["kind"] | undefined {
  if (typeof error !== "object" || error === null || !("kind" in error)) {
    return undefined;
  }
  return (error as { kind?: SavedFilterCommandError["kind"] }).kind;
}
