import { useEffect, useState } from "react";

import { Icon } from "./Icon";
import type { MetadataPatch } from "./metadata-editor";
import type {
  FieldConflictResolution,
  MetadataConflict,
  MetadataConflictField,
  MetadataConflictResolution,
  TagConflictResolution,
  UserMetadataSnapshot,
} from "./metadata-conflicts";
import {
  type ObsidianVaultStatus,
  referenceFailureLabel,
  type VaultReference,
  type VaultReferenceFailure,
} from "./obsidian-vaults";
import type { AssetRecord } from "./scanner";
import { formatBytes, issueDetails, issueLabel } from "./ui-model";

export function Inspector({
  assets,
  busy,
  conflicts,
  conflictBusy,
  obsidian,
  onEdit,
  onResolveConflict,
  onDismissConflict,
}: {
  assets: readonly AssetRecord[];
  busy: boolean;
  conflicts: readonly MetadataConflict[];
  conflictBusy?: string;
  obsidian: {
    vault?: ObsidianVaultStatus;
    reference?: VaultReference;
    failure?: VaultReferenceFailure;
    pending: boolean;
    onCopy: () => Promise<void>;
    onConfigure: () => void;
  };
  onEdit: (patch: MetadataPatch) => Promise<void>;
  onResolveConflict: (
    conflict: MetadataConflict,
    resolution: MetadataConflictResolution,
  ) => Promise<void>;
  onDismissConflict: (conflict: MetadataConflict) => Promise<void>;
}) {
  const asset = assets.length === 1 ? assets[0] : undefined;
  const [tag, setTag] = useState("");
  const [note, setNote] = useState(asset?.note ?? "");
  const selectedKeys = new Set(assets.map((item) => item.key));
  const selectedConflicts = conflicts.filter((conflict) =>
    selectedKeys.has(conflict.key),
  );

  useEffect(() => setNote(asset?.note ?? ""), [asset?.key, asset?.note]);

  const editTag = async (operation: "add" | "remove" = "add") => {
    const normalized = tag.trim();
    if (normalized.length === 0) return;
    await onEdit(
      operation === "add"
        ? { addTags: [normalized] }
        : { removeTags: [normalized] },
    );
    setTag("");
  };

  if (assets.length === 0) {
    return (
      <aside className="inspector inspector--empty" aria-label="素材检查器">
        <div className="inspector-empty-mark">
          <Icon name="image" size={22} />
        </div>
        <h2>选择一项素材</h2>
        <p>查看文件属性、Tag、评分和备注。按住 ⌘ 或 Ctrl 可选择多项。</p>
      </aside>
    );
  }

  if (asset === undefined) {
    return (
      <aside className="inspector" aria-label="批量素材检查器">
        <InspectorHeading
          eyebrow="批量编辑"
          title={`${assets.length} 项素材`}
        />
        {selectedConflicts.map((conflict) => (
          <MetadataConflictPanel
            busy={conflictBusy !== undefined}
            conflict={conflict}
            key={conflict.id}
            onDismiss={onDismissConflict}
            onResolve={onResolveConflict}
            resolving={conflictBusy === conflict.id}
          />
        ))}
        <div className="inspector-section">
          <label htmlFor="batch-tag">添加到全部选中项</label>
          <div className="compact-input-row">
            <input
              disabled={busy}
              id="batch-tag"
              onChange={(event) => setTag(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") void editTag();
              }}
              placeholder="例如 project/eagle"
              value={tag}
            />
            <button
              aria-label="批量添加 Tag"
              className="icon-button icon-button--dark"
              disabled={busy || tag.trim().length === 0}
              onClick={() => void editTag()}
              type="button"
            >
              <Icon name="plus" size={16} />
            </button>
            <button
              aria-label="批量移除 Tag"
              className="icon-button"
              disabled={busy || tag.trim().length === 0}
              onClick={() => void editTag("remove")}
              type="button"
            >
              <Icon name="minus" size={16} />
            </button>
          </div>
        </div>
        <div className="inspector-section">
          <span className="field-label">共同操作</span>
          <button
            className="wide-action"
            disabled={busy}
            onClick={() => void onEdit({ favorite: true })}
            type="button"
          >
            <Icon name="star" size={15} /> 全部收藏
          </button>
          <button
            className="wide-action"
            disabled={busy}
            onClick={() => void onEdit({ favorite: false })}
            type="button"
          >
            <Icon name="minus" size={15} /> 取消收藏
          </button>
        </div>
        <p className="inspector-footnote">
          批量修改逐项写入相邻 Sidecar；失败项不会伪造成功状态。
        </p>
      </aside>
    );
  }

  return (
    <aside className="inspector" aria-label="素材检查器">
      <InspectorHeading
        eyebrow={asset.extension?.toUpperCase() ?? asset.kind}
        title={asset.fileName}
      />

      {selectedConflicts.map((conflict) => (
        <MetadataConflictPanel
          busy={conflictBusy !== undefined}
          conflict={conflict}
          key={conflict.id}
          onDismiss={onDismissConflict}
          onResolve={onResolveConflict}
          resolving={conflictBusy === conflict.id}
        />
      ))}

      <div className="inspector-section asset-identity">
        <span className="field-label">稳定素材 ID</span>
        {asset.id ? (
          <code title={asset.id}>{asset.id}</code>
        ) : (
          <span className="muted-copy">尚未建立 Sidecar，暂无稳定 ID</span>
        )}
      </div>

      {asset.issues.length > 0 ? (
        <div className="asset-warning" role="status">
          <Icon name="alert" size={16} />
          <div>
            <strong>{issueLabel(asset.issues[0])}</strong>
            {issueDetails(asset.issues[0]) ? (
              <p>{issueDetails(asset.issues[0])}</p>
            ) : null}
          </div>
        </div>
      ) : null}

      <div className="inspector-section obsidian-reference">
        <span className="field-label">Obsidian 引用</span>
        {obsidian.vault === undefined ? (
          <>
            <p>配置目标 Vault 后，可复制或拖拽标准内部引用。</p>
            <button
              className="wide-action"
              onClick={obsidian.onConfigure}
              type="button"
            >
              <Icon name="link" size={15} /> 配置 Vault
            </button>
          </>
        ) : obsidian.pending ? (
          <p aria-live="polite">正在核对 Vault 相对路径…</p>
        ) : obsidian.reference ? (
          <>
            <div className="vault-reference-heading">
              <span>{obsidian.vault.name}</span>
              <small>Vault 内</small>
            </div>
            <code title={obsidian.reference.relativePath}>
              {obsidian.reference.markdown}
            </code>
            <button
              className="wide-action wide-action--accent"
              onClick={() => void obsidian.onCopy()}
              type="button"
            >
              <Icon name="link" size={15} /> 复制 Obsidian 引用
            </button>
            <p>也可直接将网格卡片拖入 Obsidian 编辑器。</p>
          </>
        ) : (
          <>
            <div className="vault-reference-heading">
              <span>{obsidian.vault.name}</span>
              <small>不可引用</small>
            </div>
            <p className="obsidian-reference-error">
              {referenceFailureLabel(obsidian.failure) ??
                "当前素材无法生成 Vault 内引用"}
            </p>
            <button
              className="text-action"
              onClick={obsidian.onConfigure}
              type="button"
            >
              更换目标 Vault
            </button>
          </>
        )}
      </div>

      <div className="inspector-section">
        <span className="field-label">评分与收藏</span>
        <div className="rating-row" aria-label="素材评分">
          {[1, 2, 3, 4, 5].map((rating) => (
            <button
              aria-label={`${rating} 星`}
              aria-pressed={asset.rating === rating}
              className={asset.rating >= rating ? "is-active" : ""}
              disabled={busy}
              key={rating}
              onClick={() =>
                void onEdit({ rating: asset.rating === rating ? 0 : rating })
              }
              type="button"
            >
              <Icon name="star" size={17} />
            </button>
          ))}
          <button
            aria-label={asset.favorite ? "取消收藏" : "收藏"}
            aria-pressed={asset.favorite}
            className={`favorite-toggle${asset.favorite ? " is-active" : ""}`}
            disabled={busy}
            onClick={() => void onEdit({ favorite: !asset.favorite })}
            type="button"
          >
            收藏
          </button>
        </div>
      </div>

      <div className="inspector-section">
        <span className="field-label">Tags</span>
        <div className="inspector-tags">
          {asset.tags.map((value) => (
            <button
              aria-label={`移除 Tag ${value}`}
              disabled={busy}
              key={value}
              onClick={() => void onEdit({ removeTags: [value] })}
              type="button"
            >
              {value} <Icon name="close" size={11} />
            </button>
          ))}
          {asset.tags.length === 0 ? (
            <span className="muted-copy">尚无 Tag</span>
          ) : null}
        </div>
        <div className="compact-input-row">
          <input
            disabled={busy}
            onChange={(event) => setTag(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void editTag();
            }}
            placeholder="添加 Tag"
            value={tag}
          />
          <button
            aria-label="添加 Tag"
            className="icon-button icon-button--dark"
            disabled={busy || tag.trim().length === 0}
            onClick={() => void editTag()}
            type="button"
          >
            <Icon name="plus" size={16} />
          </button>
        </div>
      </div>

      <div className="inspector-section">
        <label htmlFor="asset-note">备注</label>
        <textarea
          disabled={busy}
          id="asset-note"
          onChange={(event) => setNote(event.target.value)}
          placeholder="记录来源、用途或设计思路…"
          rows={4}
          value={note}
        />
        <button
          className="text-action"
          disabled={busy || note === asset.note}
          onClick={() => void onEdit({ note })}
          type="button"
        >
          保存备注
        </button>
      </div>

      <dl className="asset-facts">
        <div>
          <dt>尺寸</dt>
          <dd>
            {asset.dimensions
              ? `${asset.dimensions.width} × ${asset.dimensions.height}`
              : "—"}
          </dd>
        </div>
        <div>
          <dt>大小</dt>
          <dd>{formatBytes(asset.size)}</dd>
        </div>
        <div>
          <dt>路径</dt>
          <dd title={asset.path}>{asset.relativePath}</dd>
        </div>
      </dl>
    </aside>
  );
}

function MetadataConflictPanel({
  conflict,
  busy,
  resolving,
  onResolve,
  onDismiss,
}: {
  conflict: MetadataConflict;
  busy: boolean;
  resolving: boolean;
  onResolve: (
    conflict: MetadataConflict,
    resolution: MetadataConflictResolution,
  ) => Promise<void>;
  onDismiss: (conflict: MetadataConflict) => Promise<void>;
}) {
  const [tags, setTags] = useState<TagConflictResolution>();
  const [fields, setFields] = useState<
    Partial<Record<MetadataConflictField, FieldConflictResolution>>
  >({});

  useEffect(() => {
    setTags(undefined);
    setFields({});
  }, [conflict.id]);

  const ready = conflict.conflictingFields.every((field) =>
    field === "tags" ? tags !== undefined : fields[field] !== undefined,
  );
  const resolve = () =>
    onResolve(conflict, {
      ...(tags === undefined ? {} : { tags }),
      fields,
    });

  return (
    <section className="metadata-conflict" aria-label="并发元数据冲突">
      <div className="metadata-conflict-heading">
        <div>
          <strong>检测到外部 Sidecar 修改</strong>
          <span>{formatConflictTime(conflict.sidecarModifiedUnixMs)}</span>
        </div>
        <small>版本校验：mtime · 大小 · SHA-256</small>
      </div>
      {conflict.identityChanged ? (
        <p className="metadata-conflict-warning">
          外部版本的稳定 ID 也发生变化；解决时保留磁盘上的当前 ID。
        </p>
      ) : null}
      <p>
        外部变化：
        {conflict.externallyChangedFields.map(conflictFieldLabel).join("、") ||
          "文件版本变化但用户字段相同"}
      </p>
      {conflict.conflictingFields.length === 0 ? (
        <p>你的修改与外部字段不重叠，可显式应用到当前外部版本。</p>
      ) : null}
      {conflict.conflictingFields.map((field) =>
        field === "tags" ? (
          <div className="metadata-conflict-field" key={field}>
            <strong>Tags</strong>
            <ConflictVersions conflict={conflict} field={field} />
            <div className="conflict-choice-row">
              {(
                [
                  ["merge", "合并 Tag"],
                  ["keep-external", "保留外部"],
                  ["use-mine", "使用我的"],
                ] as const
              ).map(([value, label]) => (
                <button
                  aria-pressed={tags === value}
                  className={tags === value ? "is-active" : ""}
                  disabled={busy}
                  key={value}
                  onClick={() => setTags(value)}
                  type="button"
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
        ) : (
          <div className="metadata-conflict-field" key={field}>
            <strong>{conflictFieldLabel(field)}</strong>
            <ConflictVersions conflict={conflict} field={field} />
            <div className="conflict-choice-row">
              {(
                [
                  ["keep-external", "保留外部"],
                  ["use-mine", "使用我的"],
                ] as const
              ).map(([value, label]) => (
                <button
                  aria-pressed={fields[field] === value}
                  className={fields[field] === value ? "is-active" : ""}
                  disabled={busy}
                  key={value}
                  onClick={() =>
                    setFields((current) => ({
                      ...current,
                      [field]: value,
                    }))
                  }
                  type="button"
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
        ),
      )}
      <div className="metadata-conflict-actions">
        <button
          className="secondary-button"
          disabled={busy}
          onClick={() => void onDismiss(conflict)}
          type="button"
        >
          放弃我的修改并重载
        </button>
        <button
          className="primary-button"
          disabled={busy || !ready}
          onClick={() => void resolve()}
          type="button"
        >
          {resolving ? "正在重新验证…" : "按所选版本解决"}
        </button>
      </div>
    </section>
  );
}

function ConflictVersions({
  conflict,
  field,
}: {
  conflict: MetadataConflict;
  field: MetadataConflictField;
}) {
  return (
    <div className="conflict-versions">
      <span>
        <small>编辑前</small>
        {metadataFieldValue(conflict.base, field)}
      </span>
      <span>
        <small>外部当前</small>
        {metadataFieldValue(conflict.current, field)}
      </span>
      <span>
        <small>我的修改</small>
        {metadataFieldValue(conflict.proposed, field)}
      </span>
    </div>
  );
}

function metadataFieldValue(
  metadata: UserMetadataSnapshot,
  field: MetadataConflictField,
): string {
  const value = metadata[field];
  if (Array.isArray(value)) return value.join(", ") || "（空）";
  if (typeof value === "boolean") return value ? "是" : "否";
  if (field === "rating") return `${value} 星`;
  return String(value || "（空）");
}

function conflictFieldLabel(field: MetadataConflictField): string {
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

function formatConflictTime(value: number): string {
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(value);
}

function InspectorHeading({
  eyebrow,
  title,
}: {
  eyebrow: string;
  title: string;
}) {
  return (
    <header className="inspector-heading">
      <span>{eyebrow}</span>
      <h2 title={title}>{title}</h2>
    </header>
  );
}
