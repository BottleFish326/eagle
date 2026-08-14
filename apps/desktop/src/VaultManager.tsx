import { useEffect, useRef, useState } from "react";

import { Icon } from "./Icon";
import type { ObsidianVaultStatus } from "./obsidian-vaults";

export function VaultManager({
  open,
  vaults,
  activeVaultId,
  busy,
  onClose,
  onAdd,
  onToggle,
  onRemove,
  onSelect,
}: {
  open: boolean;
  vaults: readonly ObsidianVaultStatus[];
  activeVaultId?: string;
  busy: boolean;
  onClose: () => void;
  onAdd: (path: string, name: string) => Promise<void>;
  onToggle: (vault: ObsidianVaultStatus) => Promise<void>;
  onRemove: (vault: ObsidianVaultStatus) => Promise<void>;
  onSelect: (id: string) => void;
}) {
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [confirmRemove, setConfirmRemove] = useState<string>();
  const closeButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (open) closeButton.current?.focus();
    else setConfirmRemove(undefined);
  }, [open]);

  if (!open) return null;

  const submit = async () => {
    if (path.trim().length === 0 || name.trim().length === 0) return;
    try {
      await onAdd(path.trim(), name.trim());
      setPath("");
      setName("");
    } catch {
      // The application-level notice keeps the actionable error visible.
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose} role="presentation">
      <section
        aria-labelledby="vault-dialog-title"
        aria-modal="true"
        className="library-dialog"
        onMouseDown={(event) => event.stopPropagation()}
        role="dialog"
      >
        <header className="dialog-heading">
          <div>
            <span className="overline">Obsidian integration</span>
            <h2 id="vault-dialog-title">目标 Vault</h2>
            <p>
              仅保存 Vault
              根路径。引用始终指向原文件，不复制素材，也不写入笔记。
            </p>
          </div>
          <button
            aria-label="关闭 Vault 配置"
            className="icon-button"
            onClick={onClose}
            ref={closeButton}
            type="button"
          >
            <Icon name="close" />
          </button>
        </header>

        <div className="root-list">
          {vaults.map((vault) => (
            <article
              className={`root-row vault-row${vault.id === activeVaultId ? " is-active" : ""}`}
              key={vault.id}
            >
              <span
                aria-hidden="true"
                className={`root-status root-status--${vault.accessStatus}`}
              />
              <button
                className="vault-select"
                disabled={
                  busy || !vault.enabled || vault.accessStatus !== "available"
                }
                onClick={() => onSelect(vault.id)}
                type="button"
              >
                <strong>{vault.name}</strong>
                <span title={vault.path}>{vault.path}</span>
                {vault.accessMessage ? (
                  <small>{vault.accessMessage}</small>
                ) : null}
              </button>
              <span className="root-access">
                {vault.id === activeVaultId
                  ? "当前目标"
                  : vaultAccessLabel(vault)}
              </span>
              <button
                aria-label={
                  vault.enabled ? `停用 ${vault.name}` : `启用 ${vault.name}`
                }
                aria-pressed={vault.enabled}
                className={`toggle${vault.enabled ? " is-on" : ""}`}
                disabled={busy}
                onClick={() => void onToggle(vault)}
                type="button"
              >
                <span />
              </button>
              <button
                className={`root-action${confirmRemove === vault.id ? " root-action--danger" : ""}`}
                disabled={busy}
                onClick={() => {
                  if (confirmRemove === vault.id) {
                    void onRemove(vault)
                      .then(() => setConfirmRemove(undefined))
                      .catch(() => undefined);
                  } else {
                    setConfirmRemove(vault.id);
                  }
                }}
                title={
                  confirmRemove === vault.id
                    ? "再次点击确认移除"
                    : "移除 Vault 配置"
                }
                type="button"
              >
                <Icon name="trash" size={15} />
              </button>
            </article>
          ))}
          {vaults.length === 0 ? (
            <div className="root-empty">
              <Icon name="link" />
              <p>尚未配置 Obsidian Vault。</p>
            </div>
          ) : null}
        </div>

        <form
          className="add-root-form"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
        >
          <div className="form-heading">
            <span className="overline">Add Vault</span>
            <h3>添加 Vault 根目录</h3>
          </div>
          <label>
            显示名称
            <input
              autoComplete="off"
              disabled={busy}
              onChange={(event) => setName(event.target.value)}
              placeholder="例如 设计知识库"
              value={name}
            />
          </label>
          <label className="path-field">
            绝对路径
            <input
              autoComplete="off"
              disabled={busy}
              onChange={(event) => setPath(event.target.value)}
              placeholder="/Users/name/Documents/Notes"
              value={path}
            />
          </label>
          <button
            className="primary-button"
            disabled={
              busy || path.trim().length === 0 || name.trim().length === 0
            }
            type="submit"
          >
            <Icon name="plus" size={16} /> 添加 Vault
          </button>
        </form>
      </section>
    </div>
  );
}

function vaultAccessLabel(vault: ObsidianVaultStatus): string {
  if (!vault.enabled) return "已停用";
  switch (vault.accessStatus) {
    case "available":
      return "可访问";
    case "missing":
      return "位置丢失";
    case "not-directory":
      return "不是文件夹";
    case "permission-denied":
      return "权限不足";
    case "unavailable":
      return "当前不可用";
  }
}
