# Architecture Decision Records

本目录保存会影响数据安全、兼容性和长期维护的架构决策。

状态定义：

- `Proposed`：待评审；
- `Accepted`：已接受并约束实现；
- `Superseded`：已被新 ADR 替代；
- `Deprecated`：不再推荐，但可能为兼容保留。

修改已接受的决定时，不直接重写历史结论；创建新的 ADR，并在旧 ADR 中标记替代关系。

| ADR | 决策 | 状态 |
|---|---|---|
| [ADR-001](001-filesystem-source-of-truth.md) | 文件系统真相源与无权威数据库 | Accepted |
| [ADR-002](002-sidecar-format-and-identity.md) | Sidecar 格式、ID 与写入规则 | Accepted |
| [ADR-003](003-derived-cache-policy.md) | 派生缓存边界 | Accepted |
| [ADR-004](004-move-and-reconciliation.md) | 文件移动与重新关联 | Accepted |
| [ADR-005](005-obsidian-reference-strategy.md) | Obsidian 引用策略 | Accepted |
| [ADR-006](006-desktop-technology-stack.md) | 桌面端技术栈 | Accepted |
| [ADR-007](007-filesystem-boundaries.md) | 符号链接、网络盘与同步目录 | Accepted |
| [ADR-008](008-external-asset-rendering-security.md) | Vault 外素材渲染安全边界 | Accepted |
| [ADR-009](009-library-root-configuration.md) | 素材库根目录配置与授权状态 | Accepted |
| [ADR-010](010-asset-record-and-scan-protocol.md) | 统一素材模型、字段优先级与增量扫描协议 | Accepted |
| [ADR-011](011-sidecar-edit-and-catalog-consistency.md) | Sidecar 编辑、并发控制与目录一致性 | Accepted |
| [ADR-012](012-query-language-and-index-semantics.md) | 查询语言与内存索引语义 | Accepted |
| [ADR-013](013-thumbnail-pipeline-and-cache-layout.md) | 缩略图管线、并发与缓存布局 | Accepted |
| [ADR-014](014-desktop-ui-state-and-api-boundary.md) | 桌面 UI 瞬态状态、Tauri API 与开发预览边界 | Accepted |
| [ADR-015](015-obsidian-vault-configuration-and-native-reference.md) | Obsidian Vault 配置、路径解析、复制与拖放边界 | Accepted |
| [ADR-016](016-application-configuration-and-recovery-boundary.md) | 应用配置、缓存重建、诊断导出与恢复边界 | Accepted |
| [ADR-017](017-virtual-asset-grid-window.md) | 素材网格基础窗口化与阶段 1 规模门禁 | Accepted |
| [ADR-018](018-bounded-filesystem-event-reconciliation.md) | 有界文件事件归一化与一致性扫描 | Accepted |
| [ADR-019](019-stable-identity-and-explicit-relink.md) | 稳定身份扫描收敛与显式 Sidecar 重新关联 | Accepted |
| [ADR-020](020-durable-batch-metadata-transactions.md) | 纯文件批量元数据事务与条件恢复 | Accepted |
| [ADR-021](021-explicit-metadata-and-sync-conflicts.md) | 显式元数据合并与同步冲突副本诊断 | Accepted |
| [ADR-022](022-bounded-thumbnail-cache-lifecycle.md) | 有界缩略图缓存生命周期与中断安全清理 | Accepted |
| [ADR-023](023-unified-bounded-resource-scheduling.md) | 统一有界资源调度与可观测稳定性 | Accepted |
| [ADR-024](024-bounded-privacy-safe-support-diagnostics.md) | 有界、脱敏的支持诊断 | Accepted |
| [ADR-025](025-platform-path-and-offline-root-semantics.md) | 平台路径身份与离线根目录语义 | Accepted |
