# P3-01 扩展格式支持实施准备报告

- 状态：Implementation in progress；P3-01A/B/D/E/F Completed locally，P3-01C Completed
- 日期：2026-08-19
- 对应：P3-01、P3-A01、P3-A02
- 决策：[ADR-026](../../specs/adr/026-capability-based-extended-format-pipeline.md)
- 协议：[扩展格式协议](../../specs/extended-format-protocol.md)

## 1. 当前实现审计

| 层 | 当前能力 | P3-01 缺口 |
|---|---|---|
| 素材模型 | 已有 image/video/audio/pdf/other 类型与可重建 `media`；视频、音频和 PDF 专用字段已接线 | 无 |
| 扫描器 | 15 格式静态注册；64 KiB 内容签名优先；SVG、AVIF/HEIC、视频、音频与 PDF 属性均使用有界提供器并只写派生记录 | 统一 P3-A01/A02 执行器尚未完成 |
| 缩略图 | raster、SVG、固定 libheif 及 MP3/FLAC 有界封面 provider 已接入；provider 缺失/无封面稳定降级 | 视频帧与 PDFium worker 已按独立发行门禁暂缓 |
| 缓存 | provider ID/version 已进入每项 key 与 Schema 2 descriptor；layout 3 自动失效旧项 | 后续每个新增 provider 必须登记当前版本并增加失效测试 |
| 查询/UI | `scan_root → AssetCatalog → type:` 四类测试已贯通；codec/provider 缺失显示中性类型卡片，音频封面可进入既有缩略图 UI，文件故障显示错误卡片 | 完整清单的 UI/preview 统一重放尚未完成 |
| 夹具 | SVG、AVIF/HEIC、9 个视频、10 个音频和 10 个 PDF 正常/对抗夹具均具真实 SHA-256 与三平台期望 | 仍需补齐 SVG 超大正式资源证据并统一执行全部恶意集合 |

核心结论：P3-01 第一片必须先取消“只有可解码图片才是素材”的假设，而不是先给 UI 增加扩展名图标。

## 2. 已固定准备成果

- 格式识别、属性和预览三层能力模型；
- 静态编译期注册表与内容优先识别；
- codec 缺失仍建立素材记录、合并 Sidecar 和参与查询的降级合同；
- 纯 Rust 解析器与 native/复杂 worker 的隔离边界；
- SVG、AVIF/HEIC、视频、音频、PDF 的格式矩阵；
- 64 KiB 签名、5 秒富化、16 MiB SVG/封面、65,535 像素、256 MiB 解码和 10 秒 worker 等资源门限；
- 六个可独立提交、独立回滚的实施切片及 P3-A01/P3-A02 证据要求。
- 严格的格式夹具清单 JSON Schema，覆盖来源许可、SHA-256、平台/provider profile、识别/属性/预览期望和逐夹具资源预算。

## 3. 门禁状态

本报告不表示 P3-01 已完成。阶段 2 最终退出收据已接受，P3-01A/P3-01B 本地门禁和
P3-01C 托管门禁和 P3-01D/E/F 本地门禁已经关闭；可选视频帧/PDFium worker 已按独立
发行边界暂缓；全格式期望矩阵执行器和完整恶意夹具资源证据尚未完成。

阶段门禁已结束，但设计文档、交叉编译或 codec 库能力说明仍不得计作 P3-A01/P3-A02 通过；必须由后续代码、夹具和机器证据验收。

## 4. 阶段切换后的首个测试切片

P3-01A 必须以测试和清单验证器开场，不能先扩大扩展名白名单。当前代码的精确落点与完成顺序如下：

1. 在 `crates/filesystem` 增加静态格式注册表及纯识别测试。注册表先证明 descriptor ID、扩展名、MIME 与 `AssetKind` 唯一且一致，再覆盖签名优先、扩展候选和内容冲突；`asset-core` 继续只持有稳定素材值模型，不承担文件探测。
2. 为 `fixtures/formats/manifest.json` 增加独立语义验证器。除 JSON Schema 外，必须验证根内 canonical path、拒绝符号链接、真实大小/SHA-256、参考 PNG、`(fixture, providerProfile, platform)` 无重复且覆盖完整；示例中的占位摘要永远不能成为验收输入。
3. 在 `crates/filesystem/src/scanner.rs` 的单元与集成测试先建立注册格式的基础记录，证明属性或 preview provider 缺失时仍保留文件字段、稳定 kind、相邻 Sidecar 与查询可见性；随后才移除 `is_supported_mvp_image` 过滤。
4. 在 `crates/preview` 把“没有已安装 provider”与“内容损坏/解码失败”拆成稳定结果。缓存键在 provider 接入前先预留 provider ID/version，不允许继续只用全局图片 decoder version 表示所有格式。
5. 在 `apps/desktop/src/thumbnail.ts` 与 `AssetThumbnail.tsx` 增加契约和组件测试：`codec-unavailable`、`preview-unavailable` 显示中性的文件类型卡片，只有损坏、不可读、超限或超时显示故障状态。当前把所有 placeholder 渲染成红色“无法预览”的行为不得带入 P3-01A。
6. 复用现有后端 `type:image|video|audio|pdf` 查询语义增加端到端夹具断言，不在 React 中复制格式或查询判定。

## 5. P3-01A 实施证据

截至 2026-08-20，六项均已形成可重复的本地证据：

- `crates/filesystem/src/formats.rs` 注册 PNG、JPEG、GIF、WebP、SVG、AVIF、HEIC、HEIF、MP4、MOV、WebM、MP3、WAV、FLAC 与 PDF；单元测试约束 ID、MIME、扩展名和 kind 唯一一致，并验证每种签名、扩展回退和内容冲突。
- scanner 只读取最多 64 KiB 前缀，内容签名覆盖冲突扩展名并报告 `mime-mismatch`；所有已注册格式都建立基础 `AssetRecord` 并合并相邻 Sidecar，只有四种已有 raster provider 的格式尝试尺寸解析。
- `fixtures/formats/manifest.json` 已包含一个仓库确定生成、真实大小及真实 SHA-256 的 SVG 夹具。独立验证器同时执行 JSON Schema、canonical root、逐段无符号链接、常规文件、大小/SHA-256、禁用占位摘要、provider/profile 三平台完整覆盖，以及可用参考 PNG 的摘要与 IHDR 尺寸检查。
- preview 将 `codec-unavailable`、`preview-unavailable`、`invalid-content`、`resource-limited` 与 `timed-out` 分开；现有 raster provider 的 ID/version 同时进入缓存 key、descriptor 和 ready 回执，缓存 layout 升级到 3。
- 桌面端组件测试证明 codec/provider 缺失显示中性格式卡片，而损坏、不可读、超限和超时显示明确故障；扫描新增的 `mime-mismatch` 也进入前端稳定问题联合类型。
- catalog 集成测试从真实临时文件执行扫描、入库和 `type:image|video|audio|pdf` 查询，未在 React 中复制类型判定。

局部验收命令：

```bash
npm run verify:format-fixtures
node --test tools/format-fixture-manifest.test.mjs
cargo test -p asset-filesystem
cargo test -p asset-preview -p asset-catalog
npm --prefix apps/desktop test
cargo clippy --workspace --all-targets -- -D warnings
```

P3-01A 判定为 **Completed locally**，只关闭“注册表与通用卡片”范围；随后实施结果见
第 6 节。在 P3-01D 至 F、所有正式格式的真实期望清单和恶意/超限资源证据完成前，
不得判定 P3-A01、P3-A02 或 P3-01 通过。

## 6. P3-01B 实施结果

P3-01B 已按独立验收报告判为 **Completed locally**：精确锁定 `resvg/usvg 0.48.1` 且关闭默认字体、系统字体、SVGZ 与 raster image feature；共享 `asset-svg` 核心在扫描和 preview 中执行 16 MiB、100,000 XML 节点、65,535 单边限制，禁用 DTD、resolver、外部/数据引用、脚本、事件、动画和 `foreignObject`。当前固定 provider 不加载字体，含 `<text>` 的安全 SVG 明确降级为 `preview-unavailable`，不会静默产生缺字缩略图。

正常 SVG 的宽高写入可重建 `dimensions`，静态输出由 `safe-static-svg` provider 生成透明 PNG；provider 版本进入既有缓存身份。仓库真实清单现含正常、脚本、外部引用和截断四个 SVG，以及与 provider 字节完全一致的 16 × 16 PNG 参考。

P3-01C 的 core-only、worker 隔离、三平台固定 backend 与 DEB/`.app`/NSIS 成品重放已
全部通过，详见 `p3-01c-acceptance.md`。

## 7. P3-01D 容器属性实施结果

MP4/MOV/WebM 已使用只启用 `isomp4`/`mkv` demuxer 的 Symphonia 0.6.1 提取时长、
宽高、音视频轨道数和已知 codec。ISO BMFF/EBML 预检、32 MiB I/O、4,096 元素/seek、
256 轨道、365 天时长、65,535 单边和扫描 deadline 均已接线；9 个确定性正常/截断/
伪装/未知 codec/超限夹具进入清单。详情见 `p3-01d-acceptance.md`。

视频帧 worker 已按独立发行边界决策暂缓；本报告不判定 P3-A01、P3-A02 或 P3-01 通过。

## 8. P3-01E 音频实施结果

MP3/WAV 使用精确锁定且关闭默认 feature 的 Symphonia 0.6.1 读取容器/ID3 属性，FLAC
使用只读有界 metadata-block walker；三者均不解码或播放音频 sample。32 MiB I/O、
4,096 元素/seek、16 MiB tag/封面、365 天、768 kHz、64 声道、64 bit 和扫描 deadline
均已接线。`embedded-mp3-cover` / `embedded-flac-cover` 只向派生缓存输出验证后的 PNG，
无封面保持音频卡片。10 个正常/截断/伪装/未知 codec/超限夹具进入清单，详情见
`p3-01e-acceptance.md`。

## 9. P3-01F PDF 实施结果

经典 xref PDF 已使用精确锁定且关闭默认 feature 的 lopdf 0.42.0 提取页数和首页面尺寸；
32 MiB 源、100,000 对象/页、65,535 页面单边、页面树与安全字典递归上限均已接线。
加密、对象流和 xref stream 在主 parser 前降级，主动内容只标记不执行；10 个正常、主动
内容、截断、伪装、加密、对象流和超限夹具进入清单。PDFium 首页 worker 已按独立发行
边界暂缓，详情见 `p3-01f-acceptance.md` 和 `p3-01f-pdfium-decision.md`。下一动作是统一
P3-A01/P3-A02；两项及 P3-01 仍未通过。
