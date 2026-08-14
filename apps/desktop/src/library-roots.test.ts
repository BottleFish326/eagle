import { describe, expect, it, vi } from "vitest";

import {
  addLibraryRoot,
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
});
