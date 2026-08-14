import { useEffect, useRef, useState } from "react";

import { Icon } from "./Icon";
import type { LibraryRootStatus } from "./library-roots";

export function RootManager({
  open,
  roots,
  busy,
  onClose,
  onAdd,
  onToggle,
  onRemove,
  onScan,
}: {
  open: boolean;
  roots: readonly LibraryRootStatus[];
  busy: boolean;
  onClose: () => void;
  onAdd: (path: string, name: string) => Promise<void>;
  onToggle: (root: LibraryRootStatus) => Promise<void>;
  onRemove: (root: LibraryRootStatus) => Promise<void>;
  onScan: (root: LibraryRootStatus) => Promise<void>;
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
        aria-labelledby="library-dialog-title"
        aria-modal="true"
        className="library-dialog"
        onMouseDown={(event) => event.stopPropagation()}
        role="dialog"
      >
        <header className="dialog-heading">
          <div>
            <span className="overline">Library sources</span>
            <h2 id="library-dialog-title">素材根目录</h2>
            <p>应用只读取这些位置；移除配置不会移动或删除磁盘文件。</p>
          </div>
          <button
            aria-label="关闭素材根目录"
            className="icon-button"
            onClick={onClose}
            ref={closeButton}
            type="button"
          >
            <Icon name="close" />
          </button>
        </header>

        <div className="root-list">
          {roots.map((root) => (
            <article className="root-row" key={root.id}>
              <span
                className={`root-status root-status--${root.accessStatus}`}
                aria-hidden="true"
              />
              <div className="root-copy">
                <strong>{root.name}</strong>
                <span title={root.path}>{root.path}</span>
                {root.accessMessage ? (
                  <small>{root.accessMessage}</small>
                ) : null}
              </div>
              <span className="root-access">{accessLabel(root)}</span>
              <button
                className="root-action"
                disabled={
                  busy || !root.enabled || root.accessStatus !== "available"
                }
                onClick={() => void onScan(root)}
                title="重新扫描"
                type="button"
              >
                <Icon name="refresh" size={15} />
              </button>
              <button
                aria-label={
                  root.enabled ? `停用 ${root.name}` : `启用 ${root.name}`
                }
                aria-pressed={root.enabled}
                className={`toggle${root.enabled ? " is-on" : ""}`}
                disabled={busy}
                onClick={() => void onToggle(root)}
                type="button"
              >
                <span />
              </button>
              <button
                className={`root-action${confirmRemove === root.id ? " root-action--danger" : ""}`}
                disabled={busy}
                onClick={() => {
                  if (confirmRemove === root.id) {
                    void onRemove(root)
                      .then(() => setConfirmRemove(undefined))
                      .catch(() => undefined);
                  } else {
                    setConfirmRemove(root.id);
                  }
                }}
                title={
                  confirmRemove === root.id ? "再次点击确认移除" : "移除配置"
                }
                type="button"
              >
                <Icon name="trash" size={15} />
              </button>
            </article>
          ))}
          {roots.length === 0 ? (
            <div className="root-empty">
              <Icon name="folder" />
              <p>尚未配置素材位置。</p>
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
            <span className="overline">Add source</span>
            <h3>添加现有文件夹</h3>
          </div>
          <label>
            显示名称
            <input
              autoComplete="off"
              disabled={busy}
              onChange={(event) => setName(event.target.value)}
              placeholder="例如 品牌素材"
              value={name}
            />
          </label>
          <label className="path-field">
            绝对路径
            <input
              autoComplete="off"
              disabled={busy}
              onChange={(event) => setPath(event.target.value)}
              placeholder="/Users/name/Pictures/Assets"
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
            <Icon name="plus" size={16} /> 添加并扫描
          </button>
        </form>
      </section>
    </div>
  );
}

function accessLabel(root: LibraryRootStatus): string {
  if (!root.enabled) return "已停用";
  switch (root.accessStatus) {
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
