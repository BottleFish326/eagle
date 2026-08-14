import { describe, expect, it, vi } from "vitest";

import {
  addObsidianVault,
  copyVaultReference,
  listObsidianVaults,
  referenceFailureLabel,
  removeObsidianVault,
  resolveObsidianVaultReferences,
  updateObsidianVault,
  type VaultReference,
  writeVaultReferenceDragData,
} from "./obsidian-vaults";

const reference: VaultReference = {
  assetKey: "/vault/brand/logo.png",
  vaultId: "0198a9b2-43c0-7cb0-a733-000000000001",
  vaultName: "Design Notes",
  assetPath: "/vault/brand/logo.png",
  relativePath: "brand/logo.png",
  urlEncodedPath: "brand/logo.png",
  markdown: "![[brand/logo.png]]",
};

describe("Obsidian Vault command wire", () => {
  it("uses stable Tauri command and argument names", async () => {
    const call = vi.fn().mockResolvedValue({});
    await listObsidianVaults(call);
    await addObsidianVault({ path: "/vault", name: "Vault" }, call);
    await updateObsidianVault({ id: "vault-id", enabled: false }, call);
    await removeObsidianVault("vault-id", call);
    await resolveObsidianVaultReferences(
      { vaultId: "vault-id", assetKeys: ["asset-key"] },
      call,
    );

    expect(call.mock.calls).toEqual([
      ["list_obsidian_vaults"],
      ["add_obsidian_vault", { input: { path: "/vault", name: "Vault" } }],
      ["update_obsidian_vault", { input: { id: "vault-id", enabled: false } }],
      ["remove_obsidian_vault", { id: "vault-id" }],
      [
        "resolve_obsidian_vault_references",
        { input: { vaultId: "vault-id", assetKeys: ["asset-key"] } },
      ],
    ]);
  });
});

describe("Obsidian copy and drag payloads", () => {
  it("copies only the portable WikiLink without an absolute path", async () => {
    const clipboard = { writeText: vi.fn(async () => undefined) };
    await copyVaultReference(reference, clipboard);
    expect(clipboard.writeText).toHaveBeenCalledWith("![[brand/logo.png]]");
    expect(clipboard.writeText).not.toHaveBeenCalledWith(
      expect.stringContaining("/vault"),
    );
  });

  it("writes text payloads that Obsidian can insert on drop", () => {
    const entries = new Map<string, string>();
    const store = {
      effectAllowed: "none",
      setData: (format: string, value: string) => entries.set(format, value),
    };
    writeVaultReferenceDragData(store, reference);

    expect(store.effectAllowed).toBe("copy");
    expect(entries.get("text/plain")).toBe("![[brand/logo.png]]");
    expect(entries.get("text/markdown")).toBe("![[brand/logo.png]]");
    expect(entries.get("application/x-material-eagle-obsidian-reference")).toBe(
      JSON.stringify({
        vaultId: reference.vaultId,
        relativePath: "brand/logo.png",
        markdown: "![[brand/logo.png]]",
      }),
    );
  });

  it("turns structured resolution failures into actionable labels", () => {
    expect(
      referenceFailureLabel({
        assetKey: "/outside/logo.png",
        kind: "outside-vault",
        message: "outside",
      }),
    ).toBe("该素材不在当前 Vault 内");
  });
});
