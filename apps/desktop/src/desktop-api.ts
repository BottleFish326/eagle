import type { QueryAssetsInput, QueryAssetsResult } from "./asset-query";
import { queryAssets } from "./asset-query";
import type {
  ApplicationConfig,
  DerivedStateResetReport,
  DiagnosticExportReport,
  RuntimeRecoveryStatus,
  UiPreferences,
} from "./application-runtime";
import {
  exportDiagnostics,
  getApplicationConfig,
  getRuntimeRecoveryStatus,
  resetDerivedState,
  updateApplicationConfig,
} from "./application-runtime";
import type {
  AddLibraryRootInput,
  LibraryRoot,
  LibraryRootStatus,
  UpdateLibraryRootInput,
} from "./library-roots";
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
import type { LibraryScanEvent } from "./scanner";
import { cancelLibraryScan, startLibraryScan } from "./scanner";
import type {
  CacheClearReport,
  ThumbnailOutcome,
  ThumbnailRequest,
} from "./thumbnail";
import {
  clearThumbnailCache,
  readThumbnail,
  requestThumbnail,
} from "./thumbnail";

export interface DesktopApi {
  getApplicationConfig(): Promise<ApplicationConfig>;
  updateApplicationConfig(input: UiPreferences): Promise<ApplicationConfig>;
  getRuntimeRecoveryStatus(): Promise<RuntimeRecoveryStatus>;
  resetDerivedState(): Promise<DerivedStateResetReport>;
  exportDiagnostics(): Promise<DiagnosticExportReport>;
  listLibraryRoots(): Promise<LibraryRootStatus[]>;
  addLibraryRoot(input: AddLibraryRootInput): Promise<LibraryRootStatus>;
  updateLibraryRoot(input: UpdateLibraryRootInput): Promise<LibraryRootStatus>;
  removeLibraryRoot(id: string): Promise<LibraryRoot>;
  startLibraryScan(
    rootId: string,
    receive: (event: LibraryScanEvent) => void,
  ): Promise<string>;
  cancelLibraryScan(scanId: string): Promise<boolean>;
  queryAssets(input: QueryAssetsInput): Promise<QueryAssetsResult>;
  editAssetMetadata(input: BatchMetadataEdit): Promise<BatchMetadataEditResult>;
  requestThumbnail(input: ThumbnailRequest): Promise<ThumbnailOutcome>;
  readThumbnail(cacheKey: string): Promise<ArrayBuffer>;
  clearThumbnailCache(): Promise<CacheClearReport>;
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
  resetDerivedState,
  exportDiagnostics,
  listLibraryRoots,
  addLibraryRoot,
  updateLibraryRoot,
  removeLibraryRoot,
  startLibraryScan,
  cancelLibraryScan,
  queryAssets,
  editAssetMetadata,
  requestThumbnail,
  readThumbnail,
  clearThumbnailCache,
  listObsidianVaults,
  addObsidianVault,
  updateObsidianVault,
  removeObsidianVault,
  resolveObsidianVaultReferences,
};

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
