# P3-01 扩展格式支持实施准备报告

- 状态：Design ready；implementation gated by Stage 2 exit
- 日期：2026-08-19
- 对应：P3-01、P3-A01、P3-A02
- 决策：[ADR-026](../../specs/adr/026-capability-based-extended-format-pipeline.md)
- 协议：[扩展格式协议](../../specs/extended-format-protocol.md)

## 1. 当前实现审计

| 层 | 当前能力 | P3-01 缺口 |
|---|---|---|
| 素材模型 | 已有 image/video/audio/pdf/other 类型；只有图片宽高和 EXIF 结构 | 缺少可选媒体时长、页数、采样率、声道、codec、颜色空间 |
| 扫描器 | infer + 扩展回退；仅四种 MVP 图片进入记录 | 识别与 decoder 支持耦合，其他正式格式被直接丢弃，Sidecar 也无法合并 |
| 缩略图 | PNG/JPEG/GIF/WebP，65,535 单边、256 MiB 分配、共享 Decode 许可 | 非图片统一 unsupported；无 provider registry、worker 或 codec-unavailable 区分 |
| 缓存 | provider 版本进入全局 decoder version，20,000 项/1 GiB/30 天 | 多 provider 需要把 provider ID/version 纳入每项 key 与 descriptor |
| 查询/UI | 类型过滤已经支持五种 kind；通用占位原因已有基础 | 扫描不产出对应记录，因此视频/音频/PDF 路径尚未真实贯通 |
| 夹具 | 大规模 PNG、损坏 PNG、Sidecar 异常和安全清理 marker | 缺少逐格式正常/损坏/伪装/超限/恶意固定夹具及期望清单 |

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

本报告不表示 P3-01 已实现。P2-A12 三平台原生路径矩阵已通过；当前 release 正在对 Windows 长路径修复后的候选提交重新执行 P2-A11 连续 8 小时正式验收。P2-A11 与其余阶段 2 退出收据全部通过后，按 P3-01A 开始代码和夹具实现。

在门禁结束前允许继续审阅本协议和准备可再分发夹具来源，但不得把设计文档、交叉编译或 codec 库能力说明计作 P3-A01/P3-A02 通过。

## 4. 阶段切换后的首个测试切片

P3-01A 必须以测试和清单验证器开场，不能先扩大扩展名白名单。当前代码的精确落点与完成顺序如下：

1. 在 `crates/filesystem` 增加静态格式注册表及纯识别测试。注册表先证明 descriptor ID、扩展名、MIME 与 `AssetKind` 唯一且一致，再覆盖签名优先、扩展候选和内容冲突；`asset-core` 继续只持有稳定素材值模型，不承担文件探测。
2. 为 `fixtures/formats/manifest.json` 增加独立语义验证器。除 JSON Schema 外，必须验证根内 canonical path、拒绝符号链接、真实大小/SHA-256、参考 PNG、`(fixture, providerProfile, platform)` 无重复且覆盖完整；示例中的占位摘要永远不能成为验收输入。
3. 在 `crates/filesystem/src/scanner.rs` 的单元与集成测试先建立注册格式的基础记录，证明属性或 preview provider 缺失时仍保留文件字段、稳定 kind、相邻 Sidecar 与查询可见性；随后才移除 `is_supported_mvp_image` 过滤。
4. 在 `crates/preview` 把“没有已安装 provider”与“内容损坏/解码失败”拆成稳定结果。缓存键在 provider 接入前先预留 provider ID/version，不允许继续只用全局图片 decoder version 表示所有格式。
5. 在 `apps/desktop/src/thumbnail.ts` 与 `AssetThumbnail.tsx` 增加契约和组件测试：`codec-unavailable`、`preview-unavailable` 显示中性的文件类型卡片，只有损坏、不可读、超限或超时显示故障状态。当前把所有 placeholder 渲染成红色“无法预览”的行为不得带入 P3-01A。
6. 复用现有后端 `type:image|video|audio|pdf` 查询语义增加端到端夹具断言，不在 React 中复制格式或查询判定。

以上六项均在阶段 2 最终退出收据接受后执行；本节只把代码审计结果转换为可直接执行的测试顺序，不表示阶段 3 已开始。
