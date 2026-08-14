# P1-01 项目骨架与持续集成验收报告

> 验收日期：2026-08-14
>
> 验收状态：本地通过（Hosted CI 首次运行待远程仓库配置）

## 1. 验收范围

本报告仅验收开发计划中的 P1-01：

- 建立桌面端、核心模块、Schema、测试和插件目录；
- 配置格式化、静态检查、单元测试和构建任务；
- CI 使用 S 数据集执行跨模块集成测试；
- 生成包含版本、提交号和构建环境的信息。

P1-02 至 P1-09 的产品功能不在本次验收范围内。

## 2. 交付内容

| 要求 | 实现 | 结果 |
|---|---|---|
| 桌面端骨架 | `apps/desktop`：Tauri 2 + Rust + React + TypeScript + Vite | 通过 |
| 核心模块 | 根 `Cargo.toml` 统一管理既有 Rust crates 与桌面端 crate | 通过 |
| Schema | `schemas` 保留 Sidecar 与素材库配置 Schema | 通过 |
| 测试 | Rust workspace、桌面 UI、Obsidian 插件及跨模块测试统一入口 | 通过 |
| 插件 | `integrations/obsidian-bridge` 纳入根质量门禁 | 通过 |
| 持续集成 | `.github/workflows/ci.yml` 在 Ubuntu 24.04、Node.js 24、Rust stable 上运行 `npm run ci` | 已配置 |
| 构建追踪 | 桌面端显示版本、Git 提交、构建目标、构建配置和 rustc 版本 | 通过 |

Tauri 的 `src-tauri/gen`、前端 `dist`、`node_modules`、TypeScript build info 与 Rust `target` 均作为可重建派生数据忽略，不进入 Git。

## 3. 统一质量门禁

仓库根 `package.json` 提供下列任务：

| 命令 | 验收内容 |
|---|---|
| `npm run format:check` | Rust 格式和桌面端 Prettier 格式 |
| `npm run check` | Rust Clippy、桌面端 TypeScript、Obsidian 插件 TypeScript |
| `npm test` | Rust workspace、桌面 UI、插件及 S 数据集跨模块测试 |
| `npm run build` | Tauri Release 可执行文件与 Obsidian 插件产物 |
| `npm run ci` | 串行执行上述全部门禁 |

CI 使用锁文件安装依赖，并设置只读仓库权限、并发取消和 45 分钟超时。Tauri Linux 系统依赖在工作流中显式安装。

## 4. S 数据集跨模块验收

`tools/verify-s-fixture.mjs` 每次在系统临时目录中创建一个全新的 S 数据集，调用 Rust 夹具生成器和扫描器，并验证：

- 生成 1,000 个素材和 200 个 sidecar；
- 扫描结果为 1,000 个素材且没有解析问题；
- 扫描前后抽样原始素材 SHA-256 完全一致；
- 夹具目录内不存在 `.db`、`.sqlite` 或 `.sqlite3` 文件；
- 测试结束后通过安全标记清理夹具和临时父目录。

本地结果：

```text
S dataset accepted: assets=1000 sidecars=200 problems=0 assetDigestUnchanged=true
```

## 5. 构建信息与桌面构建

Rust 构建脚本在编译期写入：

- 应用版本；
- Git 提交号（CI 中使用不可歧义的 `github.sha`）；
- Rust 编译目标；
- Debug/Release 构建配置；
- rustc 完整版本字符串。

前端通过只读 Tauri command 获取并展示这些信息；浏览器预览使用明确的 fallback 信息。后端与前端解析均有单元测试。

本机已完成 `tauri build --no-bundle`，并成功启动生成的 Release 可执行文件 `target/release/material-eagle-desktop`。P1-01 不要求签名安装包，因此本项只验收桌面可执行构建。

## 6. 验收环境

| 项目 | 值 |
|---|---|
| 操作系统 | macOS 26.5.2，Apple Silicon |
| Node.js | 24.19.0 |
| Rust | 1.97.1 stable |
| Tauri CLI | 2.11.4 |
| Tauri crate | 2.11.5 |

## 7. 结论与待办

P1-01 的目录、质量门禁、S 数据集跨模块验证、可追踪构建信息与桌面 Release 构建均已在本地通过，可以进入 P1-02。

当前 Git 仓库没有远程地址，故 GitHub Actions 尚无可供引用的托管运行记录。远程仓库配置后，首次 `main` push 或 pull request 必须确认 `CI / Format, test, and build` 通过；若托管结果失败，应重新打开 P1-01，不能以本地结果代替。
