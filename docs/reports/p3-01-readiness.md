# P3-01 扩展格式支持实施准备报告

- 状态：Implementation in progress；P3-01A/P3-01B Completed locally，P3-01C three-platform fixed backend verified
- 日期：2026-08-19
- 对应：P3-01、P3-A01、P3-A02
- 决策：[ADR-026](../../specs/adr/026-capability-based-extended-format-pipeline.md)
- 协议：[扩展格式协议](../../specs/extended-format-protocol.md)

## 1. 当前实现审计

| 层 | 当前能力 | P3-01 缺口 |
|---|---|---|
| 素材模型 | 已有 image/video/audio/pdf/other 类型；只有图片宽高和 EXIF 结构 | 缺少可选媒体时长、页数、采样率、声道、codec、颜色空间 |
| 扫描器 | 15 格式静态注册；64 KiB 内容签名优先、扩展候选；注册格式均保留记录和 Sidecar；SVG 有界提取宽高；AVIF/HEIC 严格限制在首个 `ftyp` box 的 major/compatible brands，固定 worker 属性写入可重建记录 | 媒体/PDF 属性仍待接入 |
| 缩略图 | PNG/JPEG/GIF/WebP 使用 `builtin-raster`；SVG 使用 `safe-static-svg`；固定 libheif 1.23.1 backend 已在三平台真实输出受限 PNG；worker 缺失仍稳定降级 | AVIF/HEIC 三平台随应用打包与后续原生/复杂 provider 尚未关闭 |
| 缓存 | provider ID/version 已进入每项 key 与 Schema 2 descriptor；layout 3 自动失效旧项 | 后续每个新增 provider 必须登记当前版本并增加失效测试 |
| 查询/UI | `scan_root → AssetCatalog → type:` 四类测试已贯通；codec/provider 缺失显示中性类型卡片，文件故障显示错误卡片 | P3-01C 至 F 仍需逐格式扩展属性和真实预览 |
| 夹具 | 已有正常、脚本、外部引用、截断 SVG；AVIF/HEIC 正常样本、两张 64px bundled 参考 PNG 及 7 个 AVIF 损坏/伪装/超限派生样本；清单语义验证器约束完整性 | 仍需补齐 SVG 超大正式资源证据、参考 PNG 三平台一致性及后续格式夹具 |

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

本报告不表示 P3-01 已完成。阶段 2 最终退出收据已接受，P3-01A 与 P3-01B 本地门禁已经关闭；P3-01C 至 F、全格式期望矩阵和完整恶意夹具资源证据尚未完成。

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

P3-01A 判定为 **Completed locally**，只关闭“注册表与通用卡片”范围；随后实施结果见第 6 节。在 P3-01C 至 F、所有正式格式的真实期望清单和恶意/超限资源证据完成前，不得判定 P3-A01、P3-A02 或 P3-01 通过。

## 6. P3-01B 实施结果

P3-01B 已按独立验收报告判为 **Completed locally**：精确锁定 `resvg/usvg 0.48.1` 且关闭默认字体、系统字体、SVGZ 与 raster image feature；共享 `asset-svg` 核心在扫描和 preview 中执行 16 MiB、100,000 XML 节点、65,535 单边限制，禁用 DTD、resolver、外部/数据引用、脚本、事件、动画和 `foreignObject`。当前固定 provider 不加载字体，含 `<text>` 的安全 SVG 明确降级为 `preview-unavailable`，不会静默产生缺字缩略图。

正常 SVG 的宽高写入可重建 `dimensions`，静态输出由 `safe-static-svg` provider 生成透明 PNG；provider 版本进入既有缓存身份。仓库真实清单现含正常、脚本、外部引用和截断四个 SVG，以及与 provider 字节完全一致的 16 × 16 PNG 参考。

P3-01C 的 core-only、worker 隔离和三平台 libheif backend 已形成托管证据，详见 `p3-01c-readiness.md`；下一动作固定为三平台随应用打包。本报告仍不判定 P3-A01、P3-A02 或 P3-01 通过。
