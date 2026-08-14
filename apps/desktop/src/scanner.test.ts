import { describe, expect, it, vi } from "vitest";

import {
  type LibraryScanEvent,
  cancelLibraryScan,
  startLibraryScan,
} from "./scanner";

describe("scanner commands", () => {
  it("streams ordered events over the command channel", async () => {
    const call = vi.fn().mockResolvedValue("scan-id");
    const channel = { onmessage: (_message: LibraryScanEvent) => undefined };
    const receive = vi.fn();

    const scanId = await startLibraryScan(
      "root-id",
      receive,
      call,
      () => channel,
    );
    const event: LibraryScanEvent = {
      event: "started",
      data: { scanId, rootId: "root-id", root: "/pictures" },
    };
    channel.onmessage(event);

    expect(scanId).toBe("scan-id");
    expect(call).toHaveBeenCalledWith("start_library_scan", {
      rootId: "root-id",
      onEvent: channel,
    });
    expect(receive).toHaveBeenCalledWith(event);
  });

  it("requests cooperative cancellation by scan id", async () => {
    const call = vi.fn().mockResolvedValue(true);

    await expect(cancelLibraryScan("scan-id", call)).resolves.toBe(true);
    expect(call).toHaveBeenCalledWith("cancel_library_scan", {
      scanId: "scan-id",
    });
  });
});
