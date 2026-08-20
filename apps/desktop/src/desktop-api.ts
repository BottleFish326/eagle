import type { QueryAssetsInput, QueryAssetsResult } from "./asset-query";
import { queryAssets } from "./asset-query";
import type {
  ApplicationConfig,
  DerivedStateResetReport,
  DiagnosticExportReport,
  RuntimeRecoveryStatus,
  RuntimeResourceStatus,
  UiPreferences,
} from "./application-runtime";
import {
  exportDiagnostics,
  getApplicationConfig,
  getRuntimeRecoveryStatus,
  getRuntimeResourceStatus,
  resetDerivedState,
  updateApplicationConfig,
} from "./application-runtime";
import type {
  AddLibraryRootInput,
  LibraryRoot,
  LibraryRootStatus,
  UpdateLibraryRootInput,
} from "./library-roots";
import type { LibraryWatchEvent } from "./filesystem-events";
import { startLibraryWatch, stopLibraryWatch } from "./filesystem-events";
import {
  addLibraryRoot,
  listLibraryRoots,
  removeLibraryRoot,
  updateLibraryRoot,
} from "./library-roots";
import type {
  BatchMetadataEdit,
  BatchMetadataEditResult,
} from "./metadata-editor";
import type {
  AddObsidianVaultInput,
  ObsidianVault,
  ObsidianVaultStatus,
  ResolveVaultReferencesInput,
  ResolveVaultReferencesResult,
  UpdateObsidianVaultInput,
} from "./obsidian-vaults";
import {
  addObsidianVault,
  listObsidianVaults,
  removeObsidianVault,
  resolveObsidianVaultReferences,
  updateObsidianVault,
} from "./obsidian-vaults";
import { editAssetMetadata } from "./metadata-editor";
import type { MetadataConflictResolution } from "./metadata-conflicts";
import {
  dismissMetadataConflict,
  resolveMetadataConflict,
} from "./metadata-conflicts";
import type {
  MetadataTransactionRecoveryResult,
  MetadataTransactionSummary,
} from "./metadata-transactions";
import {
  continueMetadataTransaction,
  dismissMetadataTransaction,
  listMetadataTransactions,
  restoreMetadataTransaction,
} from "./metadata-transactions";
import type { ReconciliationReport, RelinkReceipt } from "./reconciliation";
import {
  confirmLibraryRelink,
  inspectLibraryReconciliation,
} from "./reconciliation";
import type { AssetRecord, LibraryScanEvent } from "./scanner";
import { cancelLibraryScan, startLibraryScan } from "./scanner";
import type {
  AssetTraceReport,
  LibraryConsistencyReport,
} from "./support-tools";
import { inspectLibraryConsistency, traceAssetSupport } from "./support-tools";
import type {
  CacheClearReport,
  CacheMaintenanceReport,
  ThumbnailOutcome,
  ThumbnailRequest,
} from "./thumbnail";
import {
  clearThumbnailCache,
  maintainThumbnailCache,
  readThumbnail,
  requestThumbnail,
} from "./thumbnail";
import type {
  SavedFilterCatalog,
  SavedFilterExecution,
  SavedFilterFileVersion,
  SavedFilterInput,
  SavedFilterMutation,
} from "./saved-filters";
import {
  createSavedFilter,
  deleteSavedFilter,
  executeSavedFilter,
  listSavedFilters,
  renameSavedFilter,
  updateSavedFilter,
} from "./saved-filters";

export interface DesktopApi {
  getApplicationConfig(): Promise<ApplicationConfig>;
  updateApplicationConfig(input: UiPreferences): Promise<ApplicationConfig>;
  getRuntimeRecoveryStatus(): Promise<RuntimeRecoveryStatus>;
  getRuntimeResourceStatus(): Promise<RuntimeResourceStatus>;
  resetDerivedState(): Promise<DerivedStateResetReport>;
  exportDiagnostics(): Promise<DiagnosticExportReport>;
  inspectLibraryConsistency(): Promise<LibraryConsistencyReport>;
  traceAssetSupport(assetId: string): Promise<AssetTraceReport>;
  listLibraryRoots(): Promise<LibraryRootStatus[]>;
  addLibraryRoot(input: AddLibraryRootInput): Promise<LibraryRootStatus>;
  updateLibraryRoot(input: UpdateLibraryRootInput): Promise<LibraryRootStatus>;
  removeLibraryRoot(id: string): Promise<LibraryRoot>;
  startLibraryScan(
    rootId: string,
    receive: (event: LibraryScanEvent) => void,
  ): Promise<string>;
  cancelLibraryScan(scanId: string): Promise<boolean>;
  inspectLibraryReconciliation(rootId: string): Promise<ReconciliationReport>;
  confirmLibraryRelink(candidateId: string): Promise<RelinkReceipt>;
  startLibraryWatch(
    rootId: string,
    receive: (event: LibraryWatchEvent) => void,
  ): Promise<string>;
  stopLibraryWatch(watchId: string): Promise<boolean>;
  queryAssets(input: QueryAssetsInput): Promise<QueryAssetsResult>;
  listSavedFilters(): Promise<SavedFilterCatalog>;
  createSavedFilter(
    expectedVersion: SavedFilterFileVersion,
    input: SavedFilterInput,
  ): Promise<SavedFilterMutation>;
  updateSavedFilter(
    expectedVersion: SavedFilterFileVersion,
    id: string,
    input: SavedFilterInput,
  ): Promise<SavedFilterMutation>;
  renameSavedFilter(
    expectedVersion: SavedFilterFileVersion,
    id: string,
    name: string,
  ): Promise<SavedFilterMutation>;
  deleteSavedFilter(
    expectedVersion: SavedFilterFileVersion,
    id: string,
  ): Promise<SavedFilterMutation>;
  executeSavedFilter(id: string): Promise<SavedFilterExecution>;
  editAssetMetadata(input: BatchMetadataEdit): Promise<BatchMetadataEditResult>;
  resolveMetadataConflict(
    conflictId: string,
    resolution: MetadataConflictResolution,
  ): Promise<AssetRecord>;
  dismissMetadataConflict(conflictId: string): Promise<void>;
  listMetadataTransactions(): Promise<MetadataTransactionSummary[]>;
  continueMetadataTransaction(
    id: string,
  ): Promise<MetadataTransactionRecoveryResult>;
  restoreMetadataTransaction(
    id: string,
  ): Promise<MetadataTransactionRecoveryResult>;
  dismissMetadataTransaction(id: string): Promise<void>;
  requestThumbnail(input: ThumbnailRequest): Promise<ThumbnailOutcome>;
  readThumbnail(cacheKey: string): Promise<ArrayBuffer>;
  clearThumbnailCache(): Promise<CacheClearReport>;
  maintainThumbnailCache(): Promise<CacheMaintenanceReport>;
  listObsidianVaults(): Promise<ObsidianVaultStatus[]>;
  addObsidianVault(input: AddObsidianVaultInput): Promise<ObsidianVaultStatus>;
  updateObsidianVault(
    input: UpdateObsidianVaultInput,
  ): Promise<ObsidianVaultStatus>;
  removeObsidianVault(id: string): Promise<ObsidianVault>;
  resolveObsidianVaultReferences(
    input: ResolveVaultReferencesInput,
  ): Promise<ResolveVaultReferencesResult>;
}

export const tauriDesktopApi: DesktopApi = {
  getApplicationConfig,
  updateApplicationConfig,
  getRuntimeRecoveryStatus,
  getRuntimeResourceStatus,
  resetDerivedState,
  exportDiagnostics,
  inspectLibraryConsistency,
  traceAssetSupport,
  listLibraryRoots,
  addLibraryRoot,
  updateLibraryRoot,
  removeLibraryRoot,
  startLibraryScan,
  cancelLibraryScan,
  inspectLibraryReconciliation,
  confirmLibraryRelink,
  startLibraryWatch,
  stopLibraryWatch,
  queryAssets,
  listSavedFilters,
  createSavedFilter,
  updateSavedFilter,
  renameSavedFilter,
  deleteSavedFilter,
  executeSavedFilter,
  editAssetMetadata,
  resolveMetadataConflict,
  dismissMetadataConflict,
  listMetadataTransactions,
  continueMetadataTransaction,
  restoreMetadataTransaction,
  dismissMetadataTransaction,
  requestThumbnail,
  readThumbnail,
  clearThumbnailCache,
  maintainThumbnailCache,
  listObsidianVaults,
  addObsidianVault,
  updateObsidianVault,
  removeObsidianVault,
  resolveObsidianVaultReferences,
};

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
