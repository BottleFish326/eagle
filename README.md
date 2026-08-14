# 文件系统素材管理器

一个以文件系统为唯一真相源的桌面素材管理项目。应用负责扫描、解释、预览、Tag 和引用文件，不使用权威数据库，不自动重构或修改原始素材。

## 当前状态

项目处于架构与开发准备阶段。开发阶段、交付物和验收规范见：

- [开发计划与验收规范](docs/development-plan.md)
- [开发进度](docs/progress.md)
- [架构决策记录](specs/adr/README.md)

## 核心原则

- 原始素材保持原位置、原名称和原内容；
- 用户元数据存储为相邻 sidecar 文件；
- 索引和缩略图均为可删除、可重建的派生缓存；
- 物理目录结构保持不变，应用提供扁平素材视图；
- Vault 内素材使用标准 Obsidian 引用；
- Vault 外素材通过受限的稳定 ID 协议与 Obsidian 联动。

## 计划中的仓库结构

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

目录会随着阶段 0 和阶段 1 的技术决策逐步创建，不提前提交空目录。

## Git 工作流

- `main` 始终保持可构建、可测试；
- 功能分支使用 `feature/<topic>`；
- 修复分支使用 `fix/<topic>`；
- 文档分支使用 `docs/<topic>`；
- 提交信息遵循 Conventional Commits；
- Sidecar Schema、文件写入规则和 Obsidian 协议变更必须附带 ADR。

完整协作规则见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 阶段 0 本地验证

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
