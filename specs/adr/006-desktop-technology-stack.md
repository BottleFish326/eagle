# ADR-006：桌面端技术栈

- 状态：Accepted
- 日期：2026-08-14

## 背景

项目需要跨平台文件系统扫描、监听、哈希、媒体解码和桌面 UI，并需要与 TypeScript 编写的 Obsidian 插件共享 Schema 与协议。

## 决策

1. 桌面壳使用 Tauri 2。
2. 核心文件系统、元数据、索引和预览模块使用稳定版 Rust。
3. 前端使用 React、TypeScript 和 Vite。
4. JavaScript 包管理器使用 npm，避免阶段 0 增加额外全局工具依赖。
5. Obsidian 插件使用 TypeScript 和 esbuild。
6. Rust 版本通过仓库内 `rust-toolchain.toml` 固定到稳定通道并包含 rustfmt、clippy。
7. 阶段 0 的核心原型首先作为 Rust workspace 和 CLI 实现；阶段 1 再接入 Tauri UI。
8. 核心逻辑不得依赖 Tauri API，保证命令行、测试和未来其他前端可复用。

## 后果

- 扫描、哈希和监听可以使用 Rust 的强类型与并发能力；
- UI 和 Obsidian 插件共享 TypeScript 类型需要通过 Schema 生成或手工同步测试保证；
- 开发环境必须安装 Rust、Node.js 和各平台的 Tauri 系统依赖；
- 需要分别维护 Rust 与 npm 依赖安全更新。

## 依据

- [Tauri 2 前置条件](https://v2.tauri.app/start/prerequisites/)
- [Tauri 2 安全模型](https://v2.tauri.app/security/)
- [Rust 官方安装方式](https://www.rust-lang.org/tools/install)
