# 开发进度

> 最后更新：2026-08-20

## 总体状态

| 阶段 | 状态 | 说明 |
|---|---|---|
| 阶段 0：技术验证与架构定案 | Accepted | P0-A01 至 P0-A08 全部通过 |
| 阶段 1：端到端 MVP | Accepted locally | P1-01 至 P1-09、P1-A01 至 P1-A12 全部通过；见阶段 1 验收报告 |
| 阶段 2：可靠性与恢复 | Accepted | P2-A01 至 P2-A12、外部/本地/数据安全门禁和最终历史重放全部通过；最终收据 SHA-256 `76b135b…` |
| 阶段 3：完整素材能力 | In progress | P3-01A/B 已本地完成；P3-01C 的严格 brand 识别、真实夹具和 codec 缺失降级已通过，下一动作是三平台 libheif worker |
| 阶段 4：Obsidian 深度集成 | Not started | 等待阶段 3 退出；P4-01 至 P4-08 的完整实施准备已收敛 |

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

> GitHub origin、SSH 和 GitHub CLI 已配置；`main` 已发布，正式托管 CI 运行 32332405466 的质量门禁和三平台矩阵全部通过。

## 阶段 2 工作项

| 工作项 | 状态 | 证据 |
|---|---|---|
| P2-01 统一文件事件模型 | Completed locally | 120 ms 静默/750 ms 最长批次、4,096 条溢出保护、临时文件折叠、单根自动/手动一致性扫描通过；见 `reports/p2-01-acceptance.md` |
| P2-02 移动与孤立文件处理 | Completed locally | 稳定 ID 成对移动保持、孤立/丢失/候选诊断、三级指纹、歧义选择与显式无覆盖 Sidecar 重联通过；见 `reports/p2-02-acceptance.md` |
| P2-03 批量操作事务与恢复 | Accepted | 全量预写计划、逐项原子提交、32 项检查点、摘要重建、继续/条件恢复及外部修改保护通过；候选提交 `581a661` 的 A04 真实进程收据确认 `317 -> 1000`；见 `reports/p2-03-acceptance.md` |
| P2-04 并发编辑与同步冲突 | Completed locally | mtime/大小/SHA-256 三元版本、Tag 显式三方合并、标量字段逐项选择、二次版本复核及同步冲突副本只读诊断通过；见 `reports/p2-04-acceptance.md` |
| P2-05 缓存生命周期 | Accepted | 20,000 项/1 GiB/30 天边界、LRU、精确失效和孤立回收通过；候选提交 `581a661` 的 A10 收据确认两个缓存崩溃点均安全恢复且用户文件不变；见 `reports/p2-05-acceptance.md` |
| P2-06 资源与稳定性控制 | Accepted | 绑定修复提交 `c0508c6` 的 100,000 素材完整 28,800 秒任务、确定性重放、敏感信息与零漂移审计均通过；见 `reports/p2-06-acceptance.md` |
| P2-07 诊断和支持工具 | Completed locally | 1 MiB × 5 JSONL 滚动日志、路径值脱敏、Schema 2 错误/性能导出、512 条有界只读一致性报告和 UUID 素材追踪通过；见 `reports/p2-07-acceptance.md` |
| P2-08 平台与文件系统兼容 | Accepted | 正式运行 32332405466 在同一 commit/run/attempt 上通过 macOS 10、Linux 12、Windows 9 项原生测试、矩阵汇总与完整质量门禁；四个 artifact 和托管回执已归档；见 `reports/p2-08-acceptance.md` |
| 阶段 2 退出验收 | Accepted | 数据安全收据绑定 `11db4c1`，最终退出收据绑定 `d4d78be`；提交顺序、历史字节、零漂移和 open P0/P1=0 均通过；见 `reports/phase-2-acceptance.md` |

## 阶段 3 工作项

| 准备项 | 状态 | 证据 |
|---|---|---|
| P3-01 扩展格式能力管线 | In progress | P3-01A/B Completed locally；P3-01C core-only 路径已具备严格 ISO BMFF brand 识别、真实 AVIF/HEIC 夹具和无缓存 codec 降级，libheif worker 尚未实现；P3-A01/A02 未判定，见 `reports/p3-01a-acceptance.md`、`reports/p3-01b-acceptance.md`、`reports/p3-01c-readiness.md` |
| P3-02 智能属性与高级过滤 | Design ready | 已固定定型谓词、整数单位、RFC 3339、精确宽高比、未知属性、混合索引和独立三方查询语料；阶段 2 退出前不计作 P3 开始，见 `reports/p3-02-readiness.md` |
| P3-03 保存过滤器 | Design ready | 已固定用户级 YAML、条目隔离、未知字段保留、完整版本、精确 Tag AST 重写和跨文件协调恢复；阶段 2 退出前不计作 P3 开始，见 `reports/p3-03-readiness.md` |
| P3-04 批量工作流 | Design ready | 已固定精确全选快照、catalog revision、只读预检、协作取消、事务续传与剪贴板单次提交；阶段 2 退出前不计作 P3 开始，见 `reports/p3-04-readiness.md` |
| P3-05 重复素材分析 | Design ready | 已固定大小/快速指纹/完整 SHA-256、当前文件重读、物理别名、视觉候选隔离和非破坏性报告；阶段 2 退出前不计作 P3 开始，见 `reports/p3-05-readiness.md` |
| P3-06 拖放与剪贴板 | Design ready | 已固定原始文件/引用双拖柄、后端授权 native drag、Copy-only、Vault 内外有序引用和最小剪贴板权限；阶段 2 退出前不计作 P3 开始，见 `reports/p3-06-readiness.md` |
| P3-07 生产级交互 | Design ready | 已固定后端排序、多密度虚拟网格、key-based 焦点、预览 session、命令面板、Tag 重命名、任务中心和 A10/A11 证据；阶段 2 退出前不计作 P3 开始，见 `reports/p3-07-readiness.md` |

## 阶段 4 实施准备

| 准备项 | 状态 | 证据 |
|---|---|---|
| P4-01/P4-02 权限与离线索引 | Architecture ready | 已固定机器级授权 manifest、插件逐根批准、Unix socket/Windows pipe、两阶段内存索引、小图 object URL 和大媒体 Range lease；阶段 3 退出前不计作 P4 开始，见 `reports/p4-01-02-readiness.md` |
| P4-03/P4-05/P4-06 搜索、定位与恢复 | Architecture ready | 已固定在线/离线统一选择器、目标 digest 防误写、单事务插入、临时 Markdown AST backlink、双向 handle 定位及无改写移动恢复；阶段 3 退出前不计作 P4 开始，见 `reports/p4-03-05-06-readiness.md` |
| P4-07/P4-08 兼容、降级与发布 | Architecture ready | 已固定 desktop-only、两版冻结矩阵、移动/Publish 源码保留降级、teardown、隐私披露、确定性插件源码导出和可复现发布门禁；阶段 3 退出前不计作 P4 开始，见 `reports/p4-07-08-readiness.md` |

## 已固定的关键决定

- 文件系统与相邻 Sidecar 是唯一真相源；
- Sidecar 使用 YAML，逻辑结构由 JSON Schema 约束；
- 稳定 ID 使用 UUIDv7；
- 桌面端采用 Tauri 2、Rust、React、TypeScript 与 Vite；
- JavaScript 工具链固定为 Node.js 24 LTS；
- 阶段 0 核心原型先以 Rust workspace 和 CLI 完成；
- Vault 内使用标准 Obsidian 引用，Vault 外使用 `material://<uuid>`；
- 默认不跟随符号链接，外部渲染只能访问显式授权根；
- Sidecar 正式桌面编辑按 mtime、文件大小和 SHA-256 完整版本执行乐观并发控制，成功持久化后才更新内存索引。
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
- 并发冲突计划只保存于有界运行期内存；Tag 重叠修改必须显式选择集合合并/外部/我的版本，备注等字段必须逐项选边，解决前再次复核完整 Sidecar 版本。
- Dropbox、Syncthing 和保守通用命名的 YAML 冲突副本只进入一致性诊断；只有稳定 ID 唯一匹配时比较用户字段，应用不自动删除、移动或合并任何副本。
- 缩略图采用 PNG 与同键 JSON 派生文件对，默认最多 20,000 项、1 GiB、保留 30 天；命中以 PNG mtime 更新 LRU，旧解码器、孤立和不完整项由启动/扫描后/手动维护回收。
- 全量缓存清理先写所有权标记并把固定根原子轮换为 UUIDv7 tombstone，再建立新根；启动只回收名称和标记均匹配的遗留目录。
- 扫描、哈希和解码共享单一运行期资源控制器；窗口失焦降低全局容量，许可等待、监听事件和扫描 UI 批次都具有硬上限，溢出依靠已授权根完整扫描收敛。
- 文件解析在不可分割 I/O 之间检查取消和 5 秒协作期限，超过 256 MiB 的素材跳过可选原生元数据，超过 4 MiB 的 Sidecar 跳过 YAML；该限制不隐藏素材，也不修改源文件。
- 运行支持事件同时进入 256 条内存缓冲和最多 5 × 1 MiB 的 JSONL 滚动日志；路径值落盘前指纹化，诊断导出聚合错误与性能，一致性/素材追踪只读且不接受任意路径。
- 根目录 canonicalize 后按平台逐组件判断重叠；Windows 路径键折叠大小写与 Unicode、macOS 只折叠 Unicode、Linux 保留原样。符号链接只诊断不跟随，扫描中掉线或撤权时放弃本次权威结果并恢复扫描前目录记录。
- 扩展格式按识别、属性和预览三层能力解耦；codec 缺失只降级为通用类型卡片，不得隐藏素材。复杂或原生解码器使用固定 worker 和硬超时，文件派生媒体属性不写入 Sidecar。
- 高级查询先解析为定型谓词；大小、时长与时间使用整数，宽高比用分数交叉比较，未知属性只能由显式 `unknown` 匹配。P3-A03 的离线 oracle 不得复用产品解析器或索引。
- “全选当前结果”在后端物化精确有序快照，不能在执行时重新求值而纳入新素材；元数据批次沿用纯文件事务，取消后已提交项保持完整、未开始项可继续或条件恢复。
- 重复分析只在运行期执行大小、快速指纹和当前文件完整 SHA-256 分层确认；硬链接别名、独立字节副本和视觉候选分开报告，不提供自动删除、合并或引用改写。
- 原始文件拖出与引用拖放使用显式双模式；后端从精确选择快照解析并复核路径，本地 native adapter 固定 Copy 且不暴露任意路径 IPC，剪贴板只授予文本/HTML 写入。
- 生产 UI 的查询与排序由 Rust catalog 唯一执行；虚拟网格按素材 key/revision 维护焦点与选择，三种密度、预览、命令面板和任务状态均保持可访问且不保存素材快照。
- Obsidian 授权根由桌面端机器级 manifest 发布并由插件逐根批准；在线控制使用当前用户 socket/pipe，离线只建内存索引，小图 object URL 与大媒体短期 Range lease 每次都复核 UUID、realpath 和授权。

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
- [P2-04 并发编辑与同步冲突验收报告](reports/p2-04-acceptance.md)
- [P2-05 缓存生命周期验收报告](reports/p2-05-acceptance.md)
- [P2-06 资源与稳定性验收准备报告](reports/p2-06-acceptance.md)
- [P2-07 诊断和支持工具验收报告](reports/p2-07-acceptance.md)
- [P2-08 平台与文件系统兼容验收报告](reports/p2-08-acceptance.md)
- [阶段 2 验收报告](reports/phase-2-acceptance.md)
- [阶段 2 数据安全缺陷审计](reports/p2-data-safety-audit.md)
- [P3-01 扩展格式支持实施准备报告](reports/p3-01-readiness.md)
- [P3-01A 注册表与通用卡片验收报告](reports/p3-01a-acceptance.md)
- [P3-01B SVG 验收报告](reports/p3-01b-acceptance.md)
- [P3-01C AVIF/HEIC 就绪报告](reports/p3-01c-readiness.md)
- [P3-02 智能属性与高级过滤实施准备报告](reports/p3-02-readiness.md)
- [P3-03 保存过滤器实施准备报告](reports/p3-03-readiness.md)
- [P3-04 批量工作流实施准备报告](reports/p3-04-readiness.md)
- [P3-05 重复素材分析实施准备报告](reports/p3-05-readiness.md)
- [P3-06 拖放与剪贴板实施准备报告](reports/p3-06-readiness.md)
- [P3-07 生产级交互实施准备报告](reports/p3-07-readiness.md)
- [P4-01/P4-02 插件权限与离线索引实施准备报告](reports/p4-01-02-readiness.md)
- [P4-03/P4-05/P4-06 搜索插入、双向定位与恢复实施准备报告](reports/p4-03-05-06-readiness.md)
- [P4-07/P4-08 兼容、降级与发布实施准备报告](reports/p4-07-08-readiness.md)
- [平台差异记录](../specs/platform-notes.md)
- 参考环境：Apple M4、16 GiB、APFS SSD、macOS 26.5.2；
- L 数据集：100,000 个素材、20,000 个 sidecar；
- 完整扫描：1.454 秒；
- 查询 p95：19.082 毫秒；
- 峰值常驻内存：202,473,472 字节，约 193 MiB。
- P1-05 正式复合查询（L 数据集、200 次）p95：53.691 毫秒。
- 阶段 1 M 数据集：10,000 素材、2,000 Sidecar，完整扫描 321 毫秒，200 次复合查询 p95 3.346 毫秒，原始素材摘要不变。
- 阶段 1 M UI：60 秒预热后连续运行 30 分钟，2,025 次动作、0 错误；最大 JS 堆 57.6 MiB，最大 30 张卡片，事件循环延迟 p95 1 毫秒，Long Task 占比 0.1249%。
