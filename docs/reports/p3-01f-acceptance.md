# P3-01F PDF 属性与安全降级验收报告

- 状态：Completed locally；PDFium 首页渲染 Deferred by design
- 日期：2026-08-21
- 范围：PDF 页数、首页面尺寸、安全结构检查、故障隔离和 provider 缺失降级
- 非结论：不代表 P3-01、P3-A01、P3-A02 或阶段 3 通过

## 1. 验收结论

经典 xref PDF 现在可以在不渲染页面、不执行动作和不读取内容 stream 的前提下派生页数与
首页面尺寸。文档中的 JavaScript、打开动作、附件、远程跳转、表单提交和外部 URI 只产生
`unsafe-embedded-content` 标记；应用不会执行、打开或访问它们。

加密、对象流和 xref stream 在进入通用 parser 前就被识别为需要隔离 worker 的可选能力，
所以素材记录、相邻 Sidecar、`type:pdf` 查询和 Obsidian 引用仍然存在。截断或结构损坏映射
为 `invalid-native-metadata`，超大页数/页面映射为 `resource-limited`，都只影响单文件富化。

扫描器仍以素材文件和相邻 Sidecar 为唯一真相源。PDF 字段只进入可丢弃、可重建的
`AssetRecord.dimensions/media`；本切片不创建素材副本、不改写 PDF、文件名、目录、Tag 或
Sidecar，也不把 PDF 属性保存成第二份权威数据。

## 2. 固定实现边界

| 项目 | 判定 |
|---|---|
| 依赖 | `lopdf = 0.42.0` 精确锁定并关闭默认 feature；与当前 Rust 1.85+ 工具链兼容；无原生动态库 |
| 输入 | 只接受非符号链接普通文件；源最大 32 MiB；必须以 `%PDF-1.`/`%PDF-2.` 开头并在末尾 1,024 字节内包含 `%%EOF` |
| 结构 | 只解析 strict classic xref；进入 parser 前拒绝 `/Encrypt`、`/ObjStm` 与 `/XRef`，不在主进程解密或展开压缩对象/xref stream |
| 上限 | 最多 100,000 个对象、100,000 页、65,535 首页单边；页面树父链最多 128 层，安全字典递归最多 100 层 |
| 属性 | 复核 `/Pages /Count`、实际 page map、首页继承的 `CropBox`/`MediaBox` 和 90 度倍数旋转；小数边界向上取整为整数点 |
| 主动内容 | 标记 `AA`、`EmbeddedFiles`、`Filespec`、`JavaScript`、`JS`、`Launch`、`OpenAction`、`RichMedia`、`SubmitForm`、`URI`、`GoToR` |
| 隔离 | parser 不渲染页面、不解压页面内容 stream、不执行 action；解析前后复核文件长度与修改时间，扫描复用每文件 5 秒协作 deadline |
| 预览 | `application/pdf` 稳定返回中性 `preview-unavailable` 卡片；清单固定为 `unsupported-feature/pdfium-worker-unavailable`，不得回退到 `PATH` 工具 |

## 3. 固定夹具与清单

仓库生成器确定性产生 10 个 PDF 夹具：两页普通 PDF、JavaScript 动作、外部 URI、截断
文件、PNG 伪装 PDF、PDF 伪装 PNG、加密 trailer、对象流、超大页数和超大页面。所有夹具
均为仓库生成、MIT 许可；生成器测试要求跟踪字节与重新生成结果完全一致。

格式清单现包含 42 个源文件、1,468,255 字节源内容和 24,718 字节引用计数。新增 10 项均
覆盖 Windows、macOS 与 Linux 的 `core-only` 期望；普通与主动内容 PDF 固定为 2 页、
612 × 792。PNG 伪装夹具按内容识别为 PNG 并复用 SHA-256 为 `ead9b240…` 的 16 × 16
参考图；反向伪装按内容识别为 PDF。扫描测试逐个复核所有源文件字节不变。

## 4. 可重复证据

```bash
npm run generate:pdf-format-fixtures
npm run verify:format-fixtures
node --test tools/generate-pdf-format-fixtures.test.mjs tools/format-fixture-manifest.test.mjs
cargo test -p asset-pdf -p asset-filesystem -p asset-preview --all-targets
cargo clippy --workspace --all-targets -- -D warnings
npm run ci
```

- 实现提交 `0d4827c`；提交前 staged sensitive-pattern audit 无命中；
- `asset-pdf`：3 项测试覆盖普通属性、主动内容、截断、加密、对象流、超大声明与零 deadline；
- `asset-filesystem`：58 项测试通过，其中 PDF 测试精确断言属性、内容优先识别、故障隔离
  和所有源文件逐字节不变；
- `asset-preview`：24 项测试通过，PDF provider 缺失与内容损坏保持不同的稳定结果；
- 干净提交 `0d4827c` 上完整 `npm run ci` 通过：工具测试 97 项、Rust workspace 全部测试、
  桌面 UI 52 项、Obsidian bridge 8 项、S 数据集、严格 Clippy/TypeScript、Tauri release 与
  bridge production build 均接受。

## 5. 未关闭范围

- 全格式 P3-A01 清单与实际 scanner/preview 结果的统一执行器和三平台重放；
- P3-A02 对 SVG、AVIF/HEIC、视频、音频和 PDF 的统一超大/损坏/恶意集合，取消与资源
  峰值正式证据；
- 已按独立发行边界暂缓的视频帧与 PDFium 首页 worker；
- P3-02 至 P3-07 及 P3-A03 至 P3-A11。

因此 P3-01F 基础格式切片判定为 **Completed locally**，下一动作是统一 P3-A01/P3-A02
门禁；P3-01 与阶段 3 保持未通过。
