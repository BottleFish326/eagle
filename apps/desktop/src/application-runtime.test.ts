import { describe, expect, it, vi } from "vitest";

import {
  cacheStartupLabel,
  copyLocalPath,
  exportDiagnostics,
  getApplicationConfig,
  getRuntimeRecoveryStatus,
  resetDerivedState,
  updateApplicationConfig,
} from "./application-runtime";

describe("application recovery command wire", () => {
  it("uses stable Tauri command and input names", async () => {
    const call = vi.fn().mockResolvedValue({});
    await getApplicationConfig(call);
    await updateApplicationConfig(
      {
        query: "favorite:true",
        tagFilters: { "state/draft": "exclude" },
        activeVaultId: null,
      },
      call,
    );
    await getRuntimeRecoveryStatus(call);
    await resetDerivedState(call);
    await exportDiagnostics(call);

    expect(call.mock.calls).toEqual([
      ["get_application_config"],
      [
        "update_application_config",
        {
          input: {
            query: "favorite:true",
            tagFilters: { "state/draft": "exclude" },
            activeVaultId: null,
          },
        },
      ],
      ["runtime_recovery_status"],
      ["reset_derived_state"],
      ["export_diagnostics"],
    ]);
  });

  it("copies an exported path through the injected clipboard", async () => {
    const clipboard = { writeText: vi.fn().mockResolvedValue(undefined) };
    await copyLocalPath("/logs/diagnostic.json", clipboard);
    expect(clipboard.writeText).toHaveBeenCalledWith("/logs/diagnostic.json");
  });

  it("describes automatic incompatible cache recovery", () => {
    expect(
      cacheStartupLabel({
        disposition: "rebuilt-incompatible",
        removedFiles: 42,
        removedBytes: 8192,
      }),
    ).toBe("发现旧版缓存，已自动重建并移除 42 项");
  });
});
