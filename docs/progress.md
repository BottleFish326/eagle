# 开发进度

> 最后更新：2026-08-14

## 总体状态

| 阶段 | 状态 | 说明 |
|---|---|---|
| 阶段 0：技术验证与架构定案 | In progress | ADR 与 Schema 已建立，原型待实现 |
| 阶段 1：端到端 MVP | Not started | 等待阶段 0 退出 |
| 阶段 2：可靠性与恢复 | Not started | 等待阶段 1 退出 |
| 阶段 3：完整素材能力 | Not started | 等待阶段 2 退出 |
| 阶段 4：Obsidian 深度集成 | Not started | 等待阶段 3 退出 |

## 阶段 0 工作项

| 工作项 | 状态 | 证据 |
|---|---|---|
| P0-01 架构决策记录 | Completed | `specs/adr/001` 至 `008` |
| Sidecar 与 Library Schema | Completed | `schemas/*.schema.json` |
| P0-02 测试素材生成器 | Not started | — |
| P0-03 增量扫描原型 | Not started | — |
| P0-04 内存索引与查询原型 | Not started | — |
| P0-05 Sidecar 写入与冲突原型 | Not started | — |
| P0-06 文件监听原型 | Not started | — |
| P0-07 Obsidian 联动原型 | Not started | — |
| 阶段 0 验收报告 | Not started | — |

## 已固定的关键决定

- 文件系统与相邻 Sidecar 是唯一真相源；
- Sidecar 使用 YAML，逻辑结构由 JSON Schema 约束；
- 稳定 ID 使用 UUIDv7；
- 桌面端采用 Tauri 2、Rust、React、TypeScript 与 Vite；
- 阶段 0 核心原型先以 Rust workspace 和 CLI 完成；
- Vault 内使用标准 Obsidian 引用，Vault 外使用 `material://<uuid>`；
- 默认不跟随符号链接，外部渲染只能访问显式授权根。
