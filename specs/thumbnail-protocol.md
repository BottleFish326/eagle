# 缩略图协议

本文档定义 P1-06 的桌面端缩略图请求、缓存与错误协议。缩略图是可删除派生数据，任何调用都不得修改原始素材或相邻 Sidecar。

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
- 素材必须是图片类型；格式以文件内容识别，仅支持 PNG、JPEG、WebP 和 GIF；
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
    "decoderVersion": "image-0.25.9-triangle-png-v1"
  }
}
```

素材级降级：

```json
{
  "status": "placeholder",
  "assetKey": "/normalized/library/damaged.png",
  "reason": "decode-failed",
  "message": "decoder error details"
}
```

占位原因是稳定枚举：

| 原因 | 含义 |
|---|---|
| `missing-asset` | 请求前或排队后素材已消失 |
| `unsupported-format` | 不是受支持的图片类型或内容格式 |
| `unreadable` | 文件存在但无法打开或读取 |
| `decode-failed` | 内容损坏、超过资源限制或无法解码 |
| `source-changed` | 解码期间源文件版本改变，本次结果未缓存 |

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
decoder version
```

布局为：

```text
<OS application cache>/
  thumbnails-v1/
    .material-eagle-thumbnail-cache
    ab/
      ab...64hex.png
```

键命中前会验证缓存图片。源文件大小或 mtime 改变、请求尺寸改变或解码器升级都会得到新键，不复用旧 PNG；IPC 中的 `sourceModifiedUnixMs` 只用于界面显示，缓存键使用文件系统可提供的完整时间精度。

## 5. 清理

Tauri 命令：`clear_thumbnail_cache`，无参数。

```json
{
  "removedFiles": 42,
  "removedBytes": 1048576
}
```

清理与读取/写入互斥，只处理应用缓存目录中的固定 `thumbnails-v1`。返回计数不包含保护标记。素材、Sidecar、根目录配置和内存目录不在删除边界内；下一次可见请求会重新生成 PNG。

## 6. 命令级错误

命令调用失败时返回以下可区分结构；素材自身的正常降级仍使用 `placeholder`，不是命令错误。

```text
asset-not-found  assetKey 未出现在内存目录
invalid-request  尺寸或缓存键不合法
cache            缓存边界、读取或写入失败
internal         任务或共享状态失败
```

前端应保留 `kind`，显示 `message`；不得把命令错误静默转换为成功占位。
