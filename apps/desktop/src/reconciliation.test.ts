import { describe, expect, it, vi } from "vitest";

import {
  confirmLibraryRelink,
  inspectLibraryReconciliation,
} from "./reconciliation";

describe("reconciliation commands", () => {
  it("inspects only a configured root id without accepting a client path", async () => {
    const call = vi.fn().mockResolvedValue({
      rootId: "root-id",
      orphanSidecars: [],
      missingAssets: [],
      pendingMoves: [],
      syncConflictCopies: [],
    });

    await inspectLibraryReconciliation("root-id", call);

    expect(call).toHaveBeenCalledWith("inspect_library_reconciliation", {
      rootId: "root-id",
    });
  });

  it("confirms an opaque server candidate instead of sending file paths", async () => {
    const call = vi.fn().mockResolvedValue({
      candidateId: "candidate-id",
      sidecarId: "sidecar-id",
      from: "/library/old.png.asset.yml",
      to: "/library/new.png.asset.yml",
    });

    await confirmLibraryRelink("candidate-id", call);

    expect(call).toHaveBeenCalledWith("confirm_library_relink", {
      candidateId: "candidate-id",
    });
  });
});
