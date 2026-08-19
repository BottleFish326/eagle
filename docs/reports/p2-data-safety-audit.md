# 阶段 2 数据安全缺陷审计

> 状态：审计链已实现；正式 reviewed 收据 pending
>
> 日期：2026-08-20

## 1. 目标

阶段 2 退出条件要求 P0/P1 数据安全缺陷为零。P0 指原始素材丢失或被修改、任意文件读取、不可恢复元数据损坏；P1 指核心链路不可用、静默错误引用或批量操作无法恢复。测试通过与“当前没有想到缺陷”不能替代显式缺陷审计。

本门禁以 [`docs/defects.json`](../defects.json) 为唯一阶段缺陷登记输入。仓库中的初始状态故意为 `draft`；即使 `findings=[]`，draft 也不能满足退出条件。只有 A11/A12 外部门禁与 A04/A10 候选崩溃收据均完成后，逐项复核自动化结果、验收报告、失败现场和已知限制，登记所有发现，再把状态改为 `reviewed` 并写入晚于全部输入证据的 UTC 时间。

## 2. 登记规则

- 每项缺陷使用唯一 `DEF-<四位以上数字>` ID，记录 P0 至 P3、open/resolved、摘要、复现或修复证据；
- open P0/P1 会无条件拒绝阶段退出；不得用 workaround、计划版本或文字豁免通过；
- open P2 必须同时记录可执行规避方式与目标版本；
- resolved 项必须有解决时间和证据，open 项不得伪填解决时间；
- 重复 ID、未知字段、无证据项、无效时间、超过 1,000 项或审核早于最终门禁证据均拒绝；
- [`defect-register.schema.json`](../../schemas/defect-register.schema.json) 固定存档结构；纯分析器还执行 Schema 难以表达的唯一性、时间顺序和优先级规则。

## 3. 十项数据安全控制

| ID | 控制 | 固定证据 |
|---|---|---|
| DS-01 | 原始素材不可变 | 阶段 1、P2-02、P2-05 |
| DS-02 | Sidecar 原子持久化与冲突保护 | 阶段 1、P2-03、P2-04 |
| DS-03 | 批量事务可恢复 | P2-03 与本地真实故障收据 |
| DS-04 | 外部/同步冲突保留 | P2-04 |
| DS-05 | 派生缓存所有权与崩溃恢复 | P2-05 与本地真实故障收据 |
| DS-06 | 文件事件收敛与离线隔离 | P2-01、P2-08 |
| DS-07 | 资源、队列与缓存有界 | P2-06 与 A11 正式证据 |
| DS-08 | 诊断只读且脱敏 | P2-07 |
| DS-09 | 授权路径与引用边界 | 阶段 1、P2-08 |
| DS-10 | 真实进程崩溃与 hosted 平台证据 | P2-03、P2-05、P2-06、P2-08及两份机器收据 |

审计收据固定记录阶段 1 与 P2-01 至 P2-08 九份报告的路径和 SHA-256，并绑定 `p2-external-gates.json`、`p2-local-fault-gates.json` 与登记册原始字节。报告摘要不能替代两份机器收据的独立严格检查。

## 4. 正式执行

在外部门禁、本地故障收据、最终报告更新和缺陷登记均已提交，且工作树完全干净后执行：

```text
npm run verify:p2-data-safety
npm run inspect:p2-data-safety
```

操作过程中先运行 `npm run status:p2-exit`。状态机会区分登记册 missing/draft/reviewed/invalid，验证审核不早于两份上游证据且不晚于包含它的 candidate commit，检查登记册与九份报告逐字存在于 HEAD，并分别给出 `review-defect-register`、`commit-data-safety-inputs`、`resolve-data-safety-findings` 或 `verify-data-safety`；不会在 draft 状态误导执行必然失败的正式生成命令。

生成器要求 Node.js 24、无参数、全部输入逐字存在于当前提交，并验证 `A11 commit <= A12 commit <= local fault commit <= data-safety candidate`。失败只输出拒绝原因，不写正式文件；成功报告先经过不读取原始输入的独立检查器，再以不可覆盖、相同输入可幂等复核的方式写入 `docs/reports/evidence/p2-data-safety-audit.json`。收据由 [`p2-data-safety-audit.schema.json`](../../schemas/p2-data-safety-audit.schema.json) 固定。

收据提交后，`verify:p2-exit` 不信任其中的摘要：它从收据绑定的历史 candidate commit 重新读取登记册、外部/本地收据和九份报告，再生成逐字相同的审计结论。这样既允许最终收据之后按既定范围更新文档，也不能用后改的文件伪造原审计。

## 5. 当前结论

登记册、两个 Schema、纯分析器、离线检查器、不可覆盖生成器、P2 状态转换和最终历史重放均已实现。正式 A11/A12 与本地候选故障证据尚未齐备，因此登记册保持 `draft`，不得生成 accepted 数据安全收据，也不得把“P0/P1 为零”标记为 Pass。
