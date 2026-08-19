# 文件系统素材管理器

一个以文件系统为唯一真相源的桌面素材管理项目。应用负责扫描、解释、预览、Tag 和引用文件，不使用权威数据库，不自动重构或修改原始素材。

## 当前状态

阶段 0 与阶段 1 已通过本地验收。阶段 2 已完成 P2-01 至 P2-05 和 P2-07；P2-06 的资源稳定性实现等待 P2-A11 连续 8 小时正式验收，P2-08 的平台兼容与离线根降级已通过 macOS 本机验收，等待 Windows/Linux 托管矩阵完成 P2-A12。仓库尚无远程地址，首次托管 CI 仍待远程建立后确认。开发阶段、交付物和验收规范见：

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
- [P2-06 验收准备报告](docs/reports/p2-06-acceptance.md)
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
npm ci --prefix apps/desktop
npm ci --prefix integrations/obsidian-bridge
npm run ci
npm run test:medium
npm run test:stability
npm run test:resource-stability
```

`npm run ci` 依次执行格式检查、Rust/TypeScript 静态检查、全部单元测试、S 数据集跨模块测试、Tauri 桌面端构建和 Obsidian 插件构建。`npm run test:medium` 在受标记保护的临时目录生成 10,000 项 M 夹具，验证扫描、复合查询和原始素材摘要后安全清理，用于阶段/每日规模回归，不放入每次提交的快速 CI。`npm run test:stability` 使用独立 Chrome 对同规模界面预热 60 秒并连续滚动、筛选 30 分钟，自动拒绝堆增长、主线程阻塞、全量卡片挂载、对象 URL 越界、错误结果、页面重载和采集超时；默认需要 macOS Google Chrome，可用 `-- --chrome <path>` 覆盖浏览器路径。`npm run test:resource-stability` 默认生成 L 数据集并运行 8 小时原生负载，采集 RSS、CPU、线程、句柄、调度与缓存曲线；它是 P2-A11 的阶段验收，不属于每次提交的快速 CI。

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
