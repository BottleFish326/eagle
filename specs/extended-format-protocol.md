# 扩展格式识别、属性与预览协议

本文定义 P3-01 的支持级别、格式矩阵、资源边界、降级语义和实施切片。协议在阶段 2 退出后实施；当前文件用于锁定接口与验收范围。

## 1. 三层能力

每个格式分别报告以下能力，三者不能相互替代：

1. `recognized`：内容签名或受控扩展名规则确认这是已注册素材；
2. `metadata`：能够在预算内提取只读属性；
3. `preview`：能够生成静态缩略图或启动受控动态预览。

`recognized` 成功后必须建立 `AssetRecord`、读取普通文件字段并合并相邻 Sidecar。`metadata` 或 `preview` 不可用只降低卡片丰富度，不从目录、Tag 或查询中删除素材。

建议的稳定能力结果：

```text
available
codec-unavailable
unsupported-feature
invalid-content
resource-limited
timed-out
source-changed
unreadable
```

## 2. 格式矩阵

| 格式 | 首选识别 | 只读属性 | 静态预览 | 动态预览/降级 |
|---|---|---|---|---|
| PNG/JPEG/GIF/WebP | 现有 magic + MIME | 宽高；JPEG/PNG/WebP 可选 EXIF | `image` 首帧/静态图 → PNG | GIF 动画不自动播放，保留现有首帧 |
| SVG | 有界 UTF-8/XML 根元素 + `.svg` 候选 | `width`、`height`、`viewBox`、是否含外部引用 | `usvg`/`resvg` 静态子集 → PNG | 不执行脚本、事件、动画；外部引用被拒绝后显示诊断 |
| AVIF | ISO BMFF `ftyp` 的 AVIF brand | 主图宽高、方向、颜色/Alpha（可得时） | 已打包 HEIF/AVIF worker → PNG | worker/codec 缺失时通用图片卡片 |
| HEIC/HEIF | ISO BMFF `ftyp` 的 HEIF/HEIC brand | 主图宽高、方向、颜色/Alpha、图像数量（可得时） | 已打包 libheif worker → PNG | worker/codec 缺失时通用图片卡片 |
| MP4/MOV | ISO BMFF brand + track probe | 时长、视频宽高、音视频轨道、编码名称 | 受控媒体 worker 提取首个有效视频帧 | WebView/系统 codec 可用时显式播放；否则视频卡片 |
| WebM | EBML + WebM DocType | 时长、视频宽高、音视频轨道、编码名称 | 受控媒体 worker 提取首个有效视频帧 | 同上，不因预览缺失隐藏文件 |
| MP3 | MPEG audio frame/ID3 | 时长、采样率、声道、编码、内嵌封面（有界） | 有界封面 → PNG；无封面使用音频卡片 | 后续显式播放，不自动播放 |
| WAV | RIFF/WAVE | 时长、采样率、声道、位深、编码 | 音频类型卡片；可选派生波形后置 | 后续显式播放 |
| FLAC | `fLaC` | 时长、采样率、声道、位深、内嵌封面（有界） | 有界封面 → PNG；无封面使用音频卡片 | 后续显式播放 |
| PDF | `%PDF-` + 有界结构探测 | 页数、首页面尺寸；不执行动作/脚本 | 已打包 Pdfium worker 渲染第一页 → PNG | Pdfium 不可用或文档受限时 PDF 卡片 |

## 3. 注册表与数据模型

注册表是编译期静态表，不扫描插件目录，不从数据库加载。概念接口：

```text
FormatDescriptor {
  id, extensions, mime, kind,
  recognizer,
  metadata_provider,
  thumbnail_provider,
  dynamic_preview
}
```

`AssetRecord` 后续增加可选 `media`：

```text
MediaProperties {
  durationMs?, pageCount?, frameCount?,
  sampleRateHz?, channelCount?, bitDepth?,
  colorSpace?, codec?, hasAlpha?
}
```

宽高继续写入既有 `dimensions`，避免同一事实出现两个冲突字段。所有字段来自当前文件，可在重扫时丢弃和重建；Sidecar Schema 不增加这些字段。

## 4. 识别规则

- 首次只读取最多 64 KiB 签名窗口；需要尾部/索引的格式通过可取消的 seek 探测，不把整个文件读入内存；
- 已知内容签名优先，扩展名只选择候选识别器；扩展名与内容冲突产生 `mime-mismatch` 诊断；
- ISO BMFF 必须读取 compatible brands 和轨道，而不是把所有 `ftyp` 都当成 AVIF/HEIC/MP4；
- SVG 只接受 UTF-8，并在解析 XML 前执行源大小上限；
- PDF、媒体容器和音频标签中的路径、URL、脚本、附件及动作都不是素材发现入口；
- 未注册文件继续忽略，但 Sidecar/配置文件仍由现有排除规则处理。

## 5. 资源与隔离边界

| 操作 | 默认边界 | 超限行为 |
|---|---:|---|
| 签名窗口 | 64 KiB | 使用已知扩展候选或保持未识别 |
| 单文件扫描富化 | 沿用 5 秒协作期限 | 保留基础记录，添加 `resource-limited` |
| SVG 源 | 16 MiB | 不解析/渲染，保留通用卡片 |
| 内嵌封面 | 16 MiB | 忽略封面，保留音频属性 |
| 解码源尺寸 | 单边 65,535 像素 | 不分配像素缓冲，返回超限 |
| 解码分配 | 256 MiB | 终止当前预览，其他素材继续 |
| 缩略图输出 | 单边 16–2,048 像素 | 沿用现有请求校验 |
| 原生/复杂 worker | 单请求硬超时 10 秒 | 终止 worker，返回 `timed-out` |
| 并发 worker | 受共享 Decode 许可和全局上限约束 | 有界等待或明确拒绝 |

worker 请求只包含请求 ID、provider ID/version、已授权 canonical path、目标尺寸和限制。响应只包含结构化属性、受限 PNG 字节或稳定错误；stderr 进入有界脱敏诊断。worker 崩溃不得带崩桌面进程，下一请求重新启动干净 worker。

## 6. 提供器决策

- SVG：使用纯 Rust `usvg`/`resvg` 静态子集；禁用脚本、事件、动画和网络获取，不允许相对文件引用逃出素材自身；字体策略必须固定并记录在 provider version。
- AVIF/HEIC：识别不依赖 codec。正式缩略图提供器使用应用打包并验证版本的 decoder-only libheif worker；保持 libheif 安全限制开启，不启用实验 API。若目标平台尚未打包则报告 `codec-unavailable`。
- MP3/WAV/FLAC 及 MP4/WebM 音轨/容器属性：采用显式 feature 集的纯 Rust Symphonia，不使用笼统 `all` feature；只注册本协议需要的 demuxer、codec 和 metadata reader。
- 视频帧：不得调用 `PATH` 中任意 FFmpeg。打包媒体 worker、平台 API 和许可证评审完成前只交付容器属性与通用视频卡片。
- PDF：轻量结构/页数可由受限纯 Rust parser 提供；第一页渲染只由固定 Pdfium worker 提供。Pdfium 未打包时页面预览降级，不影响 PDF 素材和 Sidecar。

## 7. 安全夹具

每个格式至少提交以下固定、可再分发夹具及 SHA-256 清单：

- 最小正常文件；
- 截断 header/container；
- 正确扩展但其他格式内容；
- 正确内容但错误扩展；
- 声明超大尺寸/时长/页数但体积极小的解压炸弹式文件；
- 超过属性/封面/XML 上限的文件；
- 解析慢或循环引用样本；
- SVG 外部 URL/文件引用和脚本；PDF 动作、附件或加密；媒体未知 codec/track。

夹具生成器只能清理带自身所有权 marker 的目录。恶意固定二进制放入 `fixtures/formats`，清单记录来源、许可证、期望识别结果、属性结果、预览结果和最大允许时间/内存。

## 8. 实施切片与验收

### P3-01A：注册表与通用卡片

- 扫描所有列出的注册格式并合并 Sidecar；
- UI 展示稳定类型卡片；查询 `type:image|video|audio|pdf` 正确；
- codec 缺失不产生扫描失败。

### P3-01B：SVG

- 有界属性解析、静态 PNG、外部引用/脚本隔离；
- 正常、损坏、超大和恶意 SVG 夹具通过。

### P3-01C：AVIF/HEIC

- ISO BMFF brand 识别与通用卡片先通过；
- 三平台 worker 打包完成后开放属性/缩略图 capability。

### P3-01D：视频

- MP4/MOV/WebM 轨道和时长；未知 codec 降级；
- 视频帧 worker 单独验收，不阻塞基础格式支持。

### P3-01E：音频

- MP3/WAV/FLAC 时长、采样率、声道和有界封面；
- 无封面、损坏标签、未知编码都保留音频卡片。

### P3-01F：PDF

- 页数与首页面尺寸；Pdfium worker/缺失两条路径；
- 动作、脚本、附件和网络引用不执行。

每片通过后更新格式矩阵，但 P3-A01 只有所有正式格式的期望清单一致才通过；P3-A02 还必须在固定超大/损坏/恶意集合上证明单文件隔离、取消和内存边界。

## 9. 依据

- [image-rs 支持格式与 AVIF native decoder 说明](https://docs.rs/image/latest/image/codecs/)
- [resvg 官方项目与静态 SVG/安全边界](https://github.com/linebender/resvg)
- [libheif 官方编解码、插件与 security limits](https://github.com/strukturag/libheif)
- [Symphonia 官方格式、codec 与 MSRV 矩阵](https://github.com/pdeljanov/Symphonia)
- [pdfium-render 官方动态打包说明](https://github.com/ajrcarey/pdfium-render)
- [lopdf 文档](https://docs.rs/lopdf/latest/lopdf/)
