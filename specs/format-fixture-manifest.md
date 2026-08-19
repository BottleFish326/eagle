# 扩展格式夹具清单规范

P3-01 的固定格式夹具位于 `fixtures/formats`，根清单为 `fixtures/formats/manifest.json`，必须通过 [`format-fixture-manifest.schema.json`](../schemas/format-fixture-manifest.schema.json)。[`format-fixture-manifest.example.json`](examples/format-fixture-manifest.example.json) 只演示结构，其中摘要和字节数是占位值，不能作为验收输入。在真实夹具和参考预览就绪前不得提交空清单或伪造摘要。

## 1. 完整性与来源

- 每个源文件和参考 PNG 都记录小写 SHA-256；验证器先核对摘要和字节数，再启动解析器；
- `generated-in-repository` 只用于仓库代码能够确定性重建的自有夹具；
- 外部文件必须是 `public-domain` 或明确允许再分发的 `redistributable`，填写许可证、来源 URL 和必要署名；
- 不明来源、仅允许个人测试或带隐私数据的媒体不得提交；
- fixture/reference path 只能使用 `/` 分隔的根内相对路径，验证器 canonicalize 后再次拒绝越界和符号链接。
- 路径各段必须同时兼容 Windows、macOS 和 Linux；拒绝 `.`、`..`、空段、反斜杠、盘符及 Windows 保留标点。

## 2. 平台与提供器 Profile

同一夹具可以声明多条 expectation：

- `core-only`：只有内置安全 Rust 识别/属性能力，不假定原生 codec；
- `bundled-codecs`：发行包规定的 libheif、媒体或 Pdfium worker 可用。

每条 expectation 明确适用 Windows、macOS、Linux 的哪些平台。验证器必须拒绝同一 fixture/profile 下平台重复或缺失，而不能挑一个有利结果。

JSON Schema 负责字段和单条 expectation 的结构约束；跨 expectation 的平台覆盖、重复 `(profile, platform)`、真实文件摘要和 canonical path 必须由清单验证器执行。

## 3. 期望结果

- `recognized` 单独核对；未识别时 MIME/kind 为 `null`，属性和预览给出稳定非 available 状态；
- `metadata.properties` 只写该格式可独立验证的字段，不用容差掩盖整数错误；
- `issueCodes` 使用稳定 code，不比较可能包含库版本或路径的自由文本；
- `preview.status=available` 时必须提供参考 PNG 的路径、摘要、宽和高；验证器既核对协议字段，也核对实际 PNG；
- native codec 缺失、文件损坏、资源超限和超时必须是不同状态/原因。
- 属性或预览状态不是 `available` 时必须给出稳定 `reasonCode`；属性集合为空，预览不得携带参考图字段。

## 4. 资源预算

每个夹具都声明扫描、预览、RSS 增量和预览输出上限，但不得超过协议全局硬限制。正常小夹具应给出更紧预算，不能统一填写全局最大值来规避性能退化。

资源验收在隔离 worker 或专用验证进程中测量。单次偶发抖动可以按报告规定重复，但不能只保留最快一次；最终证据保存全部样本、平台、provider version、构建提交和失败列表。

## 5. 目录与清理

固定夹具属于 Git 内容，不由 `fixture-generator clean` 删除。运行时复制必须进入系统生成的唯一临时目录，测试只清理自己创建且带所有权 marker 的副本。任何清理工具都不得接受未校验的仓库根、用户目录或素材根作为递归删除目标。
