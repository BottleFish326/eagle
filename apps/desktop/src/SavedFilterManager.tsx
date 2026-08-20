import { useEffect, useRef, useState } from "react";

import { Icon } from "./Icon";
import type { LibraryRootStatus } from "./library-roots";
import type {
  SavedFilter,
  SavedFilterCatalog,
  SavedFilterInput,
  SavedFilterSortDirection,
  SavedFilterSortField,
} from "./saved-filters";

export function SavedFilterManager({
  open,
  catalog,
  roots,
  currentQuery,
  activeFilterId,
  busy,
  onClose,
  onRefresh,
  onCreate,
  onUpdate,
  onRename,
  onDelete,
  onActivate,
}: {
  open: boolean;
  catalog: SavedFilterCatalog;
  roots: readonly LibraryRootStatus[];
  currentQuery: string;
  activeFilterId?: string;
  busy: boolean;
  onClose: () => void;
  onRefresh: () => Promise<void>;
  onCreate: (input: SavedFilterInput) => Promise<void>;
  onUpdate: (filter: SavedFilter, input: SavedFilterInput) => Promise<void>;
  onRename: (filter: SavedFilter, name: string) => Promise<void>;
  onDelete: (filter: SavedFilter) => Promise<void>;
  onActivate: (filter: SavedFilter) => Promise<void>;
}) {
  const [name, setName] = useState("");
  const [scopeKind, setScopeKind] = useState<"all" | "selected">("all");
  const [selectedRootIds, setSelectedRootIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [sortField, setSortField] =
    useState<SavedFilterSortField>("modified-at");
  const [sortDirection, setSortDirection] =
    useState<SavedFilterSortDirection>("descending");
  const [renameId, setRenameId] = useState<string>();
  const [renameName, setRenameName] = useState("");
  const closeButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (open) closeButton.current?.focus();
    else {
      setRenameId(undefined);
      setRenameName("");
    }
  }, [open]);

  if (!open) return null;

  const input = (inputName: string): SavedFilterInput => ({
    name: inputName.trim(),
    query: currentQuery.trim(),
    scope:
      scopeKind === "all"
        ? { kind: "all-enabled-roots" }
        : { kind: "selected-roots", rootIds: [...selectedRootIds].sort() },
    sort: { field: sortField, direction: sortDirection },
  });
  const canCreate =
    name.trim().length > 0 && (scopeKind === "all" || selectedRootIds.size > 0);
  const unavailableById = new Map(
    catalog.unavailableFilters.map((entry) => [entry.filter.id, entry]),
  );
  const filters = [
    ...catalog.validFilters,
    ...catalog.unavailableFilters.map((entry) => entry.filter),
  ].sort((left, right) => left.name.localeCompare(right.name));

  return (
    <div className="modal-backdrop" onMouseDown={onClose} role="presentation">
      <section
        aria-labelledby="saved-filter-dialog-title"
        aria-modal="true"
        className="library-dialog saved-filter-dialog"
        onMouseDown={(event) => event.stopPropagation()}
        role="dialog"
      >
        <header className="dialog-heading">
          <div>
            <span className="overline">Filesystem views</span>
            <h2 id="saved-filter-dialog-title">保存过滤器</h2>
            <p>
              只保存查询、范围与排序到应用配置目录；结果会从当前文件重新计算。
            </p>
          </div>
          <div className="dialog-heading-actions">
            <button
              aria-label="重新读取保存过滤器文件"
              className="icon-button"
              disabled={busy}
              onClick={() => void onRefresh()}
              type="button"
            >
              <Icon name="refresh" />
            </button>
            <button
              aria-label="关闭保存过滤器"
              className="icon-button"
              onClick={onClose}
              ref={closeButton}
              type="button"
            >
              <Icon name="close" />
            </button>
          </div>
        </header>

        {catalog.fileIssues.length > 0 ? (
          <div className="saved-filter-warning" role="alert">
            <Icon name="alert" size={16} />
            saved-filters.yml 当前无效，应用已保持原文件不变。修复后请重新读取。
          </div>
        ) : null}

        <div className="saved-filter-layout">
          <section className="saved-filter-list" aria-label="已保存过滤器">
            {filters.map((filter) => {
              const unavailable = unavailableById.get(filter.id);
              return (
                <article
                  className={`saved-filter-row${activeFilterId === filter.id ? " is-active" : ""}`}
                  key={filter.id}
                >
                  <div className="saved-filter-copy">
                    {renameId === filter.id ? (
                      <input
                        aria-label={`重命名 ${filter.name}`}
                        autoFocus
                        disabled={busy}
                        maxLength={128}
                        onChange={(event) => setRenameName(event.target.value)}
                        value={renameName}
                      />
                    ) : (
                      <strong>{filter.name}</strong>
                    )}
                    <span title={filter.query}>
                      {filter.query || "全部素材"}
                    </span>
                    <small>
                      {sortLabel(filter)}
                      {unavailable
                        ? ` · ${unavailable.missingRootIds.length} 个范围当前不可用`
                        : ""}
                    </small>
                  </div>
                  {renameId === filter.id ? (
                    <button
                      className="root-action"
                      disabled={busy || renameName.trim().length === 0}
                      onClick={() =>
                        void onRename(filter, renameName)
                          .then(() => {
                            setRenameId(undefined);
                            setRenameName("");
                          })
                          .catch(() => undefined)
                      }
                      title="确认重命名"
                      type="button"
                    >
                      <Icon name="check" size={15} />
                    </button>
                  ) : (
                    <button
                      className="root-action"
                      disabled={busy}
                      onClick={() => {
                        setRenameId(filter.id);
                        setRenameName(filter.name);
                      }}
                      title="重命名"
                      type="button"
                    >
                      <Icon name="tag" size={15} />
                    </button>
                  )}
                  <button
                    className="root-action"
                    disabled={busy}
                    onClick={() =>
                      void onUpdate(filter, input(filter.name)).catch(
                        () => undefined,
                      )
                    }
                    title="用当前视图更新"
                    type="button"
                  >
                    <Icon name="refresh" size={15} />
                  </button>
                  <button
                    className="saved-filter-activate"
                    disabled={busy}
                    onClick={() =>
                      void onActivate(filter).catch(() => undefined)
                    }
                    type="button"
                  >
                    应用
                  </button>
                  <button
                    className="root-action"
                    disabled={busy}
                    onClick={() => void onDelete(filter).catch(() => undefined)}
                    title="删除保存过滤器"
                    type="button"
                  >
                    <Icon name="trash" size={15} />
                  </button>
                </article>
              );
            })}
            {filters.length === 0 ? (
              <div className="root-empty">
                <Icon name="search" />
                <p>尚未保存过滤器。</p>
              </div>
            ) : null}
            {catalog.invalidEntries.length > 0 ? (
              <div className="saved-filter-warning" role="status">
                <Icon name="alert" size={16} />
                已隔离 {catalog.invalidEntries.length}{" "}
                个无效条目，其他条目仍可使用。
              </div>
            ) : null}
          </section>

          <form
            className="saved-filter-form"
            onSubmit={(event) => {
              event.preventDefault();
              if (!canCreate) return;
              void onCreate(input(name))
                .then(() => setName(""))
                .catch(() => undefined);
            }}
          >
            <div className="form-heading">
              <span className="overline">Save current view</span>
              <h3>保存当前视图</h3>
            </div>
            <label>
              名称
              <input
                autoComplete="off"
                disabled={busy}
                maxLength={128}
                onChange={(event) => setName(event.target.value)}
                placeholder="例如 本周参考"
                value={name}
              />
            </label>
            <label>
              素材范围
              <select
                disabled={busy}
                onChange={(event) =>
                  setScopeKind(
                    event.target.value === "selected" ? "selected" : "all",
                  )
                }
                value={scopeKind}
              >
                <option value="all">所有已启用位置</option>
                <option value="selected">指定位置</option>
              </select>
            </label>
            {scopeKind === "selected" ? (
              <fieldset className="saved-filter-roots">
                <legend>指定位置</legend>
                {roots.map((root) => (
                  <label key={root.id}>
                    <input
                      checked={selectedRootIds.has(root.id)}
                      disabled={busy}
                      onChange={() =>
                        setSelectedRootIds((current) => {
                          const next = new Set(current);
                          if (next.has(root.id)) next.delete(root.id);
                          else next.add(root.id);
                          return next;
                        })
                      }
                      type="checkbox"
                    />
                    {root.name}
                  </label>
                ))}
              </fieldset>
            ) : null}
            <div className="saved-filter-sort">
              <label>
                排序字段
                <select
                  disabled={busy}
                  onChange={(event) =>
                    setSortField(event.target.value as SavedFilterSortField)
                  }
                  value={sortField}
                >
                  <option value="modified-at">修改时间</option>
                  <option value="created-at">创建时间</option>
                  <option value="file-name">文件名</option>
                  <option value="file-size">文件大小</option>
                  <option value="rating">评分</option>
                  <option value="asset-kind">素材类型</option>
                </select>
              </label>
              <label>
                方向
                <select
                  disabled={busy}
                  onChange={(event) =>
                    setSortDirection(
                      event.target.value as SavedFilterSortDirection,
                    )
                  }
                  value={sortDirection}
                >
                  <option value="descending">降序</option>
                  <option value="ascending">升序</option>
                </select>
              </label>
            </div>
            <div className="saved-filter-query" title={currentQuery}>
              <span>当前查询</span>
              <code>{currentQuery || "全部素材"}</code>
            </div>
            <button
              className="primary-button"
              disabled={busy || !canCreate}
              type="submit"
            >
              <Icon name="plus" size={16} /> 保存过滤器
            </button>
          </form>
        </div>
      </section>
    </div>
  );
}

function sortLabel(filter: SavedFilter): string {
  const fields: Record<SavedFilterSortField, string> = {
    "file-name": "文件名",
    "modified-at": "修改时间",
    "created-at": "创建时间",
    "file-size": "文件大小",
    rating: "评分",
    "asset-kind": "素材类型",
  };
  return `${fields[filter.sort.field]}${filter.sort.direction === "ascending" ? "升序" : "降序"}`;
}
