# 开发进度

> 最后更新：2026-08-19

## 总体状态

| 阶段 | 状态 | 说明 |
|---|---|---|
| 阶段 0：技术验证与架构定案 | Accepted | P0-A01 至 P0-A08 全部通过 |
| 阶段 1：端到端 MVP | Accepted locally | P1-01 至 P1-09、P1-A01 至 P1-A12 全部通过；见阶段 1 验收报告 |
| 阶段 2：可靠性与恢复 | In progress | P2-01 至 P2-03 已完成本地验收；下一项 P2-04 并发编辑与同步冲突 |
| 阶段 3：完整素材能力 | Not started | 等待阶段 2 退出 |
| 阶段 4：Obsidian 深度集成 | Not started | 等待阶段 3 退出 |

## 阶段 0 工作项

| 工作项 | 状态 | 证据 |
|---|---|---|
| P0-01 架构决策记录 | Completed | `specs/adr/001` 至 `008` |
| Sidecar 与 Library Schema | Completed | `schemas/*.schema.json` |
| P0-02 测试素材生成器 | Completed | S/M/L 规模、安全清理标记和异常夹具 |
| P0-03 增量扫描原型 | Implemented | 100,000 素材完整扫描 1.454 秒 |
| P0-04 内存索引与查询原型 | Completed | 100,000 素材查询 p95 19.082 ms |
| P0-05 Sidecar 写入与冲突原型 | Completed | 三个进程崩溃点保持完整，CLI 提供 abort/reload/merge 冲突策略 |
| P0-06 文件监听原型 | Completed | 创建/移动/删除风暴产生 68,362 个事件，最终扫描准确收敛到 5,000 个素材 |
| P0-07 Obsidian 联动原型 | Completed | Node 24 LTS 自动化通过；Obsidian 1.12.7 内外部引用实机渲染通过 |
| 阶段 0 验收报告 | Accepted | P0-A01 至 P0-A08 全部通过，截图证据已归档 |

## 阶段 1 工作项

| 工作项 | 状态 | 证据 |
|---|---|---|
| P1-01 项目骨架与持续集成 | Completed locally | 桌面端 Release 构建、质量门禁和 S 数据集测试通过；见 `reports/p1-01-acceptance.md` |
| P1-02 素材库根目录管理 | Completed locally | 增删、停用、路径诊断、YAML 原子持久化和原文件保护测试通过；见 `reports/p1-02-acceptance.md` |
| P1-03 正式扫描器与素材模型 | Completed locally | 四种图片格式、尺寸与 EXIF、增量批次、取消、错误隔离和 S 数据集测试通过；见 `reports/p1-03-acceptance.md` |
| P1-04 Sidecar 元数据编辑 | Completed locally | 首次 Sidecar、全字段补丁、20 素材批量 Tag、冲突隔离和索引即时更新通过；见 `reports/p1-04-acceptance.md` |
| P1-05 Tag 查询引擎 | Completed locally | 默认 AND、同组 OR、排除、命名空间、类型/扩展名/收藏过滤和结构化解析错误通过；L 数据集查询 p95 53.691 ms；见 `reports/p1-05-acceptance.md` |
| P1-06 缩略图管线 | Completed locally | 四格式解码、GIF 首帧、视口按需生成、并发限流、版本化缓存、损坏占位和安全清理通过；见 `reports/p1-06-acceptance.md` |
| P1-07 桌面端最小 UI | Completed locally | 根目录管理、增量扁平网格、三态 Tag、查询、检查器、多选批量编辑、错误状态和键盘可访问性通过；见 `reports/p1-07-acceptance.md` |
| P1-08 Obsidian Vault 内引用 | Completed locally | 多 Vault 原子配置、真实路径边界、标准 WikiLink、复制与文本拖放、同名目录消歧通过；见 `reports/p1-08-acceptance.md` |
| P1-09 应用配置与可恢复性 | Completed locally | 可读 YAML 偏好、原子持久化、缓存版本自动重建、安全显式重建、脱敏诊断导出和用户数据说明通过；见 `reports/p1-09-acceptance.md` |
| 阶段 1 退出验收 | Accepted locally | P1-A01 至 P1-A12 全部通过；M UI 连续 30 分钟、2,025 次动作、0 错误；见 `reports/phase-1-acceptance.md` |

> 当前仓库未配置 Git 远程地址，因此 GitHub Actions 的首次托管运行仍待远程仓库建立后确认；工作流所执行的同一组命令已在本机通过。

## 阶段 2 工作项

| 工作项 | 状态 | 证据 |
|---|---|---|
| P2-01 统一文件事件模型 | Completed locally | 120 ms 静默/750 ms 最长批次、4,096 条溢出保护、临时文件折叠、单根自动/手动一致性扫描通过；见 `reports/p2-01-acceptance.md` |
| P2-02 移动与孤立文件处理 | Completed locally | 稳定 ID 成对移动保持、孤立/丢失/候选诊断、三级指纹、歧义选择与显式无覆盖 Sidecar 重联通过；见 `reports/p2-02-acceptance.md` |
| P2-03 批量操作事务与恢复 | Completed locally | 全量预写计划、逐项原子提交、32 项检查点、摘要重建、继续/条件恢复、授权根复核与外部修改保护通过；见 `reports/p2-03-acceptance.md` |
| P2-04 并发编辑与同步冲突 | Next | 等待实现集合字段显式合并、非集合字段差异选择与同步冲突副本诊断 |

## 已固定的关键决定

- 文件系统与相邻 Sidecar 是唯一真相源；
- Sidecar 使用 YAML，逻辑结构由 JSON Schema 约束；
- 稳定 ID 使用 UUIDv7；
- 桌面端采用 Tauri 2、Rust、React、TypeScript 与 Vite；
- JavaScript 工具链固定为 Node.js 24 LTS；
- 阶段 0 核心原型先以 Rust workspace 和 CLI 完成；
- Vault 内使用标准 Obsidian 引用，Vault 外使用 `material://<uuid>`；
- 默认不跟随符号链接，外部渲染只能访问显式授权根；
- Sidecar 编辑按 SHA-256 执行乐观并发控制，成功持久化后才更新内存索引。
- 查询文本先完整解析再执行，解析错误不得转换为零结果；查询索引保持可删除、可重建。
- 缩略图只按视图请求生成；缓存键包含路径、稳定 ID、大小、mtime、尺寸和解码器版本，缓存固定在操作系统应用缓存目录。
- React 只维护扫描批次产生的瞬态素材视图；正式查询、Sidecar 写入和缩略图读取继续通过 Tauri 调用 Rust，浏览器演示适配器不读写用户文件。
- Vault 授权使用独立可读 YAML 原子持久化；Rust 只对目录中的真实素材生成完整 Vault 相对 WikiLink，符号链接逃逸与保留字符显式失败。
- Obsidian 引用复制只申请系统剪贴板文本写入权限；网格拖放传递标准 Markdown 文本，不传递文件 URL 或复制素材。
- 应用偏好使用独立 `application.yml` 原子持久化，只保存查询、Tag 三态和当前 Vault；素材记录与查询结果仍为可重建运行期状态。
- 缓存重建命令不接受路径参数，只重置固定且带版本标记的 `thumbnails-v1`；诊断导出只包含计数、状态、短路径指纹和有界事件。
- 文件监听按根目录归一化和有界批处理；半重命名、越界、通道错误与溢出只触发已授权根的一致性扫描，监听事件不成为第二真相源。
- Sidecar 创建/编辑保存大小、快速指纹和完整 SHA-256；扫描完成时按稳定 ID 收敛移动，孤立重新关联必须由用户确认且目标不可覆盖。
- 两项以上的元数据编辑先把纯文件计划日志原子写入固定配置子目录；重启按原/计划/当前摘要重建状态，继续和恢复都必须重新验证素材指纹、Sidecar 摘要与授权根。

## 当前性能证据

- [阶段 0 性能报告](reports/phase-0-performance.md)
- [阶段 0 验收报告](reports/phase-0-acceptance.md)
- [阶段 1 性能与稳定性报告](reports/phase-1-performance.md)
- [阶段 1 验收报告](reports/phase-1-acceptance.md)
- [P1-05 查询引擎验收报告](reports/p1-05-acceptance.md)
- [P1-06 缩略图管线验收报告](reports/p1-06-acceptance.md)
- [P1-07 桌面端最小 UI 验收报告](reports/p1-07-acceptance.md)
- [P1-08 Obsidian Vault 内引用验收报告](reports/p1-08-acceptance.md)
- [P1-09 应用配置与可恢复性验收报告](reports/p1-09-acceptance.md)
- [P2-01 统一文件事件模型验收报告](reports/p2-01-acceptance.md)
- [P2-02 移动与孤立文件处理验收报告](reports/p2-02-acceptance.md)
- [P2-03 批量操作事务与恢复验收报告](reports/p2-03-acceptance.md)
- [平台差异记录](../specs/platform-notes.md)
- 参考环境：Apple M4、16 GiB、APFS SSD、macOS 26.5.2；
- L 数据集：100,000 个素材、20,000 个 sidecar；
- 完整扫描：1.454 秒；
- 查询 p95：19.082 毫秒；
- 峰值常驻内存：202,473,472 字节，约 193 MiB。
- P1-05 正式复合查询（L 数据集、200 次）p95：53.691 毫秒。
- 阶段 1 M 数据集：10,000 素材、2,000 Sidecar，完整扫描 321 毫秒，200 次复合查询 p95 3.346 毫秒，原始素材摘要不变。
- 阶段 1 M UI：60 秒预热后连续运行 30 分钟，2,025 次动作、0 错误；最大 JS 堆 57.6 MiB，最大 30 张卡片，事件循环延迟 p95 1 毫秒，Long Task 占比 0.1249%。
