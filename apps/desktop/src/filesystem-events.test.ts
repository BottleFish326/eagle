import { describe, expect, it, vi } from "vitest";

import {
  type LibraryWatchEvent,
  startLibraryWatch,
  stopLibraryWatch,
} from "./filesystem-events";

describe("filesystem watch commands", () => {
  it("streams normalized root-scoped batches", async () => {
    const call = vi.fn().mockResolvedValue("watch-id");
    const channel = { onmessage: (_message: LibraryWatchEvent) => undefined };
    const receive = vi.fn();

    await expect(
      startLibraryWatch("root-id", receive, call, () => channel),
    ).resolves.toBe("watch-id");
    const event: LibraryWatchEvent = {
      event: "changes",
      data: {
        watchId: "watch-id",
        rootId: "root-id",
        batch: {
          root: "/pictures",
          rawEventCount: 4,
          changes: [
            { kind: "modify", paths: ["/pictures/logo.png.asset.yml"] },
          ],
        },
      },
    };
    channel.onmessage(event);

    expect(call).toHaveBeenCalledWith("start_library_watch", {
      rootId: "root-id",
      onEvent: channel,
    });
    expect(receive).toHaveBeenCalledWith(event);
  });

  it("stops one watcher without accepting a path", async () => {
    const call = vi.fn().mockResolvedValue(true);

    await expect(stopLibraryWatch("watch-id", call)).resolves.toBe(true);
    expect(call).toHaveBeenCalledWith("stop_library_watch", {
      watchId: "watch-id",
    });
  });
});
