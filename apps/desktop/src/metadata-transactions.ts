import { invoke } from "@tauri-apps/api/core";

export type MetadataTransactionState =
  "active" | "completed" | "restored" | "conflict";

export interface MetadataTransactionSummary {
  id: string;
  state: MetadataTransactionState;
  createdAt: string;
  updatedAt: string;
  itemCount: number;
  plannedCount: number;
  appliedCount: number;
  failedCount: number;
  conflictCount: number;
  restoredCount: number;
  rootIds: string[];
}

export interface MetadataTransactionFailure {
  key: string;
  kind: "conflict" | "invalid-input" | "write-failed";
  message: string;
}

export interface MetadataTransactionRecoveryResult {
  summary: MetadataTransactionSummary;
  failures: MetadataTransactionFailure[];
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function listMetadataTransactions(
  call: Invoke = invoke,
): Promise<MetadataTransactionSummary[]> {
  return call<MetadataTransactionSummary[]>("list_metadata_transactions");
}

export function continueMetadataTransaction(
  id: string,
  call: Invoke = invoke,
): Promise<MetadataTransactionRecoveryResult> {
  return call<MetadataTransactionRecoveryResult>(
    "continue_metadata_transaction",
    { id },
  );
}

export function restoreMetadataTransaction(
  id: string,
  call: Invoke = invoke,
): Promise<MetadataTransactionRecoveryResult> {
  return call<MetadataTransactionRecoveryResult>(
    "restore_metadata_transaction",
    { id },
  );
}

export function dismissMetadataTransaction(
  id: string,
  call: Invoke = invoke,
): Promise<void> {
  return call<void>("dismiss_metadata_transaction", { id });
}
