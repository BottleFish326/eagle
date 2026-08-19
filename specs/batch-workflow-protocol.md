# 批量选择与工作流协议

本文定义 P3-04 的选择快照、预检、执行、取消和结果协议。阶段 2 退出后实施；元数据写入继续遵循 ADR-020/ADR-021，不引入第二份素材真相源。

## 1. 选择模型

桌面端可维护轻量视觉选择，但任何批量命令必须先向后端创建精确快照：

```text
SelectionSnapshot {
  id: UUIDv7,
  catalogRevision: u64,
  orderedItems: Vec<{ key: AssetKey, stableId?: UUIDv7 }>,
  createdAt,
  expiresAt
}
```

- `orderedItems` 按当前视图排序固定，按 key 去重并保持顺序；稳定 ID 只作为移动重解析提示；
- 空选择不创建快照；
- 快照只存在于有界运行期 session，应用退出、显式释放或过期即消失；
- snapshot ID 不携带路径且不可由客户端选择；
- 后端创建时比较客户端看到的 revision；不一致则返回 `catalog-changed` 和新 revision，由 UI 刷新后重试；
- L 数据集全选是强制用例，session 预算和过期回收必须有资源测试。

`Shift` 范围包含 anchor 与 target。普通点击更新 anchor；命令式“全选当前结果”不改变当前焦点。过滤或排序改变后，选中项可以保持，但范围动作必须使用新 revision 的可见顺序并明确处理消失 anchor。

## 2. 预检

批量动作先生成只读预检，不产生 Sidecar 或剪贴板变化：

```text
BatchPreflight {
  operationId,
  snapshotId,
  catalogRevision,
  requestedCount,
  executableCount,
  requiresStableIdCount,
  unavailableCount,
  conflictCount,
  estimatedOutputBytes?,
  failures[]
}
```

预检逐项确认：素材仍在目录、根已启用且授权、真实路径未逃逸、素材/Sidecar 版本可读取、操作参数合法。目录 revision 变化本身不自动失败；每个原快照键必须重新解析，移动后的稳定 ID 只有唯一匹配时才可更新目标，否则归入失败清单。

用户确认绑定 `operationId` 和预检摘要。确认前若相关版本再次变化，开始执行时返回 `preflight-stale`，不能静默扩大或缩小批次。

## 3. 元数据写入

支持：

- Tag 集合添加/删除；同一 Tag 不得同时添加和删除；
- 评分设置为 `0..5`；
- 收藏设置为明确 true/false；
- 为缺失 ID 的素材创建 UUIDv7。

元数据动作只生成一份确定性 patch。两个及以上目标进入 `metadata-transactions-v1`；单项仍使用版本化原子编辑。开始写入后，进度事件包含 `operationId`、总数、applied/failed/conflict/planned 和当前序号，不发送 Tag、备注或绝对路径。

取消行为：

1. 设置 operation cancel token；
2. 当前原子 Sidecar 替换完成或失败；
3. 后续计划项不开始；
4. 立即持久化事务日志；
5. 返回状态 `stopped` 和准确计数；
6. UI 提供“继续”“条件恢复”“查看失败”入口。

取消不是回滚。只有 ADR-020 的条件恢复可以还原当前摘要仍等于计划摘要的已应用项。

## 4. 只读批量输出

输出种类：

| 类型 | 每项结果 |
|---|---|
| `path` | 当前规范化绝对路径文本，仅用于用户主动复制 |
| `markdown` | 当前 Vault 内标准 WikiLink；Vault 外为稳定 ID 引用 |
| `html-image` | 转义后的 `<img>` 片段；不内嵌字节或令牌 |
| `stable-reference` | `material://<uuid>`，缺失 ID 需独立确认写入 |

多项输出保持快照顺序，默认以平台换行符连接。每项先返回结构化预览状态；存在失败时 UI 展示失败清单并让用户选择“只复制成功项”或取消，不能默认丢弃失败项。选择复制成功项后重新绑定精确成功键集合，不重新运行原查询。

完整输出必须在资源预算内构造，且只调用一次系统剪贴板写入。剪贴板 API 失败时返回 `clipboard-unavailable`，不创建回退文件、不写入素材根，也不把内容放进浏览器 LocalStorage。

## 5. 稳定失败类型

```text
snapshot-not-found
snapshot-expired
catalog-changed
asset-missing
asset-moved-ambiguous
root-disabled
root-offline
authorization-lost
source-changed
sidecar-conflict
invalid-operation
stable-id-required
output-too-large
clipboard-unavailable
cancelled
```

单项错误不阻断预检其他项目；安全边界或整个计划无法持久化时不得开始任何写入。

## 6. P3-A06 验收

固定 1,000 个素材，操作前记录素材 SHA-256 和 Sidecar 摘要：

1. 以复合查询和排序建立全选快照，另在范围边界选择含首尾的 100 项；
2. 快照后创建新匹配素材、删除一项、移动一项并外部修改一项 Sidecar；
3. 预检证明新素材不在批次，删除/歧义/冲突准确分类；
4. 批量添加 Tag，在固定提交数后发出取消；
5. 核对 applied 项完整、planned 项未变、失败项保持外部字节；
6. 重启后发现 active 事务，继续到完成并再执行一次条件恢复；
7. 批量复制引用验证顺序、失败提示和剪贴板单次提交；
8. 核对所有原始素材 SHA-256 不变，且没有数据库、专有素材副本或选择快照落盘。

报告保存取消请求序号、实际停止边界、各状态计数、重启重建结果、操作耗时、RSS/队列峰值和源素材摘要比较。
