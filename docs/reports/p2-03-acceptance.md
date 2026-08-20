# P2-03 批量操作事务与恢复验收报告

- 状态：Accepted；P2-A04 candidate fault gate passed
- 日期：2026-08-20
- 对应：P2-03、P2-A04、P2-A05
- 决策：[ADR-020](../../specs/adr/020-durable-batch-metadata-transactions.md)

## 1. 交付范围

本工作项完成以下链路：

- 两项以上的元数据编辑在首个 Sidecar 写入前生成完整 YAML 事务计划；
- 计划记录原始/计划内容与 SHA-256、计划/已应用/失败/恢复/冲突状态和素材根；
- Sidecar 继续独立执行乐观版本检查与同目录原子替换；
- 处理期间每 32 项持久化检查点，重启后用磁盘摘要重建检查点后的实际状态；
- 未完成事务可以继续，已应用项目可以按当前摘要条件恢复；
- 外部修改、素材指纹变化、根目录不可用、进行中扫描和授权根逃逸均安全停止；
- 设置页显示持久化事务，提供继续、安全恢复和二次确认仅删除日志；
- 启动时发现 `active` 或 `conflict` 事务会给出恢复提示，恢复后重新扫描受影响根；
- 独立故障工具支持在指定提交数后让真实进程 `abort`。

没有引入数据库、IndexedDB、LocalStorage 或权威批次表。事务日志是配置目录中的普通恢复文件；现有素材与相邻 Sidecar 仍是唯一真相源。

## 2. 持久化与状态重建

日志目录固定为应用配置目录下的 `metadata-transactions-v1`，命令不接收客户端目录。每个日志使用后端 UUIDv7 命名并通过临时文件、文件同步、原子持久化和目录同步提交。

事务先计划全部项目再开始写入。每项 Sidecar 写入后可能尚未进入最近的 32 项日志检查点；重启枚举日志时比较：

- 当前摘要等于计划摘要：`applied`；
- 当前摘要等于原摘要，或原本不存在且当前仍不存在：`planned`；
- 两者都不等：`conflict`。

因此恢复正确性不依赖进程最后来得及写入的计数。日志中的计数只是项目状态的汇总视图。

## 3. 继续与恢复安全边界

继续只处理仍为 `planned` 的项目，并重新检查素材指纹和原 Sidecar 版本。恢复只处理当前摘要仍等于计划摘要的 `applied` 项：原 Sidecar 存在时写回原始完整内容；事务新建 Sidecar 时按计划摘要条件删除。外部编辑永远不会被恢复操作覆盖。

桌面命令还会重新检查素材根存在、启用、可访问且没有扫描任务；Sidecar 必须是素材路径推导出的相邻文件，真实父目录和现有素材不能解析到授权根之外。前端只提交不透明事务 ID。

## 4. 自动化与故障验收

| 用例 | 证据 | 结果 |
|---|---|---|
| P2-A04 真实进程终止 | 专用二进制在第 317/1000 项 Sidecar 原子提交后、下个日志检查点前执行 `abort`，退出码 134 | Pass |
| P2-A04 重启发现 | 第二个进程枚举同一纯文件日志，状态为 `Active` 且重建 `applied=317` | Pass |
| P2-A04 安全继续 | 第二个进程继续到 1000/1000；逐个解析 Sidecar 并确认目标 Tag | Pass |
| P2-A04 单元回归 | 1000 项事务在 317 项边界中断、关闭 Store、重新打开并继续，全部通过 | Pass |
| P2-A05 外部编辑后恢复 | 三项批次后手工改写一个 Sidecar；恢复保留外部字节，第二个事务新建 Sidecar 按摘要删除，第三个已有 Sidecar 按原始字节精确恢复 | Pass |
| P2-A05 冲突可见 | 恢复摘要为 `Conflict`、`conflictCount=1`、`restoredCount=2`，失败项返回前端 | Pass |
| 批次预检 | 过期摘要仍进入日志并保留 `conflict` 类型；目标缺失时整个多项批次在写入前拒绝 | Pass |
| 命令边界 | 列表命令不接收目录；继续/恢复/删除只接收不透明 UUID；前后端 wire shape 测试通过 | Pass |
| 授权边界 | 继续/恢复前复核根状态、扫描互斥、推导 Sidecar 路径和真实路径范围 | Pass |
| UI 恢复入口 | 启动提示、状态计数、继续/恢复操作、二次确认删除日志及扫描刷新已接线 | Pass |
| 100 项性能基线 | Release 进程包含素材创建、全量计划日志和 100 个 Sidecar 原子写入，墙钟 1.39 秒 | Pass（目标 ≤ 3 秒） |

真实进程故障命令使用隔离临时目录，过程结果为：

```text
exit=134
discovered 01a0193e-069b-74b0-bf3f-dfdc355a69c4 Active applied=317
recovered 01a0193e-069b-74b0-bf3f-dfdc355a69c4 1000
```

UUID 仅用于本次隔离证据，不是固定测试断言。故障夹具执行后已移入系统废纸篓，不包含用户素材。

阶段 2 退出不再依赖这段人工终端摘录。统一候选门禁使用 `npm run test:p2-local-fault-gates`：它只接受 Node.js 24 和完全干净的 Git 工作树，Release 构建并记录 `transaction-fault`/`cache-fault` 二进制 SHA-256，在系统临时目录真实执行第 317 项 abort 与第二进程续传，确认 `317 -> 1000` 后删除本次精确创建的目录，再以不可覆盖方式生成 `evidence/p2-local-fault-gates.json`。收据结构由 [`p2-local-fault-gates.schema.json`](../../schemas/p2-local-fault-gates.schema.json) 固定；报告生成器的正向、异常退出、错误计数、错误 UUID、缺失故障点、来源与清理拒绝测试已通过，独立离线检查器还会拒绝字段、进程状态、摘要和故障点顺序篡改，执行器只会在该检查器再次接受报告后写盘。

正式候选门禁已从完全干净且包含外部门禁收据的提交 `581a6615c06e7f94d0771647e87d523f52c2b2ff` 执行。`p2-local-fault-gates.json` 记录 `SIGABRT`、发现 `applied=317`、恢复到 1,000、Release 二进制 SHA-256 和临时工作区已清理；独立检查器、JSON Schema、SHA-256 与敏感信息审计均通过。

## 5. 本地门禁

执行：

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
npm --prefix apps/desktop run format:check
npm --prefix apps/desktop run check
npm --prefix apps/desktop test
npm --prefix integrations/obsidian-bridge run check
npm --prefix integrations/obsidian-bridge test
node tools/verify-s-fixture.mjs
npm --prefix apps/desktop run tauri:build -- --no-bundle
npm --prefix integrations/obsidian-bridge run build
```

结果：全部通过。

- Rust：93 项测试，其中事务恢复 3 项、桌面事务边界/协议新增 3 项；
- 桌面 TypeScript：38 项测试；
- Obsidian Bridge：8 项测试；
- S 数据集：1,000 素材、200 Sidecar、999 个有效尺寸、1 个损坏图片隔离、0 个扫描问题，38 毫秒，素材摘要未变化；
- `cargo clippy --workspace --all-targets -- -D warnings`：通过；
- 真实进程 1,000 项故障注入与续传：通过；
- Release 100 项完整事务：1.39 秒，满足 3 秒基线；
- Tauri release 与 Obsidian Bridge production 构建成功。

## 6. 已知边界

- 当前事务入口覆盖两项以上的元数据补丁；批量复制、移动、导出等 P3 工作流后续复用该协调层；
- 批次语义是逐 Sidecar 原子、可恢复和可审计，不是假装跨 1000 个文件具有文件系统级全有或全无原子性；
- 完成日志默认保留到用户显式删除，会线性占用配置目录空间；P2-05 处理的是派生缩略图缓存，并未授权自动删除事务日志，未完成或冲突日志继续只能显式处理；
- 事务日志包含 Sidecar 原始与计划内容，可能含 Tag、备注和别名，不能按脱敏诊断文件分享；
- P2-04 已补齐普通 Sidecar 编辑的 Tag 三方集合合并与非集合字段逐项选择；批量事务日志仍显示项目状态/错误摘要，不保存第二套交互冲突计划；
- P2-A06 至 P2-A12、统一外部门禁和本地故障门禁均已通过；缺陷 reviewed、数据安全和最终退出收据仍在推进。

## 7. 结论

P2-03 已通过正式专项验收。批量元数据操作在写入前具有可读、原子持久化的纯文件计划；每个 Sidecar 保持独立原子性；候选提交上的真实进程中断能准确发现 317 个已完成项并继续到 1,000；条件恢复不会覆盖事务后的外部修改。A04/A10 统一故障收据与本报告已在提交 `be2f4f9` 中固化。
