# 查询一致性语料规范

P3-A03 的固定输入为 `fixtures/queries/manifest.json`，结构由 [`query-conformance-manifest.schema.json`](../schemas/query-conformance-manifest.schema.json) 约束。本规范把“产品查询结果与独立验证程序一致”落实为可重复、不会自证的三方比较。

## 1. 三方证据

每个合法用例包含：

1. `expression`：交给正式产品解析器和索引的文本；
2. `oracle`：人工审阅的规范化谓词，不是产品 AST 的序列化；
3. `expectedKeys`：提交到 Git 的固定有序键集合。

离线验证器必须以自身的简单线性逻辑在 `records` 上执行 oracle，先证明结果等于 `expectedKeys`，再调用独立构建的产品查询 CLI 并比较同一集合。验证器不得链接或导入 `asset-index`、`catalog`、Tauri 命令、桌面 demo adapter 或它们的测试 helper。

`expectedKeys` 按 Unicode 码点升序保存；验证器拒绝重复、未声明的键以及 record/case ID 重复。JSON Schema 只负责单个对象的结构，以上跨项约束由语义验证器强制执行。

## 2. Record 语义

语料记录是最小、无隐私的运行时输入，不是新的素材存储格式：

- `key` 只在语料内标识记录；
- `relativePath` 使用 `/`，不得是绝对路径或包含 `.`/`..` 段；
- `width`/`height` 是文件报告的原始正整数；`displayQuarterTurns` 为 1 或 3 时，oracle 先交换宽高；
- `null` 表示属性未知，绝不能替换为零、空字符串或 `false`；
- `colorSpace` 使用 provider 规范化后的小写稳定值；
- `note`、`tags` 是合成数据，不能从真实用户目录采集。

正式 P3-A03 还必须用 fixture-generator 从这些逻辑记录生成一套普通文件/Sidecar 输入，并在测试前后核对原素材 SHA-256。manifest 本身不能替代端到端扫描证据。

## 3. Oracle 谓词

谓词由 `field`、`operator` 和 `value` 组成。允许组合由语义验证器固定：

| 字段 | operator | value |
|---|---|---|
| `tag` | `all`, `any`, `none` | 唯一字符串数组 |
| `type`, `extension`, `orientation`, `root`, `color-space` | `any` | 唯一字符串数组 |
| `favorite`, `has-note`, `has-alpha` | `eq` | boolean；`has-alpha` 也可 `is-unknown` |
| `rating`, `size`, `width`, `height`, `created`, `modified`, `duration`, `pages` | `eq`, `lt`, `lte`, `gt`, `gte` | 已换算整数 |
| `aspect` | `eq`, `lt`, `lte`, `gt`, `gte` | 已约分 `{numerator, denominator}` |
| `path` | `contains` | NFC 字符串 |
| 可为空字段 | `is-unknown` | 固定字符串 `unknown` |

时间 oracle 值是 Unix 毫秒，大小是字节，时长是毫秒。这样参考计算不复现产品的文本单位或 RFC 3339 解析；文本解析错误会在最终集合差异或 invalid case 中暴露。

## 4. Invalid case

无效用例固定 `errorKind` 和零基 UTF-8 字节 `offset`。产品必须在完整解析结束前不执行查询；界面保留上一次合法结果。语料至少各覆盖一次新增错误，并保留版本 1 的未知字段、未闭合引号和非法 OR 回归。

## 5. 最小覆盖矩阵

正式语料至少包含 40 条记录、60 个合法用例和 24 个无效用例，并覆盖：

- 零、恰好边界、边界两侧和最大合法值；
- 未知 size/created/dimensions/media/alpha 与明确零/false；
- 1:1、4:3、3:4、16:9 以及 EXIF 等效旋转；
- 同一时刻的 `Z`、正偏移和负偏移表达；
- 组合 Tag、类别、数值范围、路径和根；
- 中文、日文、Emoji、组合字符、ASCII 大小写和带空格路径；
- 同名不同根、已移除根 ID、缺失 root ID；
- codec 缺失但素材仍存在的记录；
- 空结果、全量结果以及至少一个 10 条以上候选集合。

示例文件只说明结构，不计入上述数量或 P3-A03 证据。

## 6. 运行报告

验收报告记录 schema/semantic validator 版本、语料 SHA-256、产品提交、Node/Rust 版本、平台、用例数、每个用例的 oracle/expected/product 三方状态、p50/p95 和失败列表。报告不得包含工作目录绝对路径；只保存合成 key 和稳定用例 ID。
