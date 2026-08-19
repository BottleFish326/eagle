# 文件事件与一致性扫描协议

本文档定义 P2-01 的平台监听归一化、桌面通道和恢复边界。事件是“磁盘可能变化”的提示，不是权威历史；任何时刻都必须能够通过完整扫描从素材与 Sidecar 恢复。

## 1. 统一事件

```ts
type FsChange = {
  kind: "create" | "modify" | "move" | "delete" | "rescan-required";
  paths: string[];
  reason?:
    | "ambiguous-rename"
    | "batch-overflow"
    | "queue-overflow"
    | "channel-disconnected"
    | "out-of-scope"
    | "unknown-event"
    | "watcher-error";
};
```

- `create`、`modify`、`delete` 必须包含一个路径；
- `move` 必须按顺序包含旧路径和新路径；
- `rescan-required` 只包含规范化素材根，且必须有稳定原因；
- 纯读取或属性访问不产生变更；
- 所有路径必须是素材根内不含父目录穿越的绝对路径。

## 2. 批次

```ts
type FsChangeBatch = {
  root: string;
  changes: FsChange[];
  rawEventCount: number;
};
```

默认静默窗口为 120 毫秒，持续事件的单批最长收集时间为 750 毫秒，原始事件上限为 4,096。操作系统回调到批处理器之间使用同容量同步通道；通道满时丢弃新增提示并返回 `queue-overflow`，由完整扫描恢复真相。输出按移动路径对和单路径排序，便于测试与诊断。空批次不会发送到桌面界面。

批内折叠规则：

| 已有状态     | 后续状态      | 输出     |
| ------------ | ------------- | -------- |
| create       | modify/create | create   |
| create       | delete        | 无       |
| modify       | create        | create   |
| modify       | delete        | delete   |
| delete       | create/modify | modify   |
| 任一相同事件 | 相同事件      | 一条事件 |

临时文件移动到正式路径输出目标 `modify`；正式路径移动到临时路径先视为源 `delete`，若同批内又有替换写回则折叠为 `modify`。

## 3. 桌面通道

前端只通过以下两个命令管理监听：

```text
start_library_watch(rootId, onEvent) -> watchId
stop_library_watch(watchId) -> boolean
```

`start_library_watch` 只接受已配置、启用且当前可访问的根 ID，不接受路径。每个根同时最多一个监听器。事件形状为 `started`、`changes`、`failed`、`stopped`；通道发送失败会协作停止对应线程。

扫描生产者到桌面消费端另有每个扫描 8 批次的同步通道。前端处理 `batch` 后调用 `acknowledge_library_scan_batch(scanId, sequence)`；未确认窗口最多 8 项，30 秒没有释放容量则扫描失败且不发布权威完成结果。消费变慢会反压扫描生产者，不保留无界批次。

## 4. 一致性扫描

任一非空变更批次都会去抖触发该根的正式扫描。`rescan-required` 不携带或扩大任意路径权限。扫描开始时删除的只有该根在运行期内存索引中的派生记录；扫描随后重新读取磁盘文件和相邻 Sidecar。

根目录列表中的“完整一致性扫描”使用同一 `start_library_scan(rootId, onEvent)` 命令。删除、停用或不可访问的根不会继续监听。

## 5. 安全与故障规则

- 监听错误不得造成应用退出或修改素材；
- 批次溢出、事件缺失与无法配对的重命名必须保守重扫；
- 事件路径越界时只重扫已授权根，不读取事件声称的越界位置；
- 监听和扫描不保存素材快照数据库；
- P2-01 不自动移动、删除、重新关联或覆盖任何素材与 Sidecar。
