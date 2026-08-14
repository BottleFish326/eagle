# 文件系统素材管理器

一个以文件系统为唯一真相源的桌面素材管理项目。应用负责扫描、解释、预览、Tag 和引用文件，不使用权威数据库，不自动重构或修改原始素材。

## 当前状态

阶段 0 已通过全部技术与 Obsidian 实机验收。阶段 1 已启动，P1-01 至 P1-07 已完成本地验收，当前具备工程骨架、素材根管理、可取消增量扫描、具备并发保护的 Sidecar 批量编辑、布尔 Tag 查询、按视口请求的版本化缩略图管线，以及可操作的扁平素材桌面界面。开发阶段、交付物和验收规范见：

- [开发计划与验收规范](docs/development-plan.md)
- [开发进度](docs/progress.md)
- [阶段 0 验收报告](docs/reports/phase-0-acceptance.md)
- [P1-01 验收报告](docs/reports/p1-01-acceptance.md)
- [P1-02 验收报告](docs/reports/p1-02-acceptance.md)
- [P1-03 验收报告](docs/reports/p1-03-acceptance.md)
- [P1-04 验收报告](docs/reports/p1-04-acceptance.md)
- [P1-05 验收报告](docs/reports/p1-05-acceptance.md)
- [P1-06 验收报告](docs/reports/p1-06-acceptance.md)
- [P1-07 验收报告](docs/reports/p1-07-acceptance.md)
- [桌面端 MVP 操作说明](docs/ui-operation-guide.md)
- [素材查询语言](specs/query-language.md)
- [缩略图协议](specs/thumbnail-protocol.md)
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
```

`npm run ci` 依次执行格式检查、Rust/TypeScript 静态检查、全部单元测试、S 数据集跨模块测试、Tauri 桌面端构建和 Obsidian 插件构建。

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
