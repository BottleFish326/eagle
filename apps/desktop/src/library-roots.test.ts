import { describe, expect, it, vi } from "vitest";

import {
  addLibraryRoot,
  markLibraryRootAccessFailure,
  removeLibraryRoot,
  updateLibraryRoot,
} from "./library-roots";

describe("library root commands", () => {
  it("sends a complete add request through one typed input", async () => {
    const call = vi.fn().mockResolvedValue({ id: "root-id" });

    await addLibraryRoot(
      { path: "/pictures", name: "Pictures", ignore: ["temp/**"] },
      call,
    );

    expect(call).toHaveBeenCalledWith("add_library_root", {
      input: { path: "/pictures", name: "Pictures", ignore: ["temp/**"] },
    });
  });

  it("can disable a root and remove only its configuration record", async () => {
    const call = vi.fn().mockResolvedValue({ id: "root-id" });

    await updateLibraryRoot({ id: "root-id", enabled: false }, call);
    await removeLibraryRoot("root-id", call);

    expect(call).toHaveBeenNthCalledWith(1, "update_library_root", {
      input: { id: "root-id", enabled: false },
    });
    expect(call).toHaveBeenNthCalledWith(2, "remove_library_root", {
      id: "root-id",
    });
  });

  it("marks only a disconnected root offline without removing its configuration", () => {
    const roots = [
      {
        id: "root-id",
        path: "/pictures",
        name: "Pictures",
        enabled: true,
        scan: { recursive: true, followSymlinks: false as const, ignore: [] },
        accessStatus: "available" as const,
      },
      {
        id: "other-id",
        path: "/other",
        name: "Other",
        enabled: true,
        scan: { recursive: true, followSymlinks: false as const, ignore: [] },
        accessStatus: "available" as const,
      },
    ];

    const updated = markLibraryRootAccessFailure(
      roots,
      "root-id",
      "permission-denied",
      "access denied",
    );

    expect(updated).toHaveLength(2);
    expect(updated[0]).toMatchObject({
      id: "root-id",
      accessStatus: "permission-denied",
      accessMessage: "access denied",
    });
    expect(updated[1]).toBe(roots[1]);
  });
});
