# 保存过滤器与 Tag 重命名恢复协议

本文实现化 ADR-027，定义 `saved-filters.yml` 的加载隔离、版本化写入、查询执行、Tag AST 重写和跨文件崩溃恢复。实施等待阶段 2 退出。

## 1. 文件位置与资源边界

文件固定为操作系统应用配置目录中的 `saved-filters.yml`。命令不接收目录或文件路径，只接收稳定 filter ID、结构化更新和调用方观察到的文件版本。

```text
SavedFilterFileVersion {
  exists,
  size,
  modifiedUnixMs,
  sha256
}
```

- 文件最大 1 MiB、最多 512 项、查询最长 4,096 字符；
- YAML 只接受普通 mapping/sequence/scalar，拒绝自定义 tag、重复 mapping key、非字符串 key、循环/过量 alias 和过深嵌套；
- 解析或顶层结构失败时保留原字节、返回 `invalid-file`，不以空文件覆盖；
- 文件缺失等价于 `{schema: 1, filters: []}`，第一次写入仍使用 expected-absent；
- 不读取素材根、Vault 或缓存中的同名文件。

## 2. 加载与条目隔离

加载结果：

```text
SavedFilterCatalog {
  fileVersion,
  validFilters[],
  unavailableFilters[],
  invalidEntries[],
  fileIssues[]
}
```

条目按文件顺序检查：

1. Schema 字段、UUIDv7 ID、单行 query、scope、sort、时间；
2. ID 唯一；重复 ID 的所有条目均隔离，不能选择其中一个；
3. display name 去除首尾 Unicode 空白后做 Unicode case fold，冲突名称的所有条目均隔离；
4. 使用当前正式 parser 完整解析 query；
5. selected root ID 使用规范小写 UUID 且唯一；
6. 根离线/停用/已移除时条目进入 `unavailableFilters`，原 scope 保留，不算 YAML 损坏；
7. 未知 sort 或未来必需能力进入可修复 invalid，不退回隐式默认。

管理器保留整个 YAML `Value` 树。合法条目映射回数组位置；无效条目、未知顶层/条目字段在编辑其他项后原样逻辑保留。注释和原排版不承诺字节保留，但字段、标量值和数组顺序不得丢失。

## 3. 写入与并发

创建、更新、重命名、删除和排序都执行：

1. 读取当前文件并计算完整版本；
2. 与调用方 expected version 比较；
3. 在保留未知值的树上按稳定 ID 应用单一 mutation；
4. 对完整候选重新执行结构/语义验证；
5. 确定性序列化到同目录临时文件；
6. 同步文件、复核目标版本、原子替换并同步父目录；
7. 返回新版本和变更 filter。

版本不同时返回 `external-change` 以及重新加载入口，不自动合并或 last-write-wins。删除只移除该 filter 条目；空 catalog 仍保留合法 `schema: 1` 文件。删除文件不是正常 mutation。

确定性输出使用 UTF-8、LF、两空格缩进、顶层 `schema` 后 `filters`，已知字段按 Schema 顺序，未知字段保持相对顺序。时间输出 UTC RFC 3339 毫秒；创建时 `createdAt == updatedAt`，更新只改变 `updatedAt`。

## 4. 执行与恢复

激活过滤器时：

- 重新获取当前 filter 和文件版本；
- 解析 query，不使用保存的 AST/结果；
- scope=`all-enabled-roots` 取当前全部启用根；
- scope=`selected-roots` 只取保存 UUID 当前可用的交集，并显示缺失根；
- 将 expression/scope/sort 交给 P3-07 后端 view；
- 删除缩略图、查询索引或全部派生缓存后，重扫并重复以上步骤得到当前文件系统结果。

保存项从不包含 asset key、路径、结果数量或 scroll/selection。界面可以显示本次运行计数，但不写回文件。

## 5. Tag AST 影响分析

parser 为每个精确 Tag 节点返回 UTF-8 byte span 和节点类型。影响分析只匹配语义值完全等于 old Tag 的：

- 普通 AND Tag；
- `tag:` 显式 Tag；
- `-tag`/`-tag:` 排除 Tag；
- `any:(...)` 中的精确成员。

命名空间 wildcard 只有其完整 Tag 节点与显式 namespace rename 操作匹配时才改写；普通精确 Tag rename 不猜测前缀。字段字符串、路径、颜色空间、备注和自由文本不参与。

重写按 span 从后向前替换，只修改匹配节点并使用规范转义，尽量保留用户空白和未受影响 token。重写后必须重新解析并证明 AST 仅发生选定 Tag 替换；否则该 filter 标为 `rewrite-failed`，绝不写入。

预检逐项返回 filter ID、显示名、原 query、新 query、节点数和可选诊断。用户对每项选择 `update` 或 `retain`，也可以取消整个动作；解析失败项只能 retain。

## 6. Tag 重命名协调日志

Sidecar 批次和 `saved-filters.yml` 不能跨文件系统原子提交。为可恢复地协调，应用配置目录增加固定 `tag-renames-v1`，每个操作使用 UUIDv7 YAML 日志；该日志含 Tag/查询，属于私密恢复数据，不进入支持导出。

执行顺序：

1. 固定 old/new Tag、root scope、catalog revision、受影响素材版本和 filter 选择；
2. 以 plan-only 模式创建 ADR-020 metadata transaction，尚不应用；
3. 生成 filters 原始/计划完整字节及两者 SHA-256；
4. 原子写入 rename coordinator，引用 transaction ID 和两份文件版本；
5. 应用 metadata transaction，逐项检查取消/冲突；
6. 若选择更新 filter，以原版本为前置条件原子写 `saved-filters.yml`；
7. 重新扫描受影响根、重载 filters，审计结果后标记 coordinator completed。

恢复状态：

```text
planned
sidecars-active
filters-pending
completed
conflict
restored
```

- 进程终止后通过 metadata transaction 当前摘要和 filter 原/计划摘要重建；
- Sidecar 已完成但 filter 未写时，用户可在版本仍匹配时继续 filter 写入、retain 原 query，或条件恢复 Sidecar；
- filter 外部变化时不覆盖，状态为 conflict；
- filter 已计划写入时，恢复只在当前摘要仍等于计划摘要时写回原字节；
- coordinator 完成/恢复后保留到用户显式清理；清理日志不改变 Sidecar 或 filter。

## 7. 稳定错误

```text
invalid-file
file-too-large
unsupported-schema
invalid-entry
duplicate-id
duplicate-name
invalid-query
unknown-sort
root-unavailable
external-change
rewrite-failed
transaction-conflict
authorization-lost
recovery-conflict
```

持久诊断只保存计数和 error kind，不保存名称、query、Tag、根路径或完整恢复日志内容。

## 8. P3-A04 / P3-A05 验收

### P3-A04

1. 保存 all-enabled 与 selected-roots 两类 filter；
2. 重启应用并删除所有派生缓存/索引；
3. 从文件系统重扫后执行，结果与独立 query oracle 一致；
4. 证明文件没有 asset ID/path/result snapshot；
5. 注入 invalid query、duplicate ID/name、unknown sort、未知字段和离线根，合法项仍可使用且原值不丢失；
6. 外部编辑后写入被版本检查阻止。

### P3-A05

1. 构造 AND、排除、OR、显式 Tag、同名前缀、字段值和无效 query；
2. 预检只列出精确 Tag 节点，并验证 update/retain/cancel 三种选择；
3. 目标 Tag 已存在时 Sidecar 集合正确合并；
4. 在 coordinator 写后、Sidecar 中途、filter 替换前后终止真实进程；
5. 重启后准确继续、retain 或条件恢复，不覆盖外部变化；
6. 原始素材 SHA-256 全部不变，未选 filter/query 字节语义不变；
7. 删除缓存、配置恢复日志或应用本身不影响当前 Sidecar/filter 文件作为事实源。
