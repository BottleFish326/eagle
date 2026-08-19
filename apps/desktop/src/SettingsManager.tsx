import { useEffect, useRef, useState } from "react";

import {
  cacheStartupLabel,
  type ApplicationConfig,
  type DerivedStateResetReport,
  type DiagnosticExportReport,
  type RuntimeRecoveryStatus,
} from "./application-runtime";
import { Icon } from "./Icon";
import type { MetadataTransactionSummary } from "./metadata-transactions";
import type {
  AssetTraceReport,
  LibraryConsistencyReport,
} from "./support-tools";
import type { CacheMaintenanceReport } from "./thumbnail";
import { formatBytes } from "./ui-model";

export function SettingsManager({
  open,
  config,
  recovery,
  cacheMaintenanceReport,
  resetReport,
  diagnosticReport,
  consistencyReport,
  assetTraceReport,
  scanActive,
  resetBusy,
  cacheMaintenanceBusy,
  diagnosticBusy,
  supportBusy,
  onClose,
  onResetView,
  onResetDerived,
  onMaintainCache,
  onExportDiagnostics,
  onInspectConsistency,
  onTraceAsset,
  onCopyDiagnosticPath,
  transactions,
  transactionBusy,
  onContinueTransaction,
  onRestoreTransaction,
  onDismissTransaction,
}: {
  open: boolean;
  config?: ApplicationConfig;
  recovery?: RuntimeRecoveryStatus;
  cacheMaintenanceReport?: CacheMaintenanceReport;
  resetReport?: DerivedStateResetReport;
  diagnosticReport?: DiagnosticExportReport;
  consistencyReport?: LibraryConsistencyReport;
  assetTraceReport?: AssetTraceReport;
  scanActive: boolean;
  resetBusy: boolean;
  cacheMaintenanceBusy: boolean;
  diagnosticBusy: boolean;
  supportBusy?: "consistency" | "trace";
  transactions: readonly MetadataTransactionSummary[];
  transactionBusy?: string;
  onClose: () => void;
  onResetView: () => void;
  onResetDerived: () => Promise<void>;
  onMaintainCache: () => Promise<void>;
  onExportDiagnostics: () => Promise<void>;
  onInspectConsistency: () => Promise<void>;
  onTraceAsset: (assetId: string) => Promise<void>;
  onCopyDiagnosticPath: () => Promise<void>;
  onContinueTransaction: (
    transaction: MetadataTransactionSummary,
  ) => Promise<void>;
  onRestoreTransaction: (
    transaction: MetadataTransactionSummary,
  ) => Promise<void>;
  onDismissTransaction: (
    transaction: MetadataTransactionSummary,
  ) => Promise<void>;
}) {
  const closeButton = useRef<HTMLButtonElement>(null);
  const [confirmReset, setConfirmReset] = useState(false);
  const [confirmDismiss, setConfirmDismiss] = useState<string>();
  const [traceAssetId, setTraceAssetId] = useState("");

  useEffect(() => {
    if (open) closeButton.current?.focus();
    else {
      setConfirmReset(false);
      setConfirmDismiss(undefined);
    }
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
                <h3>一致性与素材追踪</h3>
                <p>
                  只读核对当前目录与文件状态；报告使用相对路径和短指纹，不修改素材或
                  Sidecar。
                </p>
              </div>
            </div>
            <button
              className="wide-action"
              disabled={supportBusy !== undefined}
              onClick={() => void onInspectConsistency()}
              type="button"
            >
              <Icon name="refresh" size={15} />
              {supportBusy === "consistency" ? "正在检查…" : "检查库一致性"}
            </button>
            {consistencyReport ? (
              <div className="support-result" role="status">
                <strong>
                  {consistencyReport.authoritative
                    ? "权威快照"
                    : "扫描尚未收敛"}{" "}
                  · {consistencyReport.summary.catalogAssets} 项素材
                </strong>
                <span>
                  {consistencyReport.summary.errors} 个错误 ·{" "}
                  {consistencyReport.summary.warnings} 个警告
                  {consistencyReport.truncated ? " · 明细已截断" : ""}
                </span>
                {consistencyReport.findings.length > 0 ? (
                  <ul className="support-findings">
                    {consistencyReport.findings
                      .slice(0, 8)
                      .map((finding, index) => (
                        <li key={`${finding.code}-${String(index)}`}>
                          <code>{finding.code}</code>
                          <span>
                            {finding.relativePath ??
                              finding.pathFingerprint ??
                              finding.message}
                          </span>
                        </li>
                      ))}
                  </ul>
                ) : (
                  <span>当前快照未发现一致性问题。</span>
                )}
              </div>
            ) : null}
            <label className="support-trace-field">
              <span>按稳定素材 ID 追踪</span>
              <input
                onChange={(event) => setTraceAssetId(event.target.value)}
                placeholder="UUIDv7"
                spellCheck={false}
                value={traceAssetId}
              />
            </label>
            <button
              className="wide-action"
              disabled={supportBusy !== undefined || traceAssetId.trim() === ""}
              onClick={() => void onTraceAsset(traceAssetId.trim())}
              type="button"
            >
              <Icon name="search" size={15} />
              {supportBusy === "trace" ? "正在追踪…" : "查看解析与关联过程"}
            </button>
            {assetTraceReport ? (
              <div className="support-result" role="status">
                <strong>
                  匹配 {assetTraceReport.matchCount} 条目录记录 ·{" "}
                  {assetTraceReport.assetId}
                </strong>
                <ol className="support-trace-steps">
                  {assetTraceReport.steps.slice(0, 12).map((step, index) => (
                    <li
                      data-outcome={step.outcome}
                      key={`${step.stage}-${String(index)}`}
                    >
                      <code>{step.stage}</code>
                      <span>{step.message}</span>
                    </li>
                  ))}
                </ol>
              </div>
            ) : null}
          </section>

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

          <section className="settings-card settings-card--transactions">
            <div className="settings-card-heading">
              <span className="settings-card-icon">
                <Icon name="refresh" size={17} />
              </span>
              <div>
                <h3>批量事务恢复</h3>
                <p>
                  每次多选编辑先写入纯文件计划；继续和恢复都按 Sidecar
                  摘要检查外部修改。
                </p>
              </div>
            </div>
            {transactions.length === 0 ? (
              <p className="transaction-empty">当前没有保留的批量事务日志。</p>
            ) : (
              <div className="transaction-list">
                {transactions.map((transaction) => (
                  <article className="transaction-row" key={transaction.id}>
                    <div className="transaction-copy">
                      <strong>{transactionStateLabel(transaction)}</strong>
                      <span>
                        {transaction.itemCount} 项 · 已应用{" "}
                        {transaction.appliedCount}
                        {transaction.restoredCount > 0
                          ? ` · 已恢复 ${transaction.restoredCount}`
                          : ""}
                        {transaction.failedCount > 0
                          ? ` · 失败 ${transaction.failedCount}`
                          : ""}
                        {transaction.conflictCount > 0
                          ? ` · 冲突 ${transaction.conflictCount}`
                          : ""}
                      </span>
                      <small>
                        {formatTransactionTime(transaction.updatedAt)}
                      </small>
                    </div>
                    <div className="transaction-actions">
                      {transaction.state === "active" ||
                      transaction.state === "conflict" ? (
                        <button
                          className="text-action"
                          disabled={scanActive || transactionBusy !== undefined}
                          onClick={() =>
                            void onContinueTransaction(transaction)
                          }
                          type="button"
                        >
                          继续安全项
                        </button>
                      ) : null}
                      {transaction.appliedCount > 0 ? (
                        <button
                          className="text-action"
                          disabled={scanActive || transactionBusy !== undefined}
                          onClick={() => void onRestoreTransaction(transaction)}
                          type="button"
                        >
                          安全恢复
                        </button>
                      ) : null}
                      <button
                        className={
                          confirmDismiss === transaction.id
                            ? "text-action text-action--danger"
                            : "text-action"
                        }
                        disabled={transactionBusy !== undefined}
                        onClick={() => {
                          if (confirmDismiss !== transaction.id) {
                            setConfirmDismiss(transaction.id);
                            return;
                          }
                          setConfirmDismiss(undefined);
                          void onDismissTransaction(transaction);
                        }}
                        type="button"
                      >
                        {confirmDismiss === transaction.id
                          ? "确认仅删除日志"
                          : "不再保留"}
                      </button>
                    </div>
                  </article>
                ))}
              </div>
            )}
            {scanActive ? <small>扫描结束后才能继续或恢复事务。</small> : null}
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
            {recovery ? (
              <dl className="settings-facts">
                <div>
                  <dt>缓存条目</dt>
                  <dd>
                    {recovery.cacheStats.entryCount} /{" "}
                    {recovery.cacheStats.maxEntries}
                  </dd>
                </div>
                <div>
                  <dt>缓存空间</dt>
                  <dd>
                    {formatBytes(recovery.cacheStats.byteCount)} /{" "}
                    {formatBytes(recovery.cacheStats.maxBytes)}
                  </dd>
                </div>
                <div>
                  <dt>保留周期</dt>
                  <dd>{recovery.cacheStats.retentionDays} 天</dd>
                </div>
                <div>
                  <dt>解码器</dt>
                  <dd>{recovery.cacheStats.decoderVersion}</dd>
                </div>
              </dl>
            ) : null}
            {cacheMaintenanceReport ? (
              <p className="settings-result" role="status">
                上次回收移除 {cacheMaintenanceReport.removedEntries}{" "}
                个缓存条目（
                {formatBytes(
                  cacheMaintenanceReport.removedBytes,
                )}），当前保留 {cacheMaintenanceReport.stats.entryCount} 项。
              </p>
            ) : null}
            {resetReport ? (
              <p className="settings-result" role="status">
                上次重建移除 {resetReport.cache.removedFiles} 个缓存文件（
                {formatBytes(resetReport.cache.removedBytes)}
                ），并从文件重新解释
                {resetReport.catalogAssetsRemoved} 项素材。
              </p>
            ) : null}
            <button
              className="wide-action"
              disabled={cacheMaintenanceBusy || resetBusy || scanActive}
              onClick={() => void onMaintainCache()}
              type="button"
            >
              <Icon name="refresh" size={15} />
              {cacheMaintenanceBusy ? "正在回收…" : "立即回收无效缓存"}
            </button>
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

function transactionStateLabel(
  transaction: MetadataTransactionSummary,
): string {
  switch (transaction.state) {
    case "active":
      return "操作中断，可继续或恢复";
    case "completed":
      return "批量操作已完成";
    case "restored":
      return "已恢复到事务前状态";
    case "conflict":
      return "存在外部修改，未覆盖";
  }
}

function formatTransactionTime(value: string): string {
  const timestamp = Date.parse(value);
  return Number.isNaN(timestamp)
    ? value
    : new Intl.DateTimeFormat("zh-CN", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
      }).format(timestamp);
}
