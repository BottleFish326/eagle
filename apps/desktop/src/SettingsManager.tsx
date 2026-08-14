import { useEffect, useRef, useState } from "react";

import {
  cacheStartupLabel,
  type ApplicationConfig,
  type DerivedStateResetReport,
  type DiagnosticExportReport,
  type RuntimeRecoveryStatus,
} from "./application-runtime";
import { Icon } from "./Icon";
import { formatBytes } from "./ui-model";

export function SettingsManager({
  open,
  config,
  recovery,
  resetReport,
  diagnosticReport,
  scanActive,
  resetBusy,
  diagnosticBusy,
  onClose,
  onResetView,
  onResetDerived,
  onExportDiagnostics,
  onCopyDiagnosticPath,
}: {
  open: boolean;
  config?: ApplicationConfig;
  recovery?: RuntimeRecoveryStatus;
  resetReport?: DerivedStateResetReport;
  diagnosticReport?: DiagnosticExportReport;
  scanActive: boolean;
  resetBusy: boolean;
  diagnosticBusy: boolean;
  onClose: () => void;
  onResetView: () => void;
  onResetDerived: () => Promise<void>;
  onExportDiagnostics: () => Promise<void>;
  onCopyDiagnosticPath: () => Promise<void>;
}) {
  const closeButton = useRef<HTMLButtonElement>(null);
  const [confirmReset, setConfirmReset] = useState(false);

  useEffect(() => {
    if (open) closeButton.current?.focus();
    else setConfirmReset(false);
  }, [open]);

  if (!open) return null;

  return (
    <div className="modal-backdrop" onMouseDown={onClose} role="presentation">
      <section
        aria-labelledby="settings-dialog-title"
        aria-modal="true"
        className="library-dialog settings-dialog"
        onMouseDown={(event) => event.stopPropagation()}
        role="dialog"
      >
        <header className="dialog-heading">
          <div>
            <span className="overline">Recovery &amp; support</span>
            <h2 id="settings-dialog-title">设置与恢复</h2>
            <p>仅管理应用偏好与可重建数据；素材和 Sidecar 不在清理边界内。</p>
          </div>
          <button
            aria-label="关闭设置与恢复"
            className="icon-button"
            onClick={onClose}
            ref={closeButton}
            type="button"
          >
            <Icon name="close" />
          </button>
        </header>

        <div className="settings-sections">
          <section className="settings-card">
            <div className="settings-card-heading">
              <span className="settings-card-icon">
                <Icon name="search" size={17} />
              </span>
              <div>
                <h3>保存的素材视图</h3>
                <p>查询、Tag 三态和当前 Vault 会自动写入可读配置。</p>
              </div>
            </div>
            <dl className="settings-facts">
              <div>
                <dt>查询</dt>
                <dd>{config?.ui.query || "未设置"}</dd>
              </div>
              <div>
                <dt>Tag 条件</dt>
                <dd>{Object.keys(config?.ui.tagFilters ?? {}).length} 项</dd>
              </div>
            </dl>
            <button className="wide-action" onClick={onResetView} type="button">
              清除保存的视图条件
            </button>
          </section>

          <section className="settings-card">
            <div className="settings-card-heading">
              <span className="settings-card-icon">
                <Icon name="refresh" size={17} />
              </span>
              <div>
                <h3>派生缓存</h3>
                <p>
                  {recovery
                    ? cacheStartupLabel(recovery.cacheStartup)
                    : "正在读取缓存状态…"}
                </p>
              </div>
            </div>
            {resetReport ? (
              <p className="settings-result" role="status">
                上次重建移除 {resetReport.cache.removedFiles} 个缓存文件（
                {formatBytes(resetReport.cache.removedBytes)}
                ），并从文件重新解释
                {resetReport.catalogAssetsRemoved} 项素材。
              </p>
            ) : null}
            <button
              className={`wide-action${confirmReset ? " wide-action--danger" : ""}`}
              disabled={resetBusy || scanActive}
              onClick={() => {
                if (!confirmReset) {
                  setConfirmReset(true);
                  return;
                }
                setConfirmReset(false);
                void onResetDerived();
              }}
              type="button"
            >
              <Icon name="refresh" size={15} />
              {resetBusy
                ? "正在重建…"
                : confirmReset
                  ? "再次点击确认清理并重建"
                  : "清理缓存并重建"}
            </button>
            {scanActive ? <small>扫描完成或停止后才能安全重建。</small> : null}
          </section>

          <section className="settings-card">
            <div className="settings-card-heading">
              <span className="settings-card-icon">
                <Icon name="folder" size={17} />
              </span>
              <div>
                <h3>诊断日志</h3>
                <p>
                  导出构建、缓存、访问状态和最近事件；不包含素材路径、Tag
                  或备注。
                </p>
              </div>
            </div>
            <button
              className="wide-action wide-action--accent"
              disabled={diagnosticBusy}
              onClick={() => void onExportDiagnostics()}
              type="button"
            >
              <Icon name="folder" size={15} />
              {diagnosticBusy ? "正在导出…" : "导出诊断日志"}
            </button>
            {diagnosticReport ? (
              <div className="diagnostic-result" role="status">
                <span>
                  已导出 {diagnosticReport.eventCount} 条事件 ·{" "}
                  {formatBytes(diagnosticReport.sizeBytes)}
                </span>
                <code title={diagnosticReport.path}>
                  {diagnosticReport.path}
                </code>
                <button
                  className="text-action"
                  onClick={() => void onCopyDiagnosticPath()}
                  type="button"
                >
                  复制文件路径
                </button>
              </div>
            ) : null}
          </section>

          <section className="settings-card settings-card--paths">
            <div className="settings-card-heading">
              <span className="settings-card-icon">
                <Icon name="settings" size={17} />
              </span>
              <div>
                <h3>应用数据位置</h3>
                <p>配置是用户状态；缓存和诊断可独立删除。</p>
              </div>
            </div>
            <dl className="settings-facts settings-facts--paths">
              <div>
                <dt>配置</dt>
                <dd title={recovery?.paths.configDirectory}>
                  {recovery?.paths.configDirectory ?? "—"}
                </dd>
              </div>
              <div>
                <dt>缓存</dt>
                <dd title={recovery?.paths.cacheDirectory}>
                  {recovery?.paths.cacheDirectory ?? "—"}
                </dd>
              </div>
              <div>
                <dt>日志</dt>
                <dd title={recovery?.paths.logDirectory}>
                  {recovery?.paths.logDirectory ?? "—"}
                </dd>
              </div>
            </dl>
          </section>
        </div>
      </section>
    </div>
  );
}
