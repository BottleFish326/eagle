# AssetRecord v1 运行时规范

`AssetRecord` 是扫描器交给桌面端和内存索引的统一运行时对象。它不是数据库记录，不直接落盘；删除内存状态后可以由素材文件、相邻 sidecar 和普通根目录配置重建。

## 字段来源

| 字段 | 来源 | 可为空 | 说明 |
|---|---|---|---|
| `key` | 规范化绝对路径 | 否 | 当前运行时键；稳定跨移动身份使用 `id` |
| `id` | Sidecar | 是 | 尚未产生用户元数据时允许为空 |
| `rootId` | 根目录配置 | 是 | 正式库扫描必填；独立 CLI 扫描允许为空 |
| `path` | 文件系统 | 否 | 规范化绝对路径 |
| `relativePath` | 文件系统 | 否 | 相对于授权根的路径 |
| `sidecarPath` | 文件系统 | 是 | 只有检测到相邻 sidecar 时存在 |
| `sidecarState` | Sidecar 读取结果 | 是 | Schema、内容摘要和更新时间，供后续乐观并发写入使用 |
| `fileName`、`extension` | 文件系统路径 | 部分 | `extension` 统一为小写 |
| `mime`、`kind` | 文件头与受限扩展名回退 | 否 | P1 只接收 PNG/JPEG/GIF/WebP |
| `size` | 文件系统 metadata | 是 | 文件在扫描中消失或不可读时为空 |
| `createdUnixMs` | 文件系统 metadata | 是 | 平台或文件系统不提供时为空 |
| `modifiedUnixMs` | 文件系统 metadata | 是 | 无法读取时为空 |
| `fileReadOnly` | 文件系统权限 | 是 | 表示 metadata 中的只读状态，不代替实际写入权限检查 |
| `dimensions` | 图片文件头 | 是 | 损坏或不支持时为空，并附加素材问题 |
| `nativeMetadata` | 选定 EXIF 字段 | 是 | 只读，不包含 GPS |
| `tags`、`rating`、`favorite`、`note`、`aliases` | Sidecar | 否 | Sidecar 缺失时使用显式默认值 |
| `issues` | 扫描器 | 否 | 仅描述本素材问题，不阻断同根其他素材 |

## 合并优先级

```text
文件系统路径与 stat ─┐
图片文件头与 EXIF ───┼─> 文件派生字段（Sidecar 不可覆盖）
根目录配置 ──────────┘

相邻 Sidecar ─────────> 用户元数据字段
解析诊断 ─────────────> issues
```

未知 Sidecar 字段由 Sidecar 读写层保留，但扫描器不会把它们复制到 `AssetRecord`，也不会据此改变文件派生字段。

## 素材级问题

| 类型 | 含义 |
|---|---|
| `invalid-sidecar` | 相邻 YAML 无法解析或不符合约束 |
| `unreadable-file` | 文件在遍历后消失、权限变化或 metadata 读取失败 |
| `invalid-image-metadata` | 无法从受支持图片读取合法尺寸 |
| `invalid-native-metadata` | 原生 EXIF 容器存在但无法解析 |
| `missing-asset` | 已知记录对应的素材缺失，供后续一致性阶段使用 |
| `unsupported-format` | 已知对象的格式不受当前阶段支持 |

## 增量扫描事件

桌面端以一次 `start_library_scan` 创建唯一 UUIDv7 扫描 ID，并通过 Channel 接收：

1. `started`：扫描 ID、根 ID 与规范化根路径；
2. `batch`：有序批次，包含 `sequence`、`assets`、`problems` 和 `visitedFiles`；
3. `finished`：累计数量、耗时，以及 `completed` 或 `cancelled`；
4. `failed`：根目录或扫描配置使本轮无法继续。

`cancel_library_scan` 是协作式取消。调用成功只表示取消令牌已设置，最终状态以同一扫描 ID 的 `finished` 事件为准。
