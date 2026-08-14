import { invoke } from "@tauri-apps/api/core";
import { writeText as writeClipboardText } from "@tauri-apps/plugin-clipboard-manager";

export type VaultAccessStatus =
  | "available"
  | "missing"
  | "not-directory"
  | "permission-denied"
  | "unavailable";

export interface ObsidianVault {
  id: string;
  path: string;
  name: string;
  enabled: boolean;
}

export interface ObsidianVaultStatus extends ObsidianVault {
  accessStatus: VaultAccessStatus;
  accessMessage?: string;
}

export interface AddObsidianVaultInput {
  path: string;
  name: string;
}

export interface UpdateObsidianVaultInput {
  id: string;
  name?: string;
  enabled?: boolean;
}

export interface VaultReference {
  assetKey: string;
  vaultId: string;
  vaultName: string;
  assetPath: string;
  relativePath: string;
  urlEncodedPath: string;
  markdown: string;
}

export type VaultReferenceFailureKind =
  | "asset-not-found"
  | "vault-not-found"
  | "vault-disabled"
  | "vault-unavailable"
  | "asset-unavailable"
  | "outside-vault"
  | "unsafe-wikilink"
  | "internal";

export interface VaultReferenceFailure {
  assetKey: string;
  kind: VaultReferenceFailureKind;
  message: string;
}

export interface ResolveVaultReferencesInput {
  vaultId: string;
  assetKeys: string[];
}

export interface ResolveVaultReferencesResult {
  resolved: VaultReference[];
  failures: VaultReferenceFailure[];
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function listObsidianVaults(
  call: Invoke = invoke,
): Promise<ObsidianVaultStatus[]> {
  return call<ObsidianVaultStatus[]>("list_obsidian_vaults");
}

export function addObsidianVault(
  input: AddObsidianVaultInput,
  call: Invoke = invoke,
): Promise<ObsidianVaultStatus> {
  return call<ObsidianVaultStatus>("add_obsidian_vault", { input });
}

export function updateObsidianVault(
  input: UpdateObsidianVaultInput,
  call: Invoke = invoke,
): Promise<ObsidianVaultStatus> {
  return call<ObsidianVaultStatus>("update_obsidian_vault", { input });
}

export function removeObsidianVault(
  id: string,
  call: Invoke = invoke,
): Promise<ObsidianVault> {
  return call<ObsidianVault>("remove_obsidian_vault", { id });
}

export function resolveObsidianVaultReferences(
  input: ResolveVaultReferencesInput,
  call: Invoke = invoke,
): Promise<ResolveVaultReferencesResult> {
  return call<ResolveVaultReferencesResult>(
    "resolve_obsidian_vault_references",
    { input },
  );
}

interface TextClipboard {
  writeText(value: string): Promise<void>;
}

interface DragDataStore {
  effectAllowed: string;
  setData(format: string, data: string): void;
}

export async function copyVaultReference(
  reference: VaultReference,
  clipboard?: TextClipboard,
): Promise<void> {
  if (clipboard !== undefined) {
    await clipboard.writeText(reference.markdown);
    return;
  }
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    await writeClipboardText(reference.markdown);
    return;
  }
  if (typeof navigator === "undefined" || navigator.clipboard === undefined) {
    throw new Error("当前运行环境不支持剪贴板写入");
  }
  await navigator.clipboard.writeText(reference.markdown);
}

export function writeVaultReferenceDragData(
  store: DragDataStore,
  reference: VaultReference,
): void {
  store.effectAllowed = "copy";
  store.setData("text/plain", reference.markdown);
  store.setData("text/markdown", reference.markdown);
  store.setData(
    "application/x-material-eagle-obsidian-reference",
    JSON.stringify({
      vaultId: reference.vaultId,
      relativePath: reference.relativePath,
      markdown: reference.markdown,
    }),
  );
}

export function referenceFailureLabel(
  failure: VaultReferenceFailure | undefined,
): string | undefined {
  if (failure === undefined) return undefined;
  switch (failure.kind) {
    case "outside-vault":
      return "该素材不在当前 Vault 内";
    case "unsafe-wikilink":
      return "路径包含 Obsidian WikiLink 保留字符";
    case "asset-unavailable":
      return "素材文件当前不可访问";
    case "vault-disabled":
      return "当前 Vault 已停用";
    case "vault-unavailable":
      return "当前 Vault 不可访问";
    case "asset-not-found":
      return "素材尚未进入当前扫描目录";
    case "vault-not-found":
      return "Vault 配置已不存在";
    case "internal":
      return failure.message;
  }
}
