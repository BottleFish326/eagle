# 重复素材分析协议

本文定义 P3-05 的输入快照、精确内容管线、物理别名、视觉候选、资源边界和验收报告。实施等待阶段 2 退出。

## 1. 任务输入与生命周期

输入只能是：

- ADR-029 的 `SelectionSnapshot`；或
- 用户明确选择的已启用根 ID 集合在当前 catalog revision 上生成的精确快照。

任务状态：

```text
preparing -> sampling -> hashing -> grouping -> completed
                              \-> stopped
```

创建任务返回不透明 UUIDv7。读取结果、取消和释放只接收任务 ID；客户端不能提交路径。任务超时、应用退出或显式释放后，所有运行期摘要和组被销毁。

## 2. 精确内容管线

### 2.1 大小预分组

只对当前可读且大小已知的普通素材文件分组。大小只出现一次的文件无需读取；大小未知、根离线、符号链接、目录和特殊文件分别产生稳定跳过原因。

### 2.2 快速筛选

同大小候选读取首尾最多 64 KiB，并调用现有 `sha256-sample-64k-v1`。快速值只用于减少完整读取量；单一快速值被淘汰，两个以上相同才进入完整哈希。

### 2.3 完整确认

候选以固定块流式计算 SHA-256。读取前保存：

```text
FileObservation {
  canonicalPath,
  size,
  modifiedUnixMs,
  platformFileId?,
  quickFingerprint
}
```

读取后重新 stat 并重新计算快速指纹；大小、mtime、物理 ID 或快速值变化即丢弃本次摘要。实现不得为满足进度期限截断文件或使用 Sidecar 历史摘要补全。

最终键为 `(size, sha256)`。组成员至少为 2 才输出，稳定摘要采用小写 64 位十六进制。P3 按计划以 SHA-256 作为最终确认，不用路径或解码结果增强/削弱结论。

## 3. 组和成员

```text
DuplicateGroup {
  kind: exact-content | same-file-alias | visual-candidate,
  groupId,
  size?,
  sha256?,
  algorithm,
  members[],
  independentFileCount,
  reclaimableBytes
}

DuplicateMember {
  assetKey,
  stableId?,
  rootId,
  relativePath,
  sidecarDigest?,
  physicalIdentity: independent | same-file | unknown
}
```

`groupId` 是任务内由 kind/algorithm/摘要派生的不透明值，不是持久素材 ID。`reclaimableBytes = size × max(independentFileCount - 1, 0)`，只用于估计；P3 不据此执行删除。

同一物理文件的路径首先形成 `same-file-alias` 子组。若同摘要还存在其他独立文件，界面同时显示物理别名关系和完整内容组，不把别名重复计入独立副本或可回收空间。

## 4. 视觉相似

视觉管线必须显式启用且与精确内容分开。初始范围只包括能在资源限制内解码的静态图像/视频代表帧：

- 输入规范化到固定颜色空间、尺寸和方向；
- 算法 ID、版本和阈值随结果返回；
- 动画只取声明的代表帧并明确标识；
- Alpha 合成背景固定，不能随系统主题变化；
- codec 缺失、损坏或资源超限为无分数，不视为不相似；
- 相似距离仅用于排序候选，用户界面不得显示“确认重复”。

具体感知算法和阈值在该切片实施前以基准夹具确定。中间切片尚未完成时必须显示明确不可用状态，不能把视觉候选混入精确组；没有误报/漏报报告和算法版本证据时，P3-05 不得标记完成。

## 5. 资源与取消

- Sampling/Hash 使用共享 Hash 类许可；窗口失焦时沿用后台容量；
- 每个文件只保留定长缓冲、观察和摘要，不保留字节内容；
- 文件间及读取块间检查取消，取消后不开始新文件；
- 结果组、成员和错误列表按输入数线性有界；
- 进度公开 sizeGroups、sampled、hashQueued、hashed、bytesRead、groups、skipped，不发送路径；
- 任务取消返回已完成统计，但不把部分组标为最终确认；用户选择“查看已完成”时必须明确 `partial`。

## 6. 稳定跳过/失败原因

```text
asset-missing
not-regular-file
symlink-not-followed
size-unknown
root-offline
authorization-lost
unreadable
source-changed
resource-limited
cancelled
preview-unavailable
```

物理身份能力不可用作为任务级 capability 诊断返回，不跳过该文件的完整内容分析。单文件失败隔离，任务继续处理其他成员；根权限失效只停止该根的未开始项目。

## 7. P3-A07 / P3-A08 验收夹具

固定集合至少包含：

1. 两个不同路径、字节完全相同且 Sidecar ID/Tag 不同的文件；
2. 三个同名但内容不同的文件；
3. 同大小、首尾采样相同、中间字节不同的快速指纹碰撞构造；
4. 一个硬链接路径对和一个独立副本；
5. 快照后内容变化、mtime 变化和扫描中删除；
6. 中英文/Emoji 路径及不同根同相对路径；
7. 损坏、超大、只读和 codec 不可用素材；
8. 视觉相似但字节不同、视觉不同但文件名相同的图像。

验收必须证明：

- 精确组与独立完整字节比较得到的 oracle 完全一致；
- 同名不同内容未进入 `exact-content`；
- 采样碰撞在完整 SHA-256 阶段分离；
- 硬链接不重复计算可回收空间；
- 变化文件不产生过期摘要；
- 取消、不可读和单文件失败不会污染其他组；
- 分析前后全部素材和 Sidecar SHA-256 不变；
- 没有删除、移动、Sidecar 合并、Markdown 改写或结果落盘。

报告记录 fixture manifest 摘要、平台/文件系统、提交、样本/完整读取字节、组数、跳过数、p50/p95、RSS/队列峰值和素材保护证明。
