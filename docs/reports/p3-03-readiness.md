# P3-03 保存过滤器实施准备报告

> 状态：Design ready；阶段 2 退出前不计作阶段 3 开始
>
> 日期：2026-08-20

## 1. 准备结论

P3-03 已固定用户级 YAML 所有权、Schema、条目隔离、未知字段保留、完整版本并发、scope/sort 执行、精确 Tag AST 重写和跨 Sidecar/filter 的纯文件恢复日志。保存项不包含素材列表，删除缓存后始终由当前文件系统重算。

## 2. 已完成资产

| 资产 | 结论 |
|---|---|
| ADR-027 | 固定用户配置目录、无结果快照、根 ID scope 和显式 Tag 影响选择 |
| `schemas/saved-filters.schema.json` | 固定 Schema 1、512 项、UUIDv7、query、scope、sort 和时间 |
| `specs/saved-filter-protocol.md` | 固定加载隔离、原子写入、AST span 重写和协调恢复状态机 |
| `specs/examples/saved-filters.example.yml` | 覆盖 all-enabled、selected-roots、两种 sort 和未知字段保留 |
| ADR-020/029 | 提供 plan-first Sidecar 事务、条件恢复和精确选择/预检基础 |

Schema 已使用 Draft 2020-12 严格编译；YAML 示例转换为普通 JSON 值后通过。负例覆盖非法 UUIDv7、空 selected roots、多行 query、非法 sort 和缺失必填字段。ID/name 唯一、query parse、canonical root ID、未知根与 AST rewrite 属于语义验证器范围。

## 3. 实施切片

### P3-03A：SavedFilterStore

- 固定配置路径和 1 MiB/512 项限制；
- 安全 YAML parse、Value 树与未知字段保留；
- 完整文件版本、确定性原子写入和 expected-absent；
- 单项 create/update/rename/delete API。

### P3-03B：加载隔离与执行

- duplicate ID/name 全部隔离；
- invalid query/sort 与 unavailable root 区分；
- all-enabled/selected scope 和 P3-07 sort；
- 重启/删缓存后重扫执行。

### P3-03C：Tag 引用分析

- parser 输出精确 Tag node byte span；
- 后向 span rewrite 和重解析等价审计；
- affected filter before/after 预览；
- update/retain/cancel 逐项选择。

### P3-03D：Rename coordinator

- metadata transaction plan-only 扩展；
- `tag-renames-v1` 私密纯文件日志；
- Sidecar/filter 两阶段执行、崩溃状态重建；
- 继续、retain、条件恢复和外部变化保护。

### P3-03E：P3-A04/A05

- 固定保存/重启/删缓存 oracle；
- invalid/unknown/并发文件用例；
- 五个真实进程终止点；
- 源素材哈希与无结果快照审计。

## 4. 通过条件

P3-A04/P3-A05 只有同时证明以下事实后通过：

- 保存 filter 跨重启和缓存删除恢复，结果来自当前文件系统；
- YAML 不含 asset key/path/result/thumbnail/index snapshot；
- 单条无效、离线根和未知字段不损坏其他条目；
- 外部文件变化不会被静默覆盖；
- Tag 影响只来自精确 AST 节点，update/retain/cancel 结果准确；
- 任一崩溃点可重建并安全继续/保留/恢复；
- Sidecar、filter 与恢复日志均为普通可检查文件，没有数据库；
- 原始素材 SHA-256 不变。

## 5. 尚未开始

- 未实现 SavedFilterStore、Tauri 命令或桌面管理 UI；
- 未扩展 parser source span 或 transaction plan-only；
- 未创建 `tag-renames-v1` 运行期目录；
- 未执行 P3-A04/A05；
- 未把阶段 3 状态改为 In progress。

以上实现等待阶段 2 退出评审完成后开始。
