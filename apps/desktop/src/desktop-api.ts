import type { QueryAssetsInput, QueryAssetsResult } from "./asset-query";
import { queryAssets } from "./asset-query";
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
}

export const tauriDesktopApi: DesktopApi = {
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
};

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
