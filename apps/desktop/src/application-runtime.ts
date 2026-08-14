import { invoke } from "@tauri-apps/api/core";
import { writeText as writeClipboardText } from "@tauri-apps/plugin-clipboard-manager";

export type SavedTagFilterState = "include" | "exclude";

export interface UiPreferences {
  query: string;
  tagFilters: Record<string, SavedTagFilterState>;
  activeVaultId: string | null;
}

export interface ApplicationConfig {
  schema: number;
  ui: UiPreferences;
}

export interface ApplicationPaths {
  configDirectory: string;
  cacheDirectory: string;
  logDirectory: string;
}

export type CacheStartupDisposition =
  "created" | "reused" | "rebuilt-missing-marker" | "rebuilt-incompatible";

export interface CacheStartupReport {
  disposition: CacheStartupDisposition;
  removedFiles: number;
  removedBytes: number;
}

export interface RuntimeRecoveryStatus {
  paths: ApplicationPaths;
  cacheStartup: CacheStartupReport;
}

export interface DerivedStateResetReport {
  cache: {
    removedFiles: number;
    removedBytes: number;
  };
  catalogAssetsRemoved: number;
}

export interface DiagnosticExportReport {
  path: string;
  generatedAt: string;
  eventCount: number;
  sizeBytes: number;
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function getApplicationConfig(
  call: Invoke = invoke,
): Promise<ApplicationConfig> {
  return call<ApplicationConfig>("get_application_config");
}

export function updateApplicationConfig(
  input: UiPreferences,
  call: Invoke = invoke,
): Promise<ApplicationConfig> {
  return call<ApplicationConfig>("update_application_config", { input });
}

export function getRuntimeRecoveryStatus(
  call: Invoke = invoke,
): Promise<RuntimeRecoveryStatus> {
  return call<RuntimeRecoveryStatus>("runtime_recovery_status");
}

export function resetDerivedState(
  call: Invoke = invoke,
): Promise<DerivedStateResetReport> {
  return call<DerivedStateResetReport>("reset_derived_state");
}

export function exportDiagnostics(
  call: Invoke = invoke,
): Promise<DiagnosticExportReport> {
  return call<DiagnosticExportReport>("export_diagnostics");
}

interface TextClipboard {
  writeText(value: string): Promise<void>;
}

export async function copyLocalPath(
  path: string,
  clipboard?: TextClipboard,
): Promise<void> {
  if (clipboard !== undefined) {
    await clipboard.writeText(path);
    return;
  }
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    await writeClipboardText(path);
    return;
  }
  if (typeof navigator === "undefined" || navigator.clipboard === undefined) {
    throw new Error("当前运行环境不支持剪贴板写入");
  }
  await navigator.clipboard.writeText(path);
}

export function cacheStartupLabel(report: CacheStartupReport): string {
  switch (report.disposition) {
    case "created":
      return "已创建兼容缓存";
    case "reused":
      return "缓存版本兼容，已复用";
    case "rebuilt-missing-marker":
      return `发现无版本缓存，已自动重建并移除 ${report.removedFiles} 项`;
    case "rebuilt-incompatible":
      return `发现旧版缓存，已自动重建并移除 ${report.removedFiles} 项`;
  }
}
