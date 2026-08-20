# P3-02 智能属性与高级过滤 / P3-A03 验收报告

- 状态：Accepted
- 日期：2026-08-21
- 最终实现提交：`8a0fc304f12fb65931d15aac1831e96d60643dac`
- 非结论：不代表阶段 3 或 P3-A04 至 P3-A11 通过

## 1. 验收结论

P3-02A-E 与 P3-A03 的全部固定条件已经满足。高级查询仍由 Rust catalog 统一解析和执行；
桌面 UI、保存条件和验收工具不维护素材快照。文件派生属性只进入可删除运行时记录，不写入
Sidecar；Tag、评分、收藏和备注仍只从相邻可读 Sidecar 合并。

最终证据由三份不可变 JSON 组成：

| 证据 | SHA-256 | 结论 |
|---|---|---|
| `evidence/p3-a03-query-conformance.json` | `b301556c…` | 64/64 合法用例的 oracle/expected/product 三方一致，25/25 非法用例错误一致 |
| `evidence/p3-a03-query-performance.json` | `fe745278…` | 100,000 条、200 次组合查询 p95 34.128 ms，RSS 峰值 318,048 KiB |
| `evidence/p3-a03-query-scan.json` | `37c29519…` | 8 个真实文件与 8 个 Sidecar 扫描后 20/20 合法、3/3 非法查询通过，素材摘要不变 |

三份报告的 `accepted` 均为 `true`、`failures` 均为空。

## 2. 定型查询与独立一致性

固定语料包含 64 条逻辑记录、64 个合法用例和 25 个非法用例，覆盖版本 1 条件、15 个高级
字段、显式 unknown、单位/溢出、开闭区间、RFC 3339 时区、精确分数、显示旋转、Unicode
路径、根 UUID、颜色空间与布尔冲突。Schema 与跨项语义验证通过。

独立 Node oracle 不导入产品 parser/index；产品 CLI 直接链接正式 `asset-index`。每个合法
用例先证明 oracle 等于固定 `expectedKeys`，再证明产品等于相同集合。故意注入 product、
expected 和 oracle 三类变异时验证器都会失败。固定语料 SHA-256 为
`501c582ff9b39f61c9eeca8c230dde7efb6cd5270f5aec6e5ee733be52b78d31`。

## 3. L 性能与资源

Release 产品索引加载 100,000 条记录后执行 `combined-advanced` 200 次：

- 结果数固定为 20,312；
- 索引构建 198.657 ms；
- p50 32.965 ms、p95 34.128 ms、最大 36.513 ms，低于统一 100 ms p95 基线；
- 外部进程树 113 个 RSS 样本，基线 6,304 KiB、峰值 318,048 KiB、增量 311,744 KiB，
  低于 1 GiB 上限；
- 完整 200 个原始延迟样本已归档并可独立重放。

## 4. 真实文件、Sidecar 与原素材安全

最终扫描门禁只读选取仓库固定的 SVG、MP4、MOV、WebM、MP3、WAV、FLAC 和 PDF 普通文件，
复制到隔离临时素材根，再使用正式 metadata writer 原子创建 8 个带完整/快速指纹的相邻
Sidecar。正式 filesystem scanner 以固定授权根 UUID 扫描并把 8 条记录送入正式 catalog。

20 个查询覆盖 Tag、评分、收藏、备注、类型、宽度、方向、宽高比、时长、页数、根、路径、
大小、修改时间与未知属性；另有 3 个错误复核稳定错误种类和 UTF-8 字节偏移。全部匹配独立
相对路径集合，扫描问题数为 0。

跟踪源文件及隔离副本的聚合 SHA-256 在扫描/查询前后均为
`97353d6a1f4d17e2ad43722b368a13fd7df56bacf7173a07d3e40bacac314782`。收据只保存仓库相对
路径、稳定 ID、派生字段和摘要，不保存临时绝对路径；敏感信息审计无命中。

## 5. 桌面交互与错误保留

桌面可视化编辑器覆盖 15 个高级字段，只生成协议允许的操作符、值和单位。浏览器实测由
编辑器添加 `rating:>=4` 后 16 项变为 7 项；再输入无效 `kind:image` 时仍保留 7 项，并展示
`unknown-filter`、Token 和 UTF-8 字节 0。字段、操作符、值、单位、状态和按钮均有可访问
名称，640 px 窄屏弹层不溢出。

该结果只完成 P3-02E；P3-A10 仍需在阶段 3 退出前对搜索、筛选、选择、预览和复制引用执行
完整人工键盘路径。

## 6. 可重放性与零漂移

扫描收据在干净 `8a0fc30` 上由 Release 二进制连续重放，结果逐字节一致。完整 `npm run ci`
在同一提交通过：111 项工具测试、Rust workspace（含扫描门禁）、桌面 73 项、Obsidian 8 项、
S 数据集、严格格式/Clippy/TypeScript、Release 桌面与 bridge build 全部成功。

从 conformance 收据绑定提交 `cb7708f` 到 `8a0fc30`，产品 parser/index、query-gate、固定语料
与独立 conformance 工具零漂移；从性能收据绑定提交 `c4600d9` 到 `8a0fc30`，相同产品与性能
工具范围也零漂移。因此两份早期干净收据仍精确适用于最终实现提交。

P3-02 与 P3-A03 判定为 **Accepted**。下一工作项为 P3-03 保存过滤器；P3-A04 至 P3-A11
仍保持未通过，阶段 3 不退出。
