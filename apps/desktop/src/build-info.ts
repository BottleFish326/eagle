import { invoke } from "@tauri-apps/api/core";

export interface BuildInfo {
  version: string;
  gitCommit: string;
  buildTarget: string;
  buildProfile: string;
  rustcVersion: string;
}

type Invoke = (command: string) => Promise<unknown>;

const WEB_PREVIEW_INFO: BuildInfo = {
  version: "0.1.0",
  gitCommit: "web-preview",
  buildTarget: "browser",
  buildProfile: "development",
  rustcVersion: "not-available",
};

export async function loadBuildInfo(call: Invoke = invoke): Promise<BuildInfo> {
  try {
    const value = await call("build_info");
    return isBuildInfo(value) ? value : WEB_PREVIEW_INFO;
  } catch {
    return WEB_PREVIEW_INFO;
  }
}

function isBuildInfo(value: unknown): value is BuildInfo {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Partial<BuildInfo>;
  return [
    candidate.version,
    candidate.gitCommit,
    candidate.buildTarget,
    candidate.buildProfile,
    candidate.rustcVersion,
  ].every((field) => typeof field === "string" && field.length > 0);
}
