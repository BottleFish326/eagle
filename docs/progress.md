# 开发进度

> 最后更新：2026-08-14

## 总体状态

| 阶段 | 状态 | 说明 |
|---|---|---|
| 阶段 0：技术验证与架构定案 | In progress | 核心原型已实现，故障注入与 Obsidian 实机验收待完成 |
| 阶段 1：端到端 MVP | Not started | 等待阶段 0 退出 |
| 阶段 2：可靠性与恢复 | Not started | 等待阶段 1 退出 |
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
| P0-05 Sidecar 写入与冲突原型 | Implemented | 原子替换、摘要冲突和未知字段测试通过；进程故障注入待完成 |
| P0-06 文件监听原型 | Implemented | 外部创建事件烟测通过；10,000 文件事件风暴待完成 |
| P0-07 Obsidian 联动原型 | Implemented | 类型检查、6 项安全测试和构建通过；测试 Vault 实机渲染待完成 |
| 阶段 0 验收报告 | Not started | — |

## 已固定的关键决定

- 文件系统与相邻 Sidecar 是唯一真相源；
- Sidecar 使用 YAML，逻辑结构由 JSON Schema 约束；
- 稳定 ID 使用 UUIDv7；
- 桌面端采用 Tauri 2、Rust、React、TypeScript 与 Vite；
- 阶段 0 核心原型先以 Rust workspace 和 CLI 完成；
- Vault 内使用标准 Obsidian 引用，Vault 外使用 `material://<uuid>`；
- 默认不跟随符号链接，外部渲染只能访问显式授权根。

## 当前性能证据

- [阶段 0 性能报告](reports/phase-0-performance.md)
- 参考环境：Apple M4、16 GiB、APFS SSD、macOS 26.5.2；
- L 数据集：100,000 个素材、20,000 个 sidecar；
- 完整扫描：1.454 秒；
- 查询 p95：19.082 毫秒；
- 峰值常驻内存：202,473,472 字节，约 193 MiB。
