import { invoke } from "@tauri-apps/api/core";

import type { MetadataPatch } from "./metadata-editor";

export type BatchFailureKind =
  | "asset-missing"
  | "asset-moved-ambiguous"
  | "root-disabled"
  | "root-offline"
  | "authorization-lost"
  | "source-changed"
  | "sidecar-conflict";

export interface BatchPreflightFailure {
  key: string;
  kind: BatchFailureKind;
  message: string;
}

export interface MetadataPreflightInput {
  snapshotId: string;
  patch: MetadataPatch;
}

export interface BatchPreflightSummary {
  operationId: string;
  snapshotId: string;
  catalogRevision: number;
  requestedCount: number;
  executableCount: number;
  requiresStableIdCount: number;
  unavailableCount: number;
  conflictCount: number;
  failureCount: number;
  failuresTruncated: boolean;
  failures: BatchPreflightFailure[];
  confirmationDigest: string;
  createdAt: string;
  expiresAt: string;
}

export type BatchCommandErrorKind =
  | "snapshot-not-found"
  | "snapshot-expired"
  | "invalid-operation"
  | "output-too-large"
  | "preflight-not-found"
  | "preflight-expired"
  | "preflight-stale"
  | "internal";

export interface BatchCommandError {
  kind: BatchCommandErrorKind;
  message: string;
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function prepareMetadataBatch(
  input: MetadataPreflightInput,
  call: Invoke = invoke,
): Promise<BatchPreflightSummary> {
  return call("prepare_metadata_batch", { input });
}

export function releaseBatchPreflight(
  operationId: string,
  call: Invoke = invoke,
): Promise<boolean> {
  return call("release_batch_preflight", { operationId });
}
