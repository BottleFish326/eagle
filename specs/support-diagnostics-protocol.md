# 支持诊断协议

本文定义 P2-07 的滚动结构化日志、诊断导出、一致性报告和素材 ID 追踪边界。

## 1. 运行日志

固定目录为应用日志目录下的 `diagnostics/runtime`：

```text
runtime-events.jsonl
runtime-events.1.jsonl
runtime-events.2.jsonl
runtime-events.3.jsonl
runtime-events.4.jsonl
```

每行是一个独立 JSON 事件，包含时间、等级、category、code 和有界 details。单文件上限 1 MiB，最多 5 个文件。轮换目标必须是普通文件；不得追随符号链接或删除非普通条目。

事件输入约束：category/code 最多 64 字符，details 最多 16 项，键最多 64 字符，值最多 256 字符。绝对路径、`~/`、UNC、Windows 盘符以及用户目录形式会被整体替换为 `[redacted-path:<16位指纹>]`。

## 2. 诊断导出 Schema 2

`export_diagnostics` 保留原子写入和固定后端目录边界。Schema 2 包含：

- 构建版本、Git 提交、目标、Profile、Rust 工具链和平台；
- 配置数量、匿名根/Vault 访问状态和短路径指纹；
- 缓存状态、目录素材数、活动扫描数；
- 活动扫描/监听、调度活动/等待/峰值、缓存项数和字节数；
- 当前滚动日志文件数、字节数与配置上限；
- 最近 256 条事件，以及 warning/error 按等级、category、code 的聚合与最后时间。

禁止字段仍包括绝对路径、文件名、查询原文、Tag、备注、别名、Sidecar 正文和缩略图正文。

## 3. 只读库一致性报告

```text
inspect_library_consistency() -> LibraryConsistencyReport
```

命令不接受路径。它快照所有配置根与运行期目录，检查：启用根的可访问性、记录根关联、素材是否为普通文件、素材是否位于授权根、Sidecar 是否相邻和存在、稳定 ID 是否重复、孤立 Sidecar、内容确认的重联候选、同步冲突副本，以及扫描阶段已经隔离的解析问题。根遍历复用现有只读重联检查，并受统一哈希资源许可约束。

报告最多携带 512 条明细，完整 warning/error 数仍保留。若存在活动扫描或任一可用根尚未完成权威扫描，`authoritative` 为 `false`。命令不执行修复。

## 4. 素材追踪

```text
trace_asset_support(assetId: UUID) -> AssetTraceReport
```

命令只接受稳定 ID。返回目录查找、根关联、素材普通文件检查、相邻 Sidecar 重新解析、Sidecar ID 比对和扫描问题六类步骤。匹配 0 项或多项都明确显示，不猜测记录。输出仅包含相对路径、短指纹、MIME、访问状态和问题 code。

## 5. 失败语义

- 滚动日志写入失败不改变业务操作结果；事件仍尽力保留在有界内存中；
- 诊断导出失败不修改已有日志、配置或素材；
- 一致性与追踪期间文件变化可能产生时间点差异，下一次完整扫描负责收敛；
- 任一支持命令都不创建素材快照、Sidecar 或修复事务。
