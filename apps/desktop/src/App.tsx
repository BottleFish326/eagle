import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { AssetGrid, type AssetSelectionIntent } from "./AssetGrid";
import { type BuildInfo, loadBuildInfo } from "./build-info";
import { createDemoDesktopApi } from "./demo-api";
import {
  type DesktopApi,
  isTauriRuntime,
  tauriDesktopApi,
} from "./desktop-api";
import { Icon } from "./Icon";
import { Inspector } from "./Inspector";
import type { LibraryRootStatus } from "./library-roots";
import type { MetadataPatch } from "./metadata-editor";
import {
  copyVaultReference,
  type ObsidianVaultStatus,
  type VaultReference,
  type VaultReferenceFailure,
} from "./obsidian-vaults";
import { RootManager } from "./RootManager";
import type { AssetRecord, LibraryScanEvent } from "./scanner";
import {
  composeAssetQuery,
  cycleTagFilter,
  formatQueryError,
  removeRootAssets,
  summarizeTags,
  type TagFilterMap,
  type TagFilterState,
  upsertAssets,
} from "./ui-model";
import { VaultManager } from "./VaultManager";

const defaultApi = isTauriRuntime() ? tauriDesktopApi : createDemoDesktopApi();

type ScanPhase = "starting" | "scanning" | "completed" | "cancelled" | "failed";

interface ScanUiState {
  scanId?: string;
  phase: ScanPhase;
  visitedFiles: number;
  assetCount: number;
  problemCount: number;
  message?: string;
}

interface Notice {
  tone: "info" | "error";
  message: string;
}

export function App({ api = defaultApi }: { api?: DesktopApi }) {
  const search = useRef<HTMLInputElement>(null);
  const [buildInfo, setBuildInfo] = useState<BuildInfo>();
  const [roots, setRoots] = useState<LibraryRootStatus[]>([]);
  const [vaults, setVaults] = useState<ObsidianVaultStatus[]>([]);
  const [activeVaultId, setActiveVaultId] = useState<string>();
  const [assets, setAssets] = useState<Map<string, AssetRecord>>(
    () => new Map(),
  );
  const [visibleKeys, setVisibleKeys] = useState<string[]>([]);
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [selectionAnchor, setSelectionAnchor] = useState<string>();
  const [expression, setExpression] = useState("");
  const [tagFilters, setTagFilters] = useState<TagFilterMap>({});
  const [queryError, setQueryError] = useState<string>();
  const [queryPending, setQueryPending] = useState(false);
  const [scans, setScans] = useState<Record<string, ScanUiState>>({});
  const [rootManagerOpen, setRootManagerOpen] = useState(false);
  const [vaultManagerOpen, setVaultManagerOpen] = useState(false);
  const [rootBusy, setRootBusy] = useState(false);
  const [vaultBusy, setVaultBusy] = useState(false);
  const [vaultReferences, setVaultReferences] = useState<
    Map<string, VaultReference>
  >(() => new Map());
  const [vaultReferenceFailures, setVaultReferenceFailures] = useState<
    Map<string, VaultReferenceFailure>
  >(() => new Map());
  const [vaultReferencesPending, setVaultReferencesPending] = useState(false);
  const [editBusy, setEditBusy] = useState(false);
  const [booting, setBooting] = useState(true);
  const [notice, setNotice] = useState<Notice>();

  const runScan = useCallback(
    async (root: LibraryRootStatus) => {
      setAssets((current) => removeRootAssets(current, root.id));
      setScans((current) => ({
        ...current,
        [root.id]: {
          phase: "starting",
          visitedFiles: 0,
          assetCount: 0,
          problemCount: 0,
        },
      }));
      const receive = (event: LibraryScanEvent) => {
        handleScanEvent(root.id, event, setAssets, setScans);
      };
      try {
        const scanId = await api.startLibraryScan(root.id, receive);
        setScans((current) => ({
          ...current,
          [root.id]: {
            ...(current[root.id] ?? {
              phase: "scanning",
              visitedFiles: 0,
              assetCount: 0,
              problemCount: 0,
            }),
            scanId,
          },
        }));
      } catch (error) {
        setScans((current) => ({
          ...current,
          [root.id]: {
            phase: "failed",
            visitedFiles: 0,
            assetCount: 0,
            problemCount: 0,
            message: errorMessage(error),
          },
        }));
        setNotice({
          tone: "error",
          message: `无法扫描 ${root.name}：${errorMessage(error)}`,
        });
      }
    },
    [api],
  );

  useEffect(() => {
    let active = true;
    void loadBuildInfo().then((value) => {
      if (active) setBuildInfo(value);
    });
    void api
      .listLibraryRoots()
      .then((value) => {
        if (!active) return;
        setRoots(value);
        setBooting(false);
        for (const root of value) {
          if (root.enabled && root.accessStatus === "available")
            void runScan(root);
        }
      })
      .catch((error: unknown) => {
        if (!active) return;
        setBooting(false);
        setNotice({
          tone: "error",
          message: `素材根目录读取失败：${errorMessage(error)}`,
        });
      });
    void api
      .listObsidianVaults()
      .then((value) => {
        if (!active) return;
        setVaults(value);
        setActiveVaultId((current) =>
          current && value.some((vault) => vault.id === current)
            ? current
            : value.find(
                (vault) => vault.enabled && vault.accessStatus === "available",
              )?.id,
        );
      })
      .catch((error: unknown) => {
        if (!active) return;
        setNotice({
          tone: "error",
          message: `Obsidian Vault 配置读取失败：${errorMessage(error)}`,
        });
      });
    return () => {
      active = false;
    };
  }, [api, runScan]);

  const effectiveQuery = useMemo(
    () => composeAssetQuery(expression, tagFilters),
    [expression, tagFilters],
  );

  useEffect(() => {
    let active = true;
    const timer = window.setTimeout(() => {
      setQueryPending(true);
      void api
        .queryAssets({ expression: effectiveQuery })
        .then((result) => {
          if (!active) return;
          setVisibleKeys(result.keys.filter((key) => assets.has(key)));
          setQueryError(undefined);
        })
        .catch((error: unknown) => {
          if (!active) return;
          setQueryError(formatQueryError(error));
        })
        .finally(() => {
          if (active) setQueryPending(false);
        });
    }, 120);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [api, assets, effectiveQuery]);

  const allAssets = useMemo(() => [...assets.values()], [assets]);
  const visibleAssets = useMemo(
    () =>
      visibleKeys.flatMap((key) => (assets.get(key) ? [assets.get(key)!] : [])),
    [assets, visibleKeys],
  );
  const selectedAssets = useMemo(
    () =>
      [...selected].flatMap((key) =>
        assets.get(key) ? [assets.get(key)!] : [],
      ),
    [assets, selected],
  );
  const tags = useMemo(
    () => summarizeTags(allAssets, tagFilters),
    [allAssets, tagFilters],
  );
  const activeTagFilters = tags.filter((tag) => tag.state !== "neutral");
  const activeScans = Object.entries(scans).filter(
    ([, scan]) => scan.phase === "starting" || scan.phase === "scanning",
  );
  const failedScans = Object.entries(scans).filter(
    ([, scan]) => scan.phase === "failed",
  );
  const inaccessibleRoots = roots.filter(
    (root) => root.enabled && root.accessStatus !== "available",
  );
  const activeVault = vaults.find((vault) => vault.id === activeVaultId);

  useEffect(() => {
    let active = true;
    if (activeVaultId === undefined || visibleKeys.length === 0) {
      setVaultReferences(new Map());
      setVaultReferenceFailures(new Map());
      setVaultReferencesPending(false);
      return () => {
        active = false;
      };
    }
    const timer = window.setTimeout(() => {
      setVaultReferencesPending(true);
      void api
        .resolveObsidianVaultReferences({
          vaultId: activeVaultId,
          assetKeys: visibleKeys,
        })
        .then((result) => {
          if (!active) return;
          setVaultReferences(
            new Map(
              result.resolved.map((reference) => [
                reference.assetKey,
                reference,
              ]),
            ),
          );
          setVaultReferenceFailures(
            new Map(
              result.failures.map((failure) => [failure.assetKey, failure]),
            ),
          );
        })
        .catch((error: unknown) => {
          if (!active) return;
          setVaultReferences(new Map());
          setVaultReferenceFailures(new Map());
          setNotice({
            tone: "error",
            message: `Obsidian 引用解析失败：${errorMessage(error)}`,
          });
        })
        .finally(() => {
          if (active) setVaultReferencesPending(false);
        });
    }, 80);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [activeVaultId, api, visibleKeys]);

  useEffect(() => {
    setSelected(
      (current) => new Set([...current].filter((key) => assets.has(key))),
    );
  }, [assets]);

  useEffect(() => {
    const visible = new Set(visibleKeys);
    setSelected((current) => {
      const retained = new Set([...current].filter((key) => visible.has(key)));
      return retained.size === current.size ? current : retained;
    });
    setSelectionAnchor((current) =>
      current && visible.has(current) ? current : undefined,
    );
  }, [visibleKeys]);

  useEffect(() => {
    const handleGlobalKey = (event: globalThis.KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const editing =
        target?.matches("input, textarea, [contenteditable=true]") ?? false;
      if (event.key === "/" && !editing) {
        event.preventDefault();
        search.current?.focus();
      } else if (event.key === "Escape") {
        if (vaultManagerOpen) setVaultManagerOpen(false);
        else if (rootManagerOpen) setRootManagerOpen(false);
        else if (!editing) setSelected(new Set());
      }
    };
    window.addEventListener("keydown", handleGlobalKey);
    return () => window.removeEventListener("keydown", handleGlobalKey);
  }, [rootManagerOpen, vaultManagerOpen]);

  const selectAsset = (key: string, intent: AssetSelectionIntent) => {
    setSelected((current) => {
      if (intent.range && selectionAnchor) {
        const keys = visibleAssets.map((asset) => asset.key);
        const start = keys.indexOf(selectionAnchor);
        const end = keys.indexOf(key);
        if (start >= 0 && end >= 0) {
          return new Set(
            keys.slice(Math.min(start, end), Math.max(start, end) + 1),
          );
        }
      }
      if (intent.toggle) {
        const next = new Set(current);
        if (next.has(key)) next.delete(key);
        else next.add(key);
        return next;
      }
      return new Set([key]);
    });
    setSelectionAnchor(key);
  };

  const editSelection = async (patch: MetadataPatch) => {
    if (selectedAssets.length === 0) return;
    setEditBusy(true);
    try {
      const result = await api.editAssetMetadata({
        targets: selectedAssets.map((asset) => ({
          key: asset.key,
          expectedSidecarDigest: asset.sidecarState?.digest ?? null,
        })),
        patch,
      });
      setAssets((current) => upsertAssets(current, result.updated));
      if (result.failures.length > 0) {
        setNotice({
          tone: "error",
          message: `${result.updated.length} 项已更新，${result.failures.length} 项失败：${result.failures[0].message}`,
        });
      } else {
        setNotice({
          tone: "info",
          message: `已更新 ${result.updated.length} 项素材的 Sidecar`,
        });
      }
    } catch (error) {
      setNotice({
        tone: "error",
        message: `元数据写入失败：${errorMessage(error)}`,
      });
    } finally {
      setEditBusy(false);
    }
  };

  const addRoot = async (path: string, name: string) => {
    setRootBusy(true);
    try {
      const root = await api.addLibraryRoot({ path, name });
      setRoots((current) => [...current, root]);
      setNotice({ tone: "info", message: `已添加 ${root.name}，正在扫描` });
      await runScan(root);
    } catch (error) {
      setNotice({
        tone: "error",
        message: `添加素材根失败：${errorMessage(error)}`,
      });
      throw error;
    } finally {
      setRootBusy(false);
    }
  };

  const toggleRoot = async (root: LibraryRootStatus) => {
    setRootBusy(true);
    try {
      const updated = await api.updateLibraryRoot({
        id: root.id,
        enabled: !root.enabled,
      });
      setRoots((current) =>
        current.map((candidate) =>
          candidate.id === root.id ? updated : candidate,
        ),
      );
      if (updated.enabled && updated.accessStatus === "available") {
        await runScan(updated);
      } else {
        setAssets((current) => removeRootAssets(current, root.id));
      }
    } catch (error) {
      setNotice({
        tone: "error",
        message: `素材根更新失败：${errorMessage(error)}`,
      });
    } finally {
      setRootBusy(false);
    }
  };

  const removeRoot = async (root: LibraryRootStatus) => {
    setRootBusy(true);
    try {
      await api.removeLibraryRoot(root.id);
      setRoots((current) =>
        current.filter((candidate) => candidate.id !== root.id),
      );
      setAssets((current) => removeRootAssets(current, root.id));
      setNotice({
        tone: "info",
        message: `已移除 ${root.name} 的配置，磁盘文件保持不变`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `移除素材根失败：${errorMessage(error)}`,
      });
      throw error;
    } finally {
      setRootBusy(false);
    }
  };

  const changeTagFilter = (tag: string, state: TagFilterState) => {
    setTagFilters((current) => {
      const next = { ...current };
      const nextState = cycleTagFilter(state);
      if (nextState === "neutral") delete next[tag];
      else next[tag] = nextState;
      return next;
    });
  };

  const openRootManager = () => {
    setRootManagerOpen(true);
    void api
      .listLibraryRoots()
      .then(setRoots)
      .catch((error: unknown) =>
        setNotice({
          tone: "error",
          message: `素材位置状态刷新失败：${errorMessage(error)}`,
        }),
      );
  };

  const openVaultManager = () => {
    setVaultManagerOpen(true);
    void api
      .listObsidianVaults()
      .then((value) => {
        setVaults(value);
        setActiveVaultId((current) =>
          current && value.some((vault) => vault.id === current)
            ? current
            : value.find(
                (vault) => vault.enabled && vault.accessStatus === "available",
              )?.id,
        );
      })
      .catch((error: unknown) =>
        setNotice({
          tone: "error",
          message: `Vault 状态刷新失败：${errorMessage(error)}`,
        }),
      );
  };

  const addVault = async (path: string, name: string) => {
    setVaultBusy(true);
    try {
      const vault = await api.addObsidianVault({ path, name });
      setVaults((current) => [...current, vault]);
      setActiveVaultId(vault.id);
      setNotice({
        tone: "info",
        message: `已添加 ${vault.name} 并设为 Obsidian 目标 Vault`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `添加 Vault 失败：${errorMessage(error)}`,
      });
      throw error;
    } finally {
      setVaultBusy(false);
    }
  };

  const toggleVault = async (vault: ObsidianVaultStatus) => {
    setVaultBusy(true);
    try {
      const updated = await api.updateObsidianVault({
        id: vault.id,
        enabled: !vault.enabled,
      });
      const next = vaults.map((candidate) =>
        candidate.id === vault.id ? updated : candidate,
      );
      setVaults(next);
      if (!updated.enabled && activeVaultId === updated.id) {
        setActiveVaultId(
          next.find(
            (candidate) =>
              candidate.enabled && candidate.accessStatus === "available",
          )?.id,
        );
      } else if (
        updated.enabled &&
        updated.accessStatus === "available" &&
        activeVaultId === undefined
      ) {
        setActiveVaultId(updated.id);
      }
    } catch (error) {
      setNotice({
        tone: "error",
        message: `更新 Vault 失败：${errorMessage(error)}`,
      });
    } finally {
      setVaultBusy(false);
    }
  };

  const removeVault = async (vault: ObsidianVaultStatus) => {
    setVaultBusy(true);
    try {
      await api.removeObsidianVault(vault.id);
      const next = vaults.filter((candidate) => candidate.id !== vault.id);
      setVaults(next);
      if (activeVaultId === vault.id) {
        setActiveVaultId(
          next.find(
            (candidate) =>
              candidate.enabled && candidate.accessStatus === "available",
          )?.id,
        );
      }
      setNotice({
        tone: "info",
        message: `已移除 ${vault.name} 的授权配置，Vault 文件保持不变`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `移除 Vault 失败：${errorMessage(error)}`,
      });
      throw error;
    } finally {
      setVaultBusy(false);
    }
  };

  const copySelectedVaultReference = async () => {
    const asset = selectedAssets.length === 1 ? selectedAssets[0] : undefined;
    const reference = asset ? vaultReferences.get(asset.key) : undefined;
    if (reference === undefined) return;
    try {
      await copyVaultReference(reference);
      setNotice({
        tone: "info",
        message: `已复制 ${reference.markdown}`,
      });
    } catch (error) {
      setNotice({
        tone: "error",
        message: `复制 Obsidian 引用失败：${errorMessage(error)}`,
      });
    }
  };

  const cancelScans = async () => {
    await Promise.all(
      activeScans.flatMap(([rootId, scan]) =>
        scan.scanId
          ? [
              api.cancelLibraryScan(scan.scanId).then(() => {
                setScans((current) => ({
                  ...current,
                  [rootId]: { ...current[rootId], phase: "cancelled" },
                }));
              }),
            ]
          : [],
      ),
    );
  };

  return (
    <div className="desktop-shell">
      <header className="topbar">
        <a className="brand" href="#library" aria-label="Material Eagle 素材库">
          <span className="brand-mark">
            <Icon name="grid" size={17} />
          </span>
          <span>
            <strong>Material</strong>
            <small>Filesystem library</small>
          </span>
        </a>

        <div className={`search-box${queryError ? " has-error" : ""}`}>
          <Icon name="search" size={17} />
          <input
            aria-describedby={queryError ? "query-error" : undefined}
            aria-label="搜索和过滤素材"
            onChange={(event) => setExpression(event.target.value)}
            placeholder="搜索 Tag，或输入 type:image  /  favorite:true"
            ref={search}
            spellCheck={false}
            value={expression}
          />
          <kbd>/</kbd>
        </div>

        <div className="topbar-actions">
          <button
            className="library-button vault-button"
            onClick={openVaultManager}
            type="button"
          >
            <Icon name="link" size={16} />
            <span>{activeVault?.name ?? "配置 Vault"}</span>
            <Icon name="chevron" size={13} />
          </button>
          <button
            className="library-button"
            onClick={openRootManager}
            type="button"
          >
            <Icon name="library" size={16} />
            <span>{roots.length} 个素材位置</span>
            <Icon name="chevron" size={13} />
          </button>
        </div>
      </header>

      <div className="workspace" id="library">
        <aside className="filter-sidebar" aria-label="素材过滤器">
          <div className="sidebar-heading">
            <span className="overline">Collections</span>
            <h2>全部素材</h2>
            <p>{allAssets.length.toLocaleString()} 项已发现</p>
          </div>

          <nav className="quick-filters" aria-label="快速过滤">
            <button
              className={expression === "" ? "is-active" : ""}
              onClick={() => setExpression("")}
              type="button"
            >
              <Icon name="grid" size={15} /> 全部{" "}
              <span>{allAssets.length}</span>
            </button>
            <button
              className={expression === "favorite:true" ? "is-active" : ""}
              onClick={() => setExpression("favorite:true")}
              type="button"
            >
              <Icon name="star" size={15} /> 已收藏{" "}
              <span>{allAssets.filter((asset) => asset.favorite).length}</span>
            </button>
            <button
              className={expression === "type:image" ? "is-active" : ""}
              onClick={() => setExpression("type:image")}
              type="button"
            >
              <Icon name="image" size={15} /> 图片{" "}
              <span>
                {allAssets.filter((asset) => asset.kind === "image").length}
              </span>
            </button>
          </nav>

          <section
            className="tag-filter-panel"
            aria-labelledby="tag-filter-title"
          >
            <header>
              <div>
                <span className="overline">Tags</span>
                <h2 id="tag-filter-title">三态筛选</h2>
              </div>
              {activeTagFilters.length > 0 ? (
                <button
                  aria-label="清除 Tag 筛选"
                  onClick={() => setTagFilters({})}
                  type="button"
                >
                  清除
                </button>
              ) : null}
            </header>
            <p className="tag-legend">
              <span>点击：包含</span>
              <span>再次：排除</span>
            </p>
            <div className="tag-list">
              {tags.map((summary) => (
                <button
                  aria-label={`${summary.tag}，${summary.state === "neutral" ? "未筛选" : summary.state === "include" ? "包含" : "排除"}`}
                  className={`tag-filter tag-filter--${summary.state}`}
                  key={summary.tag}
                  onClick={() => changeTagFilter(summary.tag, summary.state)}
                  type="button"
                >
                  <span className="tag-state">
                    {summary.state === "include" ? (
                      <Icon name="check" size={11} />
                    ) : summary.state === "exclude" ? (
                      <Icon name="minus" size={11} />
                    ) : null}
                  </span>
                  <span title={summary.tag}>{summary.tag}</span>
                  <small>{summary.count}</small>
                </button>
              ))}
              {tags.length === 0 ? (
                <p className="muted-copy">扫描完成后，Tag 会显示在这里。</p>
              ) : null}
            </div>
          </section>

          <footer className="sidebar-footer">
            <span className="runtime-dot" />
            <span>
              v{buildInfo?.version ?? "0.1.0"} ·{" "}
              {isTauriRuntime() ? "LOCAL" : "PREVIEW"}
            </span>
          </footer>
        </aside>

        <main className="library-main">
          <div className="library-heading">
            <div>
              <span className="overline">Flat library</span>
              <h1>素材视图</h1>
            </div>
            <div className="library-actions">
              {selected.size > 0 ? (
                <span className="selection-count">
                  已选择 {selected.size} 项
                </span>
              ) : null}
              <button
                className="text-button"
                disabled={visibleAssets.length === 0}
                onClick={() =>
                  setSelected(new Set(visibleAssets.map((asset) => asset.key)))
                }
                type="button"
              >
                全选当前结果
              </button>
              {selected.size > 0 ? (
                <button
                  className="icon-button"
                  aria-label="清除选择"
                  onClick={() => setSelected(new Set())}
                  type="button"
                >
                  <Icon name="close" size={15} />
                </button>
              ) : null}
            </div>
          </div>

          {activeTagFilters.length > 0 || expression.trim().length > 0 ? (
            <div className="active-filter-row" aria-label="当前过滤条件">
              <span>当前条件</span>
              {expression.trim().length > 0 ? (
                <button onClick={() => setExpression("")} type="button">
                  {expression}
                  <Icon name="close" size={11} />
                </button>
              ) : null}
              {activeTagFilters.map((filter) => (
                <button
                  key={filter.tag}
                  onClick={() =>
                    setTagFilters((current) => {
                      const next = { ...current };
                      delete next[filter.tag];
                      return next;
                    })
                  }
                  type="button"
                >
                  {filter.state === "exclude" ? "排除 " : "包含 "}
                  {filter.tag}
                  <Icon name="close" size={11} />
                </button>
              ))}
            </div>
          ) : null}

          {queryError ? (
            <div className="query-error" id="query-error" role="alert">
              <Icon name="alert" size={15} />
              {queryError}
            </div>
          ) : null}
          {notice ? (
            <div className={`notice notice--${notice.tone}`} role="status">
              <Icon
                name={notice.tone === "error" ? "alert" : "check"}
                size={15}
              />
              <span>{notice.message}</span>
              <button
                aria-label="关闭通知"
                onClick={() => setNotice(undefined)}
                type="button"
              >
                <Icon name="close" size={13} />
              </button>
            </div>
          ) : null}

          <div className="result-meta" aria-live="polite">
            <span>
              {queryPending ? "正在过滤…" : `${visibleAssets.length} 项结果`}
            </span>
            {activeScans.length > 0 ? (
              <span className="scan-progress">
                <i /> 正在扫描，已发现{" "}
                {activeScans.reduce(
                  (sum, [, scan]) => sum + scan.assetCount,
                  0,
                )}{" "}
                项
                <button
                  disabled={activeScans.some(([, scan]) => !scan.scanId)}
                  onClick={() => void cancelScans()}
                  type="button"
                >
                  停止
                </button>
              </span>
            ) : failedScans.length > 0 ? (
              <span className="scan-failure" role="alert">
                <Icon name="alert" size={13} />
                扫描失败：{failedScans[0][1].message ?? "请检查素材位置"}
              </span>
            ) : null}
          </div>

          {booting ? (
            <EmptyState
              icon="refresh"
              title="正在读取素材配置"
              copy="所有内容都从本地文件系统解释，不读取远程数据库。"
            />
          ) : roots.length === 0 ? (
            <EmptyState
              icon="folder"
              title="从一个素材文件夹开始"
              copy="添加现有目录后，文件会原地保留；应用只建立可重建视图。"
              action="添加素材位置"
              onAction={openRootManager}
            />
          ) : inaccessibleRoots.length > 0 &&
            allAssets.length === 0 &&
            activeScans.length === 0 ? (
            <EmptyState
              icon="alert"
              title="素材位置当前不可访问"
              copy={`${inaccessibleRoots[0].name}：${inaccessibleRoots[0].accessMessage ?? "请检查权限或磁盘连接"}`}
              action="检查素材位置"
              onAction={openRootManager}
            />
          ) : activeScans.length > 0 && allAssets.length === 0 ? (
            <EmptyState
              icon="refresh"
              title="正在解释文件与 Sidecar"
              copy="素材会按批次出现，缩略图只在进入视口后生成。"
            />
          ) : visibleAssets.length === 0 ? (
            <EmptyState
              icon="search"
              title="没有符合条件的素材"
              copy="尝试清除排除条件，或检查查询语法。"
              action="清除全部筛选"
              onAction={() => {
                setExpression("");
                setTagFilters({});
              }}
            />
          ) : (
            <AssetGrid
              api={api}
              assets={visibleAssets}
              onSelect={selectAsset}
              selected={selected}
              vaultReferences={vaultReferences}
            />
          )}
        </main>

        <Inspector
          assets={selectedAssets}
          busy={editBusy}
          obsidian={{
            vault: activeVault,
            reference:
              selectedAssets.length === 1
                ? vaultReferences.get(selectedAssets[0].key)
                : undefined,
            failure:
              selectedAssets.length === 1
                ? vaultReferenceFailures.get(selectedAssets[0].key)
                : undefined,
            pending: vaultReferencesPending,
            onCopy: copySelectedVaultReference,
            onConfigure: openVaultManager,
          }}
          onEdit={editSelection}
        />
      </div>

      <RootManager
        busy={rootBusy}
        onAdd={addRoot}
        onClose={() => setRootManagerOpen(false)}
        onRemove={removeRoot}
        onScan={runScan}
        onToggle={toggleRoot}
        open={rootManagerOpen}
        roots={roots}
      />
      <VaultManager
        activeVaultId={activeVaultId}
        busy={vaultBusy}
        onAdd={addVault}
        onClose={() => setVaultManagerOpen(false)}
        onRemove={removeVault}
        onSelect={setActiveVaultId}
        onToggle={toggleVault}
        open={vaultManagerOpen}
        vaults={vaults}
      />
    </div>
  );
}

function handleScanEvent(
  rootId: string,
  event: LibraryScanEvent,
  setAssets: React.Dispatch<React.SetStateAction<Map<string, AssetRecord>>>,
  setScans: React.Dispatch<React.SetStateAction<Record<string, ScanUiState>>>,
) {
  if (event.event === "started") {
    setScans((current) => ({
      ...current,
      [rootId]: {
        scanId: event.data.scanId,
        phase: "scanning",
        visitedFiles: 0,
        assetCount: 0,
        problemCount: 0,
      },
    }));
  } else if (event.event === "batch") {
    setAssets((current) => upsertAssets(current, event.data.batch.assets));
    setScans((current) => ({
      ...current,
      [rootId]: {
        ...(current[rootId] ?? {
          phase: "scanning",
          assetCount: 0,
          problemCount: 0,
        }),
        phase: "scanning",
        visitedFiles: event.data.batch.visitedFiles,
        assetCount:
          (current[rootId]?.assetCount ?? 0) + event.data.batch.assets.length,
        problemCount:
          (current[rootId]?.problemCount ?? 0) +
          event.data.batch.problems.length,
      },
    }));
  } else if (event.event === "finished") {
    setScans((current) => ({
      ...current,
      [rootId]: {
        scanId: event.data.scanId,
        phase:
          event.data.summary.completion === "cancelled"
            ? "cancelled"
            : "completed",
        visitedFiles: event.data.summary.visitedFiles,
        assetCount: event.data.summary.assetCount,
        problemCount: event.data.summary.problemCount,
      },
    }));
  } else {
    setScans((current) => ({
      ...current,
      [rootId]: {
        scanId: event.data.scanId,
        phase: "failed",
        visitedFiles: current[rootId]?.visitedFiles ?? 0,
        assetCount: current[rootId]?.assetCount ?? 0,
        problemCount: current[rootId]?.problemCount ?? 0,
        message: event.data.message,
      },
    }));
  }
}

function EmptyState({
  icon,
  title,
  copy,
  action,
  onAction,
}: {
  icon: "alert" | "folder" | "refresh" | "search";
  title: string;
  copy: string;
  action?: string;
  onAction?: () => void;
}) {
  return (
    <section className="empty-state">
      <span>
        <Icon name={icon} size={22} />
      </span>
      <h2>{title}</h2>
      <p>{copy}</p>
      {action && onAction ? (
        <button className="primary-button" onClick={onAction} type="button">
          {action}
        </button>
      ) : null}
    </section>
  );
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (typeof error === "object" && error !== null && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return "未知错误";
}
