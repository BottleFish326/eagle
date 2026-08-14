import { useEffect, useState } from "react";

import { Icon } from "./Icon";
import type { MetadataPatch } from "./metadata-editor";
import type { AssetRecord } from "./scanner";
import { formatBytes, issueDetails, issueLabel } from "./ui-model";

export function Inspector({
  assets,
  busy,
  onEdit,
}: {
  assets: readonly AssetRecord[];
  busy: boolean;
  onEdit: (patch: MetadataPatch) => Promise<void>;
}) {
  const asset = assets.length === 1 ? assets[0] : undefined;
  const [tag, setTag] = useState("");
  const [note, setNote] = useState(asset?.note ?? "");

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
