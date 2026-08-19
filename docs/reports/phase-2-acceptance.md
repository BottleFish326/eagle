# 阶段 2 验收报告

> 状态：Draft — Not accepted；P2-A11 running，P2-A12 pending
>
> 日期：2026-08-20
>
> P2-A11 受测基线：`c18e1cae6a2ca40805dfd39fdc8406f1f95ffd21`；后续提交截至本报告只增加不被运行任务加载的只读 checkpoint inspector 与文档

## 1. 验收范围

阶段 2 要求应用面对外部移动、监听丢失、批量中断、同步冲突、权限/移动盘变化和长期资源压力后，仍能从文件系统与相邻 Sidecar 收敛。阶段退出还要求 Windows、macOS、Linux 的原生路径测试全部通过。

本报告提前汇总已完成证据并固定最后两项门禁的判定流程。它不是阶段通过声明；P2-A11 和 P2-A12 任一未完成时，阶段 2 必须保持 **In progress**，阶段 3 不得开始实现。

## 2. 工作项结果

| 工作项 | 当前结果 | 主要证据 |
|---|---|---|
| P2-01 文件事件模型 | Pass locally | [P2-01 报告](p2-01-acceptance.md)：有界归一化、溢出重扫、最终一致性 |
| P2-02 移动与孤立处理 | Pass locally | [P2-02 报告](p2-02-acceptance.md)：稳定 ID、三级指纹、显式无覆盖重联 |
| P2-03 批量事务恢复 | Pass locally | [P2-03 报告](p2-03-acceptance.md)：1,000 项真实中断、继续、条件恢复 |
| P2-04 并发与同步冲突 | Pass locally | [P2-04 报告](p2-04-acceptance.md)：完整版本、字段选择、冲突副本只读诊断 |
| P2-05 缓存生命周期 | Pass locally | [P2-05 报告](p2-05-acceptance.md)：容量/期限/版本边界与双崩溃点恢复 |
| P2-06 资源稳定性 | Acceptance running | [P2-06 报告](p2-06-acceptance.md)：实现/smoke 已通过；正式 8 小时任务进行中 |
| P2-07 诊断支持 | Pass locally | [P2-07 报告](p2-07-acceptance.md)：滚动日志、脱敏导出、一致性与 ID 追踪 |
| P2-08 平台兼容 | Acceptance pending | [P2-08 报告](p2-08-acceptance.md)：macOS 原生通过；Windows/Linux 托管运行待远程 |

## 3. 验收用例

| ID | 状态 | 结果与证据 |
|---|---|---|
| P2-A01 | Pass locally | 素材与 Sidecar 成对移动后按唯一 ID 生成移动映射，Tag 与选择迁移，无重复素材 |
| P2-A02 | Pass locally | 单边移动产生 orphan/missing/三级候选；扫描和诊断不写磁盘 |
| P2-A03 | Pass locally | 两个完整 SHA-256 相同候选均保持 ambiguous，用户未选择前不操作 |
| P2-A04 | Pass locally | 1,000 项批次在第 317 项后真实 abort，重启重建状态并继续到 1,000/1,000 |
| P2-A05 | Pass locally | 事务后外部改写的 Sidecar 保持原字节并成为 conflict，其余项条件恢复 |
| P2-A06 | Pass locally | 丢失/溢出触发根范围完整扫描，增量模型最终由完整文件系统结果替换 |
| P2-A07 | Pass locally | Dropbox/Syncthing/通用冲突副本与原件均保留，不自动删除或合并标量字段 |
| P2-A08 | Pass locally | 扫描中拔盘/撤权返回非权威 root-offline/permission-denied，应用与其他根保持运行 |
| P2-A09 | Pass locally | 重叠根拒绝；符号链接循环不跟随、不重复计数并输出明确诊断 |
| P2-A10 | Pass locally | 缓存根轮换的两个进程中断点均可启动恢复，素材与 Sidecar SHA-256 不变 |
| P2-A11 | Running | 正式 28,800 秒 L 数据集任务运行中；只有最终 JSON 自动判定 accepted 才可通过 |
| P2-A12 | Pending | macOS ARM64 10 项通过；尚无 Windows NTFS/Linux 原生托管结果 |

## 4. P2-A11 正式证据状态

当前任务使用 Node.js 24.19.0、macOS ARM64、100,000 素材、60 秒预热、5 秒采样和 60 秒检查点，绑定提交：

```text
c18e1cae6a2ca40805dfd39fdc8406f1f95ffd21
```

开始时间：`2026-08-19T15:37:27.899Z`。运行期间只读 checkpoint 为：

```text
docs/reports/evidence/p2-06-resource-soak.json.partial
```

partial 只证明任务仍可审计，不能提交、不能当作通过证据。当前 `inspect-resource-stability-checkpoint.mjs` 返回 healthy/no failures；该结论也不能替代 8 小时 complete sample。

### 4.1 唯一通过判据

进程正常结束后必须同时满足：

1. 生成 `evidence/p2-06-resource-soak.json`，`.partial` 被正常清除；
2. `accepted === true` 且 `failures` 为空；
3. `durationSeconds === 28800`、`fixtureCount === 100000`、Node major 为 24、git commit 与上文一致；
4. exit code 0、无 signal，最后内部 sample 是 `complete` 且 elapsed 不短于 28,800,000 ms；
5. internal/native 合法样本数均达到理论采样数 75%，时间轴单调且覆盖完整时长；
6. RSS 增长 ≤ 256 MiB、斜率 ≤ 8 MiB/min；handle 增长 ≤ 64、range ≤ 128；线程峰值 ≤ 预热基线 + 16；
7. CPU 不超调度容量包络，active/waiting/cache 始终不越界；
8. scan/event/hash/decode 和 background mode 全部实际出现；
9. 手工复核 summary 与样本后，才更新 P2-06 报告和本报告，提交最终 JSON。

任务非零退出、信号终止、机器睡眠造成采样不足或任一自动 failure 都必须保留失败证据并重新运行完整 8 小时；不得拼接两次运行或手工修改 JSON。

## 5. P2-A12 托管矩阵关闭流程

当前工作流 [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) 已定义 `ubuntu-24.04`、`macos-15`、`windows-2025` 三个独立 `platform-paths` leg，`fail-fast: false`，并定义依赖三者的 `platform-matrix-evidence` 汇总 job。阶段退出需要用户先建立 Git remote 并触发真实 hosted run，然后归档：

- remote URL、汇总 JSON 自动生成的 workflow `runUrl`、commit SHA、runner image/version 和时间；
- 三个 leg 均 success，并下载各自保留 90 天的 `p2-a12-source-<runner>-<sha>-attempt-<n>` JSON artifact；
- 汇总 job success，并下载同一 attempt 的 `p2-a12-matrix-<sha>-attempt-<n>` 中的 `p2-08-platform-matrix.json`；
- 三个 JSON 都是 `accepted=true`、`failures=[]`、同一 commit，expected/listed/executed 逐项一致；
- macOS/Linux/Windows 的精确通过数分别为 10/12/9，0 failed/ignored/measured；
- Windows 明确创建强制符号链接，实际执行 260+ UTF-16 路径扫描和 Sidecar 原子替换，无 skip；
- Linux 实际执行大小写同名并存、权限撤销和扫描中移动根目录，无 skip；
- macOS 继续执行 Unicode、循环和离线状态回归；
- job 日志/摘要可审计，不能用 cross-compile、条件编译或本机模拟替代。

如果某个 leg 因 runner 权限无法创建 Windows symlink，结果是 infrastructure failure，不是 P2-A12 pass；必须调整受控 runner/策略并重新完整执行。

若需要复验，必须重新运行全部 job。只重跑失败 job 会让成功 leg 保持旧 run attempt；汇总器不会跨 attempt 拼接证据，并会以缺平台拒绝该次结论。

逐平台证据由 `tools/verify-platform-paths.mjs` 生成；最终结论由 `tools/verify-platform-matrix.mjs` 对三个 JSON 的原始输出、摘要、SHA-256、commit、run/attempt、workflow 和 hosted 身份重新核对后生成。两层纯分析测试与本机真实 macOS 10 项输出已经通过，只证明工具链可执行；正式分析会拒绝非 GitHub-hosted 环境，因此本机不能生成 accepted matrix artifact。

下载四个 artifact 后运行 `node tools/archive-platform-matrix-evidence.mjs --input-directory <downloaded-artifacts>`。归档器会再次重放、确认受测 commit 是当前 HEAD 祖先，并把原始字节目录级原子保存到 `docs/reports/evidence/p2-a12-platform-evidence/`；不得用手工复制结果替代该步骤。

## 6. 产品不变量复核

| 不变量 | 当前结论 |
|---|---|
| 文件系统与 Sidecar 是唯一素材真相源 | Pass locally；事件、事务、冲突、缓存和诊断状态均可重建或显式配置 |
| 不自动重构/删除原始素材 | Pass locally；移动重联要求后端候选 handle 与用户确认，重复分析尚未进入本阶段 |
| 外部变化最终由完整扫描收敛 | Pass locally；取消/失败恢复前态，成功批次原子替换根目录结果 |
| Sidecar 不被截断/静默覆盖 | Pass locally；原子写、完整版本、事务摘要与真实中断覆盖 |
| 队列、缓存和诊断有界 | Implemented；短时通过，8 小时门禁仍 pending |
| 平台路径不产生错误合并/逃逸 | macOS pass；Windows/Linux runtime pending |

## 7. 退出条件

| 条件 | 当前状态 |
|---|---|
| P2-A01 至 P2-A12 全部通过 | Pending：A11 running，A12 pending |
| 完整扫描与增量模型最终一致 | Pass locally |
| 崩溃测试无截断 Sidecar | Pass locally |
| 连续 8 小时无无界资源增长 | Pending |
| 三平台核心路径原生测试通过 | Pending |
| P0/P1 数据安全缺陷为零 | Final audit pending；当前报告无已知未解决项 |

## 8. 阶段退出操作

只有第 4、5 节证据都通过后，执行一次候选提交审计：

1. 确认 P2-A11 final JSON、P2-A12 已归档 consolidated matrix/三个源 artifact/hosted logs 与实现 commit 可追溯；
2. 确认从 `c18e1ca` 起只有未被受测进程加载的只读 inspector/证据/文档变化；若有产品或验收器判定代码变化，重新跑相应门禁；
3. 检查 `git status` 只包含预期最终证据，没有 partial、临时 fixture、token 或本机路径；
4. 更新 P2-06/P2-08、本报告、`docs/progress.md` 和 README 为实际结论；
5. 运行文档/Schema 检查和最终快速质量门禁；
6. 提交阶段 2 退出证据，再把阶段 3 从 Not started 改为 In progress。

## 9. 当前结论

P2-A01 至 P2-A10 已有本地自动化/故障注入证据。P2-A11 正在执行，P2-A12 仍受没有 Git remote 的外部托管矩阵阻塞。因此阶段 2 当前结论是 **Not accepted / In progress**，不得提前宣称完成。
