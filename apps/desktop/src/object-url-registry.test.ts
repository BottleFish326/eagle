import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  createTrackedObjectUrl,
  objectUrlSnapshot,
  resetObjectUrlRegistryForTests,
  revokeTrackedObjectUrl,
} from "./object-url-registry";

describe("thumbnail object URL registry", () => {
  beforeEach(resetObjectUrlRegistryForTests);

  it("tracks active and peak URLs and revokes each URL once", () => {
    let next = 0;
    const create = () => `blob:test-${String(++next)}`;
    const revoke = vi.fn();
    const first = createTrackedObjectUrl(new Blob(), create);
    const second = createTrackedObjectUrl(new Blob(), create);

    revokeTrackedObjectUrl(first, revoke);
    revokeTrackedObjectUrl(first, revoke);

    expect(objectUrlSnapshot()).toEqual({
      active: 1,
      peakActive: 2,
      created: 2,
      revoked: 1,
    });
    expect(revoke).toHaveBeenCalledTimes(1);

    revokeTrackedObjectUrl(second, revoke);
    expect(objectUrlSnapshot().active).toBe(0);
  });
});
