# 缩略图协议

本文档定义 P1-06、P2-05 与 P3-01 的桌面端缩略图请求、提供器、缓存、生命周期与错误协议。缩略图是可删除派生数据，任何调用都不得修改原始素材或相邻 Sidecar。

## 1. 调用顺序

1. 正式扫描器把 `AssetRecord` 放入进程内目录；
2. 视图确定当前可见素材，并对每个可见键调用 `request_thumbnail`；
3. 若结果为 `ready`，视图使用返回的 `cacheKey` 调用 `read_thumbnail`；
4. `read_thumbnail` 返回 PNG `ArrayBuffer`，视图负责创建并在卡片离开视口时释放对象 URL；
5. 若结果为 `placeholder`，视图显示通用卡片、原因和可读消息，不调用二进制读取。

扫描、Tag 查询和元数据编辑均不得隐式触发上述流程。后端最多并发执行 4 个解码任务，调用方无需自行绕过限流。

## 2. 生成请求

Tauri 命令：`request_thumbnail`

```json
{
  "input": {
    "assetKey": "/normalized/library/logo.png",
    "maxEdge": 256
  }
}
```

约束：

- `assetKey` 必须已存在于本次运行的内存目录中，前端不能直接要求解码任意路径；
- `maxEdge` 为 16–2,048 的整数；
- 素材必须是扫描器已注册的类型；当前内置 raster provider 解码 PNG、JPEG、WebP 和 GIF，其他格式可以稳定降级为类型卡片；
- GIF 缩略图固定使用第一帧；输出 MIME 固定为 `image/png`。

成功或缓存命中：

```json
{
  "status": "ready",
  "thumbnail": {
    "assetKey": "/normalized/library/logo.png",
    "cacheKey": "64-lowercase-hex-characters",
    "mime": "image/png",
    "width": 256,
    "height": 128,
    "sourceSize": 18230,
    "sourceModifiedUnixMs": 1786710000000,
    "cacheHit": false,
    "providerId": "builtin-raster",
    "providerVersion": "image-0.25.9-triangle-png-v1",
    "decoderVersion": "image-0.25.9-triangle-png-v1"
  }
}
```

素材级降级：

```json
{
  "status": "placeholder",
  "assetKey": "/normalized/library/damaged.png",
  "reason": "invalid-content",
  "message": "decoder error details"
}
```

占位原因是稳定枚举：

| 原因 | 含义 |
|---|---|
| `missing-asset` | 请求前或排队后素材已消失 |
| `codec-unavailable` | 格式已注册，但当前构建未安装可选 codec |
| `preview-unavailable` | 格式已注册，但当前构建没有对应 preview provider |
| `unsupported-format` | 素材格式未注册预览能力 |
| `unreadable` | 文件存在但无法打开或读取 |
| `invalid-content` | 已注册格式的内容损坏或与声明不符 |
| `decode-failed` | provider 已接受内容但解码失败 |
| `resource-limited` | 输入或解码超过明确资源边界 |
| `timed-out` | 隔离 provider 超过硬超时 |
| `source-changed` | 解码期间源文件版本改变，本次结果未缓存 |

`codec-unavailable`、`preview-unavailable` 和 `unsupported-format` 是中性能力降级；其余原因是需要用户注意的文件或执行故障。`decoderVersion` 暂作为旧前端兼容字段，值与 `providerVersion` 相同；新逻辑使用 provider ID/version。

## 3. 二进制读取

Tauri 命令：`read_thumbnail`

```json
{ "cacheKey": "64-lowercase-hex-characters" }
```

命令只接受 64 位小写十六进制键，并在固定缓存目录内解析路径，返回原始 PNG `ArrayBuffer`。不存在或不合法的键返回结构化命令错误；前端不得持久化缓存绝对路径。

## 4. 缓存键与布局

缓存键的输入为：

```text
normalized asset runtime key
stable asset ID or none
source size
source mtime at the filesystem's available precision
requested max edge
provider ID
provider version
```

布局为：

```text
<OS application cache>/
  thumbnails-v1/
    .material-eagle-thumbnail-cache
    ab/
      ab...64hex.png
      ab...64hex.json
```

JSON 描述文件只包含 Schema、缓存键、不可逆源令牌、provider ID/version 和请求长边：

```json
{
  "schema": 2,
  "cacheKey": "64-lowercase-hex-characters",
  "sourceToken": "64-lowercase-hex-characters",
  "providerId": "builtin-raster",
  "providerVersion": "image-0.25.9-triangle-png-v1",
  "maxEdge": 256
}
```

`sourceToken` 是路径键、稳定 ID、大小和完整 mtime 的不可逆 SHA-256，不保存原路径。键命中前同时验证描述与缓存图片，并更新 PNG mtime 作为最后使用时间。源文件大小或 mtime 改变、请求尺寸改变、provider 切换或 provider 升级都会得到新键，不复用旧 PNG；IPC 中的 `sourceModifiedUnixMs` 只用于界面显示，缓存键使用文件系统可提供的完整时间精度。

PNG 与 JSON 均使用同分片临时文件、文件同步和原子持久化。只有文件对完整、描述匹配且 PNG 可解码时才命中；进程中断留下的临时文件或半项会在维护时回收。

## 5. 生命周期维护

默认策略：

| 边界 | 默认值 |
|---|---:|
| 条目数 | 20,000 |
| 总空间 | 1 GiB，包含 PNG 与 JSON，不含保护标记 |
| 最后使用后保留 | 30 天 |

容量估计超过边界时立即维护；其他写入按最多 64 次的间隔维护。应用启动时维护损坏、旧解码器、过期和超容量项；所有启用且可用的素材根都至少完成一次完整扫描、且当前并行扫描全部结束后，才使用内存目录源令牌集合继续回收孤立项。扫描失败或取消不建立该根的目录权威。扫描期间不执行目录快照孤立回收，避免把未完成扫描当作真相。

Tauri 命令：`maintain_thumbnail_cache`，无参数；活动扫描期间返回 `recovery-busy`。

```json
{
  "removedEntries": 3,
  "removedFiles": 6,
  "removedBytes": 8192,
  "incompatibleEntries": 1,
  "orphanEntries": 1,
  "expiredEntries": 1,
  "capacityEntries": 0,
  "stats": {
    "layoutVersion": 3,
    "fileCount": 4,
    "entryCount": 2,
    "byteCount": 4096,
    "maxEntries": 20000,
    "maxBytes": 1073741824,
    "retentionDays": 30,
    "decoderVersion": "image-0.25.9-triangle-png-v1"
  }
}
```

维护按以下优先级归类并删除：不完整/无法识别项、未知或旧 provider 项、孤立项、过期项、LRU 容量项。报告是派生计数，不成为第二份目录或数据库。

## 6. 全量清理与中断恢复

Tauri 命令：`clear_thumbnail_cache`，无参数。

```json
{
  "removedFiles": 42,
  "removedBytes": 1048576
}
```

清理与读取/写入互斥，只处理应用缓存目录中的固定 `thumbnails-v1`。返回计数包含 PNG 和 JSON，不包含保护标记。素材、Sidecar、根目录配置和内存目录不在删除边界内；下一次可见请求会重新生成 PNG。

清理先写入 tombstone 所有权标记，再把活动根原子改名为同级 `.material-eagle-thumbnail-cache-gc-<UUIDv7>`，建立新的带标记空根，最后回收旧根。启动只删除名称 UUID 合法且所有权标记匹配的 tombstone；相似名称的未标记目录不删除。进程在根改名后或新根建立后中断，重启都能继续运行并按需重建。

## 7. 命令级错误

命令调用失败时返回以下可区分结构；素材自身的正常降级仍使用 `placeholder`，不是命令错误。

```text
asset-not-found  assetKey 未出现在内存目录
invalid-request  尺寸或缓存键不合法
cache            缓存边界、读取或写入失败
internal         任务或共享状态失败
recovery-busy    活动素材扫描尚未形成完整目录快照
recovery-incomplete 仍有可用素材根尚未完成一次完整扫描
```

前端应保留 `kind`，显示 `message`；不得把命令错误静默转换为成功占位。
