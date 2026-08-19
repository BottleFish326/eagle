import { invoke } from "@tauri-apps/api/core";

export type SupportSeverity = "warning" | "error";

export interface ConsistencyFinding {
  severity: SupportSeverity;
  code: string;
  rootId: string | null;
  assetId: string | null;
  relativePath: string | null;
  pathFingerprint: string | null;
  message: string;
}

export interface RootConsistencySummary {
  rootId: string;
  enabled: boolean;
  accessStatus:
    | "available"
    | "missing"
    | "not-directory"
    | "permission-denied"
    | "unavailable";
  catalogAssets: number;
  warnings: number;
  errors: number;
}

export interface LibraryConsistencyReport {
  generatedUnixMs: number;
  authoritative: boolean;
  summary: {
    configuredRoots: number;
    catalogAssets: number;
    findings: number;
    warnings: number;
    errors: number;
  };
  roots: RootConsistencySummary[];
  findings: ConsistencyFinding[];
  truncated: boolean;
}

export type TraceOutcome = "passed" | "warning" | "error";

export interface AssetTraceStep {
  matchIndex: number | null;
  stage: string;
  outcome: TraceOutcome;
  code: string;
  message: string;
}

export interface AssetTraceMatch {
  rootId: string | null;
  rootAccessStatus: RootConsistencySummary["accessStatus"] | null;
  relativePath: string;
  pathFingerprint: string;
  assetPresent: boolean;
  sidecarPresent: boolean;
  sidecarIdMatches: boolean | null;
  mime: string;
  issueCodes: string[];
}

export interface AssetTraceReport {
  generatedUnixMs: number;
  assetId: string;
  matchCount: number;
  matches: AssetTraceMatch[];
  steps: AssetTraceStep[];
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function inspectLibraryConsistency(
  call: Invoke = invoke,
): Promise<LibraryConsistencyReport> {
  return call<LibraryConsistencyReport>("inspect_library_consistency");
}

export function traceAssetSupport(
  assetId: string,
  call: Invoke = invoke,
): Promise<AssetTraceReport> {
  return call<AssetTraceReport>("trace_asset_support", { assetId });
}
