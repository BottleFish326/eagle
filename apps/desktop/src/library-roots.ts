import { invoke } from "@tauri-apps/api/core";

export type RootAccessStatus =
  | "available"
  | "missing"
  | "not-directory"
  | "permission-denied"
  | "unavailable";

export interface RootScanSettings {
  recursive: boolean;
  followSymlinks: false;
  ignore: string[];
}

export interface LibraryRoot {
  id: string;
  path: string;
  name: string;
  enabled: boolean;
  scan: RootScanSettings;
}

export interface LibraryRootStatus extends LibraryRoot {
  accessStatus: RootAccessStatus;
  accessMessage?: string;
}

export interface AddLibraryRootInput {
  path: string;
  name: string;
  ignore?: string[];
}

export interface UpdateLibraryRootInput {
  id: string;
  name?: string;
  enabled?: boolean;
  ignore?: string[];
}

type Invoke = <T>(
  command: string,
  argumentsValue?: Record<string, unknown>,
) => Promise<T>;

export function listLibraryRoots(
  call: Invoke = invoke,
): Promise<LibraryRootStatus[]> {
  return call<LibraryRootStatus[]>("list_library_roots");
}

export function addLibraryRoot(
  input: AddLibraryRootInput,
  call: Invoke = invoke,
): Promise<LibraryRootStatus> {
  return call<LibraryRootStatus>("add_library_root", { input });
}

export function updateLibraryRoot(
  input: UpdateLibraryRootInput,
  call: Invoke = invoke,
): Promise<LibraryRootStatus> {
  return call<LibraryRootStatus>("update_library_root", { input });
}

export function removeLibraryRoot(
  id: string,
  call: Invoke = invoke,
): Promise<LibraryRoot> {
  return call<LibraryRoot>("remove_library_root", { id });
}
