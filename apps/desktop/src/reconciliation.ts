import { invoke } from "@tauri-apps/api/core";

export type OrphanSidecarState = "ready" | "missing-fingerprint" | "invalid";

export interface OrphanSidecar {
  sidecarId: string | null;
  sidecarPath: string;
  expectedAssetPath: string;
  state: OrphanSidecarState;
  message: string | null;
  candidateCount: number;
}

export interface MissingAsset {
  sidecarId: string | null;
  expectedAssetPath: string;
  sidecarPath: string;
}

export interface RelinkCandidate {
  candidateId: string;
  rootId: string;
  sidecarId: string;
  sidecarPath: string;
  sidecarDigest: string;
  assetKey: string;
  assetPath: string;
  size: number;
  quickFingerprint: string | null;
  sha256: string;
  ambiguous: boolean;
}

export interface ReconciliationReport {
  rootId: string;
  orphanSidecars: OrphanSidecar[];
  missingAssets: MissingAsset[];
  pendingMoves: RelinkCandidate[];
}

export interface RelinkReceipt {
  candidateId: string;
  sidecarId: string;
  from: string;
  to: string;
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function inspectLibraryReconciliation(
  rootId: string,
  call: Invoke = invoke,
): Promise<ReconciliationReport> {
  return call<ReconciliationReport>("inspect_library_reconciliation", {
    rootId,
  });
}

export function confirmLibraryRelink(
  candidateId: string,
  call: Invoke = invoke,
): Promise<RelinkReceipt> {
  return call<RelinkReceipt>("confirm_library_relink", { candidateId });
}
