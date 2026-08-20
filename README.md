# 文件系统素材管理器

一个以文件系统为唯一真相源的桌面素材管理项目。应用负责扫描、解释、预览、Tag 和引用文件，不使用权威数据库，不自动重构或修改原始素材。

## 当前状态

阶段 0 与阶段 1 已通过本地验收。阶段 2 的 P2-01 至 P2-08、P2-A11 100,000 素材连续 8 小时正式验收、P2-A12 GitHub-hosted macOS/Linux/Windows 原生矩阵、统一外部门禁和本地故障门禁均已通过；当前正在完成缺陷 reviewed、数据安全和阶段 2 最终退出收据。SSH origin、`main` 与托管 CI 已就绪。开发阶段、交付物和验收规范见：

- [开发计划与验收规范](docs/development-plan.md)
- [开发进度](docs/progress.md)
- [阶段 0 验收报告](docs/reports/phase-0-acceptance.md)
- [阶段 1 性能与稳定性报告](docs/reports/phase-1-performance.md)
- [阶段 1 验收报告](docs/reports/phase-1-acceptance.md)
- [P2-01 验收报告](docs/reports/p2-01-acceptance.md)
- [P2-02 验收报告](docs/reports/p2-02-acceptance.md)
- [P2-03 验收报告](docs/reports/p2-03-acceptance.md)
- [P2-04 验收报告](docs/reports/p2-04-acceptance.md)
- [P2-05 验收报告](docs/reports/p2-05-acceptance.md)
- [P2-06 验收报告](docs/reports/p2-06-acceptance.md)
- [P2-07 验收报告](docs/reports/p2-07-acceptance.md)
- [P2-08 验收报告](docs/reports/p2-08-acceptance.md)
- [阶段 2 验收报告（草案）](docs/reports/phase-2-acceptance.md)
- [P3-01 扩展格式实施准备](docs/reports/p3-01-readiness.md)
- [P1-01 验收报告](docs/reports/p1-01-acceptance.md)
- [P1-02 验收报告](docs/reports/p1-02-acceptance.md)
- [P1-03 验收报告](docs/reports/p1-03-acceptance.md)
- [P1-04 验收报告](docs/reports/p1-04-acceptance.md)
- [P1-05 验收报告](docs/reports/p1-05-acceptance.md)
- [P1-06 验收报告](docs/reports/p1-06-acceptance.md)
- [P1-07 验收报告](docs/reports/p1-07-acceptance.md)
- [P1-08 验收报告](docs/reports/p1-08-acceptance.md)
- [P1-09 验收报告](docs/reports/p1-09-acceptance.md)
- [桌面端 MVP 操作说明](docs/ui-operation-guide.md)
- [用户数据、缓存与恢复说明](docs/user-data-and-recovery.md)
- [素材查询语言](specs/query-language.md)
- [缩略图协议](specs/thumbnail-protocol.md)
- [Obsidian Vault 内引用协议](specs/obsidian-vault-reference-protocol.md)
- [应用配置与恢复协议](specs/application-recovery-protocol.md)
- [文件事件与一致性扫描协议](specs/filesystem-event-protocol.md)
- [扩展格式识别、属性与预览协议](specs/extended-format-protocol.md)
- [扩展格式夹具清单规范](specs/format-fixture-manifest.md)
- [架构决策记录](specs/adr/README.md)

## 核心原则

- 原始素材保持原位置、原名称和原内容；
- 用户元数据存储为相邻 sidecar 文件；
- 索引和缩略图均为可删除、可重建的派生缓存；
- 物理目录结构保持不变，应用提供扁平素材视图；
- Vault 内素材使用标准 Obsidian 引用；
- Vault 外素材通过受限的稳定 ID 协议与 Obsidian 联动。

## 仓库结构

```text
apps/desktop/                    桌面应用
crates/                          Rust 核心模块
integrations/obsidian-bridge/    Obsidian 插件
schemas/                         Sidecar 与配置 Schema
fixtures/                        固定测试素材
specs/                           架构决策与协议
tests/                           集成和端到端测试
docs/                            项目文档
```

目录随开发阶段按需创建，不提交无实际内容的空目录。桌面端当前采用 Tauri 2、Rust、React、TypeScript 与 Vite。

## Git 工作流

- `main` 始终保持可构建、可测试；
- 功能分支使用 `feature/<topic>`；
- 修复分支使用 `fix/<topic>`；
- 文档分支使用 `docs/<topic>`；
- 提交信息遵循 Conventional Commits；
- Sidecar Schema、文件写入规则和 Obsidian 协议变更必须附带 ADR。

完整协作规则见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 本地质量门禁

Node.js 版本由 `.nvmrc` 固定为 24 LTS。安装各 JavaScript 工作区依赖后，在仓库根目录运行：

```bash
npm ci
npm ci --prefix apps/desktop
npm ci --prefix integrations/obsidian-bridge
npm run ci
npm run test:schemas
npm run test:medium
npm run test:stability
npm run test:resource-stability
npm run test:p2-local-fault-gates
npm run audit:p2-soak-baseline
npm run audit:p2-hosted-readiness
npm run status:p2-exit
npm run verify:p2-external
npm run inspect:p2-external
npm run verify:p2-data-safety
npm run inspect:p2-data-safety
npm run verify:p2-exit
npm run inspect:p2-exit
```

`npm run ci` 依次执行格式检查、Rust/TypeScript 静态检查、全部单元测试、S 数据集跨模块测试、Tauri 桌面端构建和 Obsidian 插件构建。根级 `npm ci` 安装锁定的 JSON Schema 验证依赖；`npm run test:schemas` 使用严格 Draft 2020-12 模式编译仓库全部 Schema，验证缺陷登记、P2 平台/托管源收据和四类阶段收据，并确认重复平台、缺失原生测试、job、归档文件、控制项及关键数值边界都会被拒绝；该测试已包含在 `npm run test:tools` 与托管 CI 中。`npm run test:medium` 在受标记保护的临时目录生成 10,000 项 M 夹具，验证扫描、复合查询和原始素材摘要后安全清理，用于阶段/每日规模回归，不放入每次提交的快速 CI。`npm run test:stability` 使用独立 Chrome 对同规模界面预热 60 秒并连续滚动、筛选 30 分钟，自动拒绝堆增长、主线程阻塞、全量卡片挂载、对象 URL 越界、错误结果、页面重载和采集超时；默认需要 macOS Google Chrome，可用 `-- --chrome <path>` 覆盖浏览器路径。`npm run test:resource-stability` 默认生成 L 数据集并运行 8 小时原生负载，采集 RSS、CPU、线程、句柄、调度与缓存曲线；它是 P2-A11 的阶段验收，不属于每次提交的快速 CI。`npm run test:p2-local-fault-gates` 只用于阶段 2 干净候选提交：它构建并哈希两个 Rust 故障工具，真实重复 P2-A04 的 317/1000 中断续传和 P2-A10 的两个缓存 abort/recover 边界，安全清理隔离目录后发布不可覆盖的机器收据；不能在资源稳定性采样期间运行。`npm run audit:p2-soak-baseline` 只读核对正式长稳任务固定的受测 SHA、已加载脚本和产品输入，用于最终证据提交前排除源码漂移。`npm run audit:p2-hosted-readiness` 只读检查 P2-A12 的 GitHub CLI、origin/默认分支、已发布 commit 和手动工作流入口；未就绪时按顺序输出安装、认证、配置 origin、切换/发布 main 等建议动作，就绪后才生成绑定该 commit/run attempt 的触发与证据下载命令。预检不会自动执行任何建议。

`npm run status:p2-exit` 是只读阶段编排器：它检查 Git 状态、A11 final/partial、A12 原始归档与 hosted 收据、外部汇总、本地故障收据、数据安全审计和最终退出收据，必要时从原始字节重放外部门禁。数据安全阶段还会区分登记册 missing/draft/reviewed/invalid，核对审核时间、当前提交中的逐字登记和九份报告，再依次给出审核、提交或生成收据动作。命令不会构建、推送、触发 workflow 或写证据；输出唯一当前阶段和下一动作，只有发现矛盾、失败或损坏的不可覆盖证据时才返回非零。

P2-A12 的托管运行成功后，执行 `npm run collect:p2-hosted-evidence -- --run-id <run-id> --attempt <attempt>`。该命令拒绝模糊的“最新运行”，复核指定运行的状态、事件、workflow、分支、commit、URL 和五个必需 job，随后精确下载、离线重放并归档四份机器证据，同时发布不可覆盖的托管运行收据供阶段退出复验。

A11 与 A12 原始证据均就位后，`npm run verify:p2-external` 会确定性重放两项门禁并生成不可覆盖的统一收据；`npm run inspect:p2-external` 可在不读取原始证据的情况下严格复核收据结构、正式资源边界、三平台测试全集、GitHub hosted 上下文、artifact/job 绑定与确定性时间。离线检查用于归档审阅，不能代替原始重放。

外部门禁与本地候选故障收据都提交后，必须复核 [`docs/defects.json`](docs/defects.json) 中的全部发现，把审核状态和时间更新为真实值，再运行 `npm run verify:p2-data-safety`。该门禁拒绝 draft 登记、open P0/P1、无规避/目标版本的 open P2、过早审核、未提交输入或九类报告漂移；accepted 收据可用 `npm run inspect:p2-data-safety` 离线检查。

所有阶段 2 正式证据、本地故障收据和数据安全收据提交后，从完全干净的候选提交运行 `npm run verify:p2-exit`。该命令再次从 A11 原始样本与 A12 四份 artifact 重放外部门禁，严格检查 A04/A10 收据，并从数据安全收据绑定的历史提交重放缺陷登记与九份控制报告；同时复核 `soak ≤ hosted matrix ≤ local faults ≤ data safety ≤ candidate`、soak 源码零漂移、证据在相应提交中逐字存在，以及本地故障执行后没有 README/docs 之外的变化。只有全部满足并通过独立收据自检才生成不可覆盖的 `p2-phase-2-exit.json`；之后可用 `npm run inspect:p2-exit` 单独离线复核其完整结构与内部摘要一致性。

## 阶段 0 原型单独验证

Rust 核心原型：

```bash
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p fixture-generator -- generate /tmp/eagle-fixture --scale small
cargo run --release -p eagle-p0 -- scan /tmp/eagle-fixture
cargo run --release -p fixture-generator -- clean /tmp/eagle-fixture
```

Obsidian 插件原型：

```bash
cd integrations/obsidian-bridge
npm ci
npm run check
npm test
npm run build
```
