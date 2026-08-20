import { Channel, invoke } from "@tauri-apps/api/core";

import type { MetadataPatch } from "./metadata-editor";
import type { MetadataTransactionSummary } from "./metadata-transactions";
import type { AssetRecord } from "./scanner";

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

export interface BatchPreflightConfirmation {
  operationId: string;
  snapshotId: string;
  catalogRevision: number;
  requestedCount: number;
  executableCount: number;
  confirmationDigest: string;
}

export interface BatchExecutionProgress {
  total: number;
  currentSequence: number;
  plannedCount: number;
  appliedCount: number;
  failedCount: number;
  conflictCount: number;
}

export type BatchExecutionEvent = {
  event: "progress";
  data: { operationId: string; progress: BatchExecutionProgress };
};

export interface BatchExecutionFailure {
  key: string;
  kind: "conflict" | "invalid-input" | "write-failed";
  message: string;
}

export interface BatchMetadataExecutionResult {
  operationId: string;
  transaction: MetadataTransactionSummary;
  updated: AssetRecord[];
  failures: BatchExecutionFailure[];
  stopped: boolean;
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

interface BatchChannel {
  onmessage: (message: BatchExecutionEvent) => void;
}

type ChannelFactory = () => BatchChannel;

export function executeMetadataBatch(
  confirmation: BatchPreflightConfirmation,
  receive: (event: BatchExecutionEvent) => void,
  call: Invoke = invoke,
  createChannel: ChannelFactory = () => new Channel<BatchExecutionEvent>(),
): Promise<BatchMetadataExecutionResult> {
  const onEvent = createChannel();
  onEvent.onmessage = receive;
  return call("execute_metadata_batch", { confirmation, onEvent });
}

export function cancelMetadataBatch(
  operationId: string,
  call: Invoke = invoke,
): Promise<boolean> {
  return call("cancel_metadata_batch", { operationId });
}

export function preflightConfirmation(
  summary: BatchPreflightSummary,
): BatchPreflightConfirmation {
  return {
    operationId: summary.operationId,
    snapshotId: summary.snapshotId,
    catalogRevision: summary.catalogRevision,
    requestedCount: summary.requestedCount,
    executableCount: summary.executableCount,
    confirmationDigest: summary.confirmationDigest,
  };
}
