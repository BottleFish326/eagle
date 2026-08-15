import type {
  CSSProperties,
  DragEvent,
  KeyboardEvent,
  MouseEvent,
} from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { AssetThumbnail } from "./AssetThumbnail";
import type { DesktopApi } from "./desktop-api";
import { Icon } from "./Icon";
import {
  type VaultReference,
  writeVaultReferenceDragData,
} from "./obsidian-vaults";
import type { AssetRecord } from "./scanner";
import { issueLabel, nextGridIndex } from "./ui-model";

export interface AssetSelectionIntent {
  range: boolean;
  toggle: boolean;
}

export interface AssetGridWindow {
  columns: number;
  endIndex: number;
  itemHeight: number;
  rowHeight: number;
  startIndex: number;
  startRow: number;
  totalHeight: number;
}

const DESKTOP_MIN_COLUMN_WIDTH = 172;
const COMPACT_DESKTOP_MIN_COLUMN_WIDTH = 155;
const COPY_HEIGHT = 28;
const OVERSCAN_ROWS = 3;

export function calculateAssetGridWindow({
  containerWidth,
  itemCount,
  viewportHeight,
  viewportOffset,
  windowWidth,
}: {
  containerWidth: number;
  itemCount: number;
  viewportHeight: number;
  viewportOffset: number;
  windowWidth: number;
}): AssetGridWindow {
  const compact = windowWidth <= 680;
  const columnGap = compact ? 10 : 14;
  const rowGap = compact ? 16 : 20;
  const minimumColumnWidth =
    windowWidth <= 1_180
      ? COMPACT_DESKTOP_MIN_COLUMN_WIDTH
      : DESKTOP_MIN_COLUMN_WIDTH;
  const columns = compact
    ? 2
    : Math.max(
        1,
        Math.floor(
          (Math.max(1, containerWidth) + columnGap) /
            (minimumColumnWidth + columnGap),
        ),
      );
  const columnWidth = Math.max(
    1,
    (Math.max(1, containerWidth) - columnGap * (columns - 1)) / columns,
  );
  const itemHeight = columnWidth * 0.75 + COPY_HEIGHT;
  const rowHeight = itemHeight + rowGap;
  const rowCount = Math.ceil(itemCount / columns);
  const totalHeight = Math.max(0, rowCount * rowHeight - rowGap);
  if (itemCount === 0 || rowCount === 0) {
    return {
      columns,
      endIndex: 0,
      itemHeight,
      rowHeight,
      startIndex: 0,
      startRow: 0,
      totalHeight,
    };
  }

  const visibleStart = Math.max(0, viewportOffset);
  const requestedStartRow = Math.max(
    0,
    Math.floor(visibleStart / rowHeight) - OVERSCAN_ROWS,
  );
  const startRow = Math.min(rowCount - 1, requestedStartRow);
  const endRow = Math.min(
    rowCount,
    Math.max(
      startRow + 1,
      Math.ceil((visibleStart + Math.max(1, viewportHeight)) / rowHeight) +
        OVERSCAN_ROWS,
    ),
  );

  return {
    columns,
    endIndex: Math.min(itemCount, endRow * columns),
    itemHeight,
    rowHeight,
    startIndex: startRow * columns,
    startRow,
    totalHeight,
  };
}

export function AssetGrid({
  api,
  assets,
  selected,
  vaultReferences,
  onWindowChange,
  onSelect,
}: {
  api: DesktopApi;
  assets: readonly AssetRecord[];
  selected: ReadonlySet<string>;
  vaultReferences: ReadonlyMap<string, VaultReference>;
  onWindowChange: (keys: string[]) => void;
  onSelect: (key: string, intent: AssetSelectionIntent) => void;
}) {
  const grid = useRef<HTMLDivElement>(null);
  const pendingFocus = useRef<number | undefined>(undefined);
  const frame = useRef<number | undefined>(undefined);
  const [viewport, setViewport] = useState(() => ({
    containerWidth: 1,
    viewportHeight: typeof window === "undefined" ? 800 : window.innerHeight,
    viewportOffset: 0,
    windowWidth: typeof window === "undefined" ? 1_200 : window.innerWidth,
  }));

  const measure = useCallback(() => {
    const element = grid.current;
    if (element === null) return;
    const bounds = element.getBoundingClientRect();
    setViewport((current) => {
      const next = {
        containerWidth: bounds.width,
        viewportHeight: window.innerHeight,
        viewportOffset: -bounds.top,
        windowWidth: window.innerWidth,
      };
      return measurementsEqual(current, next) ? current : next;
    });
  }, []);

  const scheduleMeasure = useCallback(() => {
    if (frame.current !== undefined) return;
    frame.current = window.requestAnimationFrame(() => {
      frame.current = undefined;
      measure();
    });
  }, [measure]);

  useEffect(() => {
    measure();
    window.addEventListener("resize", scheduleMeasure);
    window.addEventListener("scroll", scheduleMeasure, { passive: true });
    const observer = new ResizeObserver(scheduleMeasure);
    if (grid.current !== null) observer.observe(grid.current);
    return () => {
      window.removeEventListener("resize", scheduleMeasure);
      window.removeEventListener("scroll", scheduleMeasure);
      observer.disconnect();
      if (frame.current !== undefined) {
        window.cancelAnimationFrame(frame.current);
        frame.current = undefined;
      }
    };
  }, [measure, scheduleMeasure]);

  useEffect(() => {
    scheduleMeasure();
  }, [assets.length, scheduleMeasure]);

  const assetWindow = useMemo(
    () =>
      calculateAssetGridWindow({
        ...viewport,
        itemCount: assets.length,
      }),
    [assets.length, viewport],
  );

  useEffect(() => {
    const index = pendingFocus.current;
    if (
      index === undefined ||
      index < assetWindow.startIndex ||
      index >= assetWindow.endIndex
    )
      return;
    grid.current
      ?.querySelector<HTMLButtonElement>(`[data-asset-index="${index}"]`)
      ?.focus();
    pendingFocus.current = undefined;
  }, [assetWindow.endIndex, assetWindow.startIndex]);

  const handleKeyboard = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!isNavigationKey(event.key)) return;
    const card = (event.target as HTMLElement).closest<HTMLButtonElement>(
      "[data-asset-index]",
    );
    const current = Number(card?.dataset.assetIndex);
    if (!Number.isInteger(current)) return;
    const next = nextGridIndex(
      current,
      assets.length,
      assetWindow.columns,
      event.key,
    );
    if (next !== current) {
      const nextCard = grid.current?.querySelector<HTMLButtonElement>(
        `[data-asset-index="${next}"]`,
      );
      if (nextCard !== null && nextCard !== undefined) {
        nextCard.focus();
      } else {
        pendingFocus.current = next;
        const bounds = grid.current?.getBoundingClientRect();
        if (bounds !== undefined) {
          const gridTop = window.scrollY + bounds.top;
          const row = Math.floor(next / assetWindow.columns);
          window.scrollTo({
            behavior: "auto",
            top: Math.max(0, gridTop + row * assetWindow.rowHeight - 96),
          });
        }
      }
    }
    event.preventDefault();
  };

  const visibleAssets = useMemo(
    () => assets.slice(assetWindow.startIndex, assetWindow.endIndex),
    [assetWindow.endIndex, assetWindow.startIndex, assets],
  );

  useEffect(() => {
    onWindowChange(visibleAssets.map((asset) => asset.key));
  }, [onWindowChange, visibleAssets]);

  useEffect(
    () => () => {
      onWindowChange([]);
    },
    [onWindowChange],
  );
  const windowStyle = {
    gap: `${viewport.windowWidth <= 680 ? 16 : 20}px ${viewport.windowWidth <= 680 ? 10 : 14}px`,
    gridAutoRows: `${assetWindow.itemHeight}px`,
    gridTemplateColumns: `repeat(${assetWindow.columns}, minmax(0, 1fr))`,
    transform: `translateY(${assetWindow.startRow * assetWindow.rowHeight}px)`,
  } satisfies CSSProperties;

  return (
    <div
      aria-label="素材网格"
      className="asset-grid"
      onKeyDown={handleKeyboard}
      ref={grid}
      role="list"
      style={{ height: assetWindow.totalHeight }}
    >
      <div className="asset-grid-window" style={windowStyle}>
        {visibleAssets.map((asset, visibleIndex) => {
          const assetIndex = assetWindow.startIndex + visibleIndex;
          const isSelected = selected.has(asset.key);
          const issue = asset.issues[0];
          const vaultReference = vaultReferences.get(asset.key);
          return (
            <article
              aria-posinset={assetIndex + 1}
              aria-setsize={assets.length}
              className="asset-grid-item"
              key={asset.key}
              role="listitem"
            >
              <button
                aria-label={`${asset.fileName}${isSelected ? "，已选择" : ""}`}
                aria-pressed={isSelected}
                className={`asset-card${isSelected ? " is-selected" : ""}`}
                data-asset-card
                data-asset-index={assetIndex}
                draggable={vaultReference !== undefined}
                onDragStart={(event) => {
                  if (vaultReference === undefined) {
                    event.preventDefault();
                    return;
                  }
                  prepareVaultDrag(event, vaultReference);
                  if (!isSelected) {
                    onSelect(asset.key, { range: false, toggle: false });
                  }
                }}
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
                {vaultReference ? (
                  <span
                    aria-label={`可拖入 ${vaultReference.vaultName}`}
                    className="asset-card-obsidian"
                    title={`拖入 Obsidian：${vaultReference.markdown}`}
                  >
                    <Icon name="link" size={12} />
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
    </div>
  );
}

function measurementsEqual(
  left: {
    containerWidth: number;
    viewportHeight: number;
    viewportOffset: number;
    windowWidth: number;
  },
  right: {
    containerWidth: number;
    viewportHeight: number;
    viewportOffset: number;
    windowWidth: number;
  },
): boolean {
  return (
    Math.abs(left.containerWidth - right.containerWidth) < 0.5 &&
    left.viewportHeight === right.viewportHeight &&
    Math.abs(left.viewportOffset - right.viewportOffset) < 0.5 &&
    left.windowWidth === right.windowWidth
  );
}

function prepareVaultDrag(
  event: DragEvent<HTMLButtonElement>,
  reference: VaultReference,
): void {
  writeVaultReferenceDragData(event.dataTransfer, reference);
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
