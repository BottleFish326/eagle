# 开发进度

> 最后更新：2026-08-14

## 总体状态

| 阶段 | 状态 | 说明 |
|---|---|---|
| 阶段 0：技术验证与架构定案 | Accepted | P0-A01 至 P0-A08 全部通过 |
| 阶段 1：端到端 MVP | In progress | P1-01 已完成本地验收，准备进入 P1-02 |
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
| P0-05 Sidecar 写入与冲突原型 | Completed | 三个进程崩溃点保持完整，CLI 提供 abort/reload/merge 冲突策略 |
| P0-06 文件监听原型 | Completed | 创建/移动/删除风暴产生 68,362 个事件，最终扫描准确收敛到 5,000 个素材 |
| P0-07 Obsidian 联动原型 | Completed | Node 24 LTS 自动化通过；Obsidian 1.12.7 内外部引用实机渲染通过 |
| 阶段 0 验收报告 | Accepted | P0-A01 至 P0-A08 全部通过，截图证据已归档 |

## 阶段 1 工作项

| 工作项 | 状态 | 证据 |
|---|---|---|
| P1-01 项目骨架与持续集成 | Completed locally | 桌面端 Release 构建、质量门禁和 S 数据集测试通过；见 `reports/p1-01-acceptance.md` |
| P1-02 素材库根目录管理 | Ready | 下一开发项 |
| P1-03 至 P1-09 | Not started | 按开发计划顺序推进 |

> 当前仓库未配置 Git 远程地址，因此 GitHub Actions 的首次托管运行仍待远程仓库建立后确认；工作流所执行的同一组命令已在本机通过。

## 已固定的关键决定

- 文件系统与相邻 Sidecar 是唯一真相源；
- Sidecar 使用 YAML，逻辑结构由 JSON Schema 约束；
- 稳定 ID 使用 UUIDv7；
- 桌面端采用 Tauri 2、Rust、React、TypeScript 与 Vite；
- JavaScript 工具链固定为 Node.js 24 LTS；
- 阶段 0 核心原型先以 Rust workspace 和 CLI 完成；
- Vault 内使用标准 Obsidian 引用，Vault 外使用 `material://<uuid>`；
- 默认不跟随符号链接，外部渲染只能访问显式授权根。

## 当前性能证据

- [阶段 0 性能报告](reports/phase-0-performance.md)
- [阶段 0 验收报告](reports/phase-0-acceptance.md)
- [平台差异记录](../specs/platform-notes.md)
- 参考环境：Apple M4、16 GiB、APFS SSD、macOS 26.5.2；
- L 数据集：100,000 个素材、20,000 个 sidecar；
- 完整扫描：1.454 秒；
- 查询 p95：19.082 毫秒；
- 峰值常驻内存：202,473,472 字节，约 193 MiB。
