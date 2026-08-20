# P3-02A 至 P3-02D 智能属性与高级查询验收报告

> 状态：Completed locally；P3-02E 与 P3-A03 最终退出未完成
>
> 日期：2026-08-21

## 1. 验收结论

P3-02A 至 P3-02D 已形成可重复的后端闭环：查询文本先解析为定型 AST；低基数字段使用集合倒排，整数、时间、精确比例和路径在候选集上线性求值；EXIF 与视频显示旋转先换算有效宽高；独立 Node oracle 不导入产品 parser/index，并与固定 `expectedKeys` 和独立 Rust 产品 CLI 做三方比较。

本报告不关闭 P3-02 或 P3-A03。桌面可视化字段编辑器、无效输入保留上次合法结果和辅助功能属于 P3-02E；从普通文件与 Sidecar 重扫生成逻辑记录、核对原始素材 SHA-256 的端到端收据仍需补齐后，P3-A03 才能判定。

## 2. 已完成范围

| 切片 | 实现 | 验收结果 |
|---|---|---|
| P3-02A | 15 个高级字段；整数单位、RFC 3339、精确分数、显式 unknown、范围归一化、稳定错误与 UTF-8 字节偏移 | Pass |
| P3-02B | `MediaProperties.displayQuarterTurns`；ISO BMFF `tkhd` 矩阵、WebM `ProjectionPoseRoll`、EXIF 方向 5–8；字段只来自文件并可重扫 | Pass |
| P3-02C | root/orientation/color-space/note/alpha 倒排；候选集范围/路径求值；upsert/remove 清理陈旧 posting | Pass |
| P3-02D | 64 record、64 合法、25 无效固定语料；Schema、语义、独立 oracle、产品 CLI、三类注错检测 | Pass |

视频旋转只保存为可删除的运行时 `media` 派生属性，不进入 Sidecar；查询适配器只接收固定逻辑记录，不创建素材副本或数据库。ISO 矩阵和 WebM 投影读取都受既有 16 MiB 元数据、4,096 元素和源版本复核边界约束。

## 3. 正确性证据

固定语料：`fixtures/queries/manifest.json`

- record：64；
- 合法用例：64/64 oracle 等于固定 `expectedKeys`，64/64 产品结果等于同一集合；
- 无效用例：25/25 错误种类和零基 UTF-8 字节 offset 一致；
- 覆盖全部 19 个 oracle 字段和 23 个稳定解析错误种类；
- 覆盖旋转宽高、未知值、零值、边界、UTC/正偏移、中文、日文、Emoji、大小写路径、同名跨根与已移除根；
- 注入 product、expected、oracle 任一错误时，独立门禁均拒绝。

正式一致性 JSON：`docs/reports/evidence/p3-a03-query-conformance.json`

- manifest SHA-256：`501c582ff9b39f61c9eeca8c230dde7efb6cd5270f5aec6e5ee733be52b78d31`；
- evidence SHA-256：`b301556c1921572c1045c45cc2cd10196269776e3843f50a6ff279f1029e4895`；
- 产品提交：`cb7708fcb84c3988c966201ef1a20f7eff6a402d`；
- Node.js：24.19.0。

## 4. L 数据集性能与资源证据

正式 Release 门以 64 条固定逻辑记录确定性扩展为 100,000 个唯一运行时 key，对 `combined-advanced` 执行 200 次。查询同时包含 rating、size、有效 width、orientation 和 has-note；每次结果数固定为 20,312。

| 指标 | 结果 | 门限 | 结论 |
|---|---:|---:|---|
| 索引构建 | 198.657 ms | 记录，不作交互门限 | Recorded |
| 查询 p50 | 32.965 ms | — | Recorded |
| 查询 p95 | 34.128 ms | 100 ms | Pass |
| 查询 max | 36.513 ms | — | Recorded |
| RSS baseline | 6,304 KiB | — | Recorded |
| RSS max | 318,048 KiB | 1,048,576 KiB | Pass |
| RSS delta | 311,744 KiB | 记录 | Recorded |
| 进程树样本 | 113 | 至少 10 | Pass |

正式性能 JSON：`docs/reports/evidence/p3-a03-query-performance.json`

- 绑定提交：`c4600d97d17499e0b2d23850d3a51fe717990ec4`；
- manifest 前后 SHA-256 一致；
- evidence SHA-256：`fe745278d36d4fc2627394668cfe2e43a97ddf0b7f5c7ad0512faae3b9e1bff6`；
- 保存全部 200 个原始延迟样本和 113 个进程树 RSS 样本，汇总值由门禁重放后生成；
- 敏感信息扫描未发现绝对用户路径、用户名、私钥、密码、token 或 secret。

## 5. 文件系统真相源与安全边界

- 产品索引只持有 `AssetRecord` 运行时记录，不落数据库，不保存查询结果快照；
- `displayQuarterTurns`、尺寸、时长、页数、颜色空间和 alpha 都是文件派生值，Sidecar Schema 未增加这些字段；
- 删除运行时目录/缓存并重扫时由文件 provider 重新获得派生属性；
- 路径查询只使用 root-relative NFC 路径，不授予或扩大根目录权限；
- 正式性能门读取固定 manifest，运行前后摘要相同，不修改原素材或 Sidecar。

## 6. 尚未完成

1. P3-02E 桌面字段条件编辑器、结构化错误位置、最后合法结果保留、键盘与屏幕阅读器路径；
2. 用普通文件/Sidecar 输入驱动扫描器，证明逻辑 record 可从文件系统重建，并保存测试前后原始素材 SHA-256；
3. P3-A03 最终收据、证据 Schema/离线重放和 reviewed 缺陷审计；
4. 阶段 3 的 P3-03 至 P3-07 与 P3-A04 至 P3-A11。

因此当前状态只能写为 **P3-02A-D Completed locally / P3-02E pending / P3-A03 not closed**。
