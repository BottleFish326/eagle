import type { KeyboardEvent, MouseEvent } from "react";
import { useRef } from "react";

import { AssetThumbnail } from "./AssetThumbnail";
import type { DesktopApi } from "./desktop-api";
import { Icon } from "./Icon";
import type { AssetRecord } from "./scanner";
import { issueLabel, nextGridIndex } from "./ui-model";

export interface AssetSelectionIntent {
  range: boolean;
  toggle: boolean;
}

export function AssetGrid({
  api,
  assets,
  selected,
  onSelect,
}: {
  api: DesktopApi;
  assets: readonly AssetRecord[];
  selected: ReadonlySet<string>;
  onSelect: (key: string, intent: AssetSelectionIntent) => void;
}) {
  const grid = useRef<HTMLDivElement>(null);

  const handleKeyboard = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!isNavigationKey(event.key)) return;
    const cards = [
      ...(grid.current?.querySelectorAll<HTMLButtonElement>(
        "[data-asset-card]",
      ) ?? []),
    ];
    const current = cards.findIndex(
      (card) => card === event.target || card.contains(event.target as Node),
    );
    if (current < 0) return;
    const columns = grid.current
      ? getComputedStyle(grid.current).gridTemplateColumns.split(" ").length
      : 1;
    const next = nextGridIndex(current, cards.length, columns, event.key);
    if (next !== current) cards[next]?.focus();
    event.preventDefault();
  };

  return (
    <div
      aria-label="素材网格"
      className="asset-grid"
      onKeyDown={handleKeyboard}
      ref={grid}
      role="list"
    >
      {assets.map((asset) => {
        const isSelected = selected.has(asset.key);
        const issue = asset.issues[0];
        return (
          <article className="asset-grid-item" key={asset.key} role="listitem">
            <button
              aria-label={`${asset.fileName}${isSelected ? "，已选择" : ""}`}
              aria-pressed={isSelected}
              className={`asset-card${isSelected ? " is-selected" : ""}`}
              data-asset-card
              onClick={(event) => onSelect(asset.key, selectionIntent(event))}
              type="button"
            >
              <AssetThumbnail api={api} asset={asset} />
              <span className="asset-card-selection" aria-hidden="true">
                {isSelected ? <Icon name="check" size={13} /> : null}
              </span>
              {asset.favorite ? (
                <span className="asset-card-favorite" aria-label="已收藏">
                  <Icon name="star" size={13} />
                </span>
              ) : null}
              {issue ? (
                <span className="asset-card-issue" title={issueLabel(issue)}>
                  <Icon name="alert" size={12} />
                </span>
              ) : null}
              <span className="asset-card-copy">
                <strong>{asset.fileName}</strong>
                <small>
                  {asset.dimensions
                    ? `${asset.dimensions.width} × ${asset.dimensions.height}`
                    : (asset.extension?.toUpperCase() ?? "FILE")}
                </small>
              </span>
            </button>
          </article>
        );
      })}
    </div>
  );
}

function selectionIntent(
  event: MouseEvent<HTMLButtonElement>,
): AssetSelectionIntent {
  return {
    range: event.shiftKey,
    toggle: event.metaKey || event.ctrlKey,
  };
}

function isNavigationKey(
  key: string,
): key is
  "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End" {
  return [
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "ArrowDown",
    "Home",
    "End",
  ].includes(key);
}
