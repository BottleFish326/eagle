import { useEffect, useRef, useState } from "react";

import { Icon } from "./Icon";
import type { LibraryRootStatus } from "./library-roots";
import type { ReconciliationReport, RelinkCandidate } from "./reconciliation";

export function RootManager({
  open,
  roots,
  busy,
  onClose,
  onAdd,
  onToggle,
  onRemove,
  onScan,
  onConfirmRelink,
  reconciliationReports,
  relinkBusy,
  scanningRootIds,
}: {
  open: boolean;
  roots: readonly LibraryRootStatus[];
  busy: boolean;
  onClose: () => void;
  onAdd: (path: string, name: string) => Promise<void>;
  onToggle: (root: LibraryRootStatus) => Promise<void>;
  onRemove: (root: LibraryRootStatus) => Promise<void>;
  onScan: (root: LibraryRootStatus) => Promise<void>;
  onConfirmRelink: (candidate: RelinkCandidate) => Promise<void>;
  reconciliationReports: Readonly<Record<string, ReconciliationReport>>;
  relinkBusy?: string;
  scanningRootIds: ReadonlySet<string>;
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
          {roots.map((root) => {
            const report = reconciliationReports[root.id];
            return (
              <div className="root-entry" key={root.id}>
                <article className="root-row">
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
                    aria-label={`完整一致性扫描 ${root.name}`}
                    className="root-action"
                    disabled={
                      busy ||
                      scanningRootIds.has(root.id) ||
                      !root.enabled ||
                      root.accessStatus !== "available"
                    }
                    onClick={() => void onScan(root)}
                    title="执行完整一致性扫描"
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
                      confirmRemove === root.id
                        ? "再次点击确认移除"
                        : "移除配置"
                    }
                    type="button"
                  >
                    <Icon name="trash" size={15} />
                  </button>
                </article>
                {report !== undefined &&
                (report.orphanSidecars.length > 0 ||
                  report.missingAssets.length > 0 ||
                  report.pendingMoves.length > 0 ||
                  report.syncConflictCopies.length > 0) ? (
                  <ReconciliationPanel
                    busy={relinkBusy}
                    onConfirm={onConfirmRelink}
                    report={report}
                    scanActive={scanningRootIds.has(root.id)}
                  />
                ) : null}
              </div>
            );
          })}
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

function ReconciliationPanel({
  report,
  busy,
  onConfirm,
  scanActive,
}: {
  report: ReconciliationReport;
  busy?: string;
  onConfirm: (candidate: RelinkCandidate) => Promise<void>;
  scanActive: boolean;
}) {
  return (
    <section className="reconciliation-panel" aria-label="移动与孤立文件诊断">
      <div className="reconciliation-summary">
        <strong>需要人工核对</strong>
        <span>孤立 Sidecar {report.orphanSidecars.length}</span>
        <span>丢失素材 {report.missingAssets.length}</span>
        <span>待确认移动 {report.pendingMoves.length}</span>
        <span>同步冲突副本 {report.syncConflictCopies.length}</span>
      </div>
      {report.syncConflictCopies.map((conflict) => (
        <div
          className="reconciliation-item reconciliation-item--conflict"
          key={conflict.path}
        >
          <div>
            <small>{syncConflictSourceLabel(conflict.source)}</small>
            <strong title={conflict.path}>{fileName(conflict.path)}</strong>
            <span>
              {conflict.modifiedUnixMs === null
                ? "修改时间未知"
                : formatSyncConflictTime(conflict.modifiedUnixMs)}
              {conflict.differingFields.length > 0
                ? ` · 差异：${conflict.differingFields
                    .map(conflictFieldLabel)
                    .join("、")}`
                : " · 尚无可比较字段"}
            </span>
            {conflict.originalSidecarPath ? (
              <span title={conflict.originalSidecarPath}>
                原文件：{fileName(conflict.originalSidecarPath)}
              </span>
            ) : null}
            {conflict.message ? <em>{conflict.message}</em> : null}
          </div>
          <span className="reconciliation-state">仅诊断，不自动合并或删除</span>
        </div>
      ))}
      {report.orphanSidecars.map((orphan) => (
        <div className="reconciliation-item" key={orphan.sidecarPath}>
          <div>
            <small>孤立 Sidecar</small>
            <strong title={orphan.sidecarPath}>
              {fileName(orphan.sidecarPath)}
            </strong>
            <span title={orphan.expectedAssetPath}>
              丢失素材：{fileName(orphan.expectedAssetPath)}
            </span>
            {orphan.message ? <em>{orphan.message}</em> : null}
          </div>
          {orphan.candidateCount === 0 ? (
            <span className="reconciliation-state">暂无安全候选</span>
          ) : null}
        </div>
      ))}
      {report.pendingMoves.map((candidate) => (
        <div className="reconciliation-item" key={candidate.candidateId}>
          <div>
            <small>
              {candidate.ambiguous ? "多个相同候选" : "SHA-256 已确认"}
            </small>
            <strong title={candidate.assetPath}>
              {fileName(candidate.assetPath)}
            </strong>
            <span>
              确认后只把 Sidecar 移到该素材旁；不会覆盖目标或修改原素材。
            </span>
          </div>
          <button
            className="secondary-button"
            disabled={busy !== undefined || scanActive}
            onClick={() => void onConfirm(candidate)}
            type="button"
          >
            {busy === candidate.candidateId
              ? "正在重新验证…"
              : candidate.ambiguous
                ? "选择此候选"
                : "确认重新关联"}
          </button>
        </div>
      ))}
    </section>
  );
}

function fileName(path: string): string {
  return path.split(/[\\/]/).at(-1) ?? path;
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

function syncConflictSourceLabel(
  source: "dropbox" | "syncthing" | "other",
): string {
  switch (source) {
    case "dropbox":
      return "Dropbox 冲突副本";
    case "syncthing":
      return "Syncthing 冲突副本";
    case "other":
      return "同步冲突副本";
  }
}

function conflictFieldLabel(
  field: "tags" | "rating" | "favorite" | "note" | "aliases",
): string {
  switch (field) {
    case "tags":
      return "Tag";
    case "rating":
      return "评分";
    case "favorite":
      return "收藏";
    case "note":
      return "备注";
    case "aliases":
      return "别名";
  }
}

function formatSyncConflictTime(value: number): string {
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(value);
}
