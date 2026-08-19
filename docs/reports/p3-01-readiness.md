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

## 3. 门禁状态

本报告不表示 P3-01 已实现。当前 release 正在执行 P2-A11 连续 8 小时正式验收；P2-A12 仍需要三平台原生路径矩阵。两项完成并确认阶段 2 退出后，按 P3-01A 开始代码和夹具实现。

在门禁结束前允许继续审阅本协议和准备可再分发夹具来源，但不得把设计文档、交叉编译或 codec 库能力说明计作 P3-A01/P3-A02 通过。
