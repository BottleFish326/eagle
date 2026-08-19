# 阶段 2 验收报告

> 状态：Draft — Not accepted；P2-A11 running，P2-A12 pending
>
> 日期：2026-08-20
>
> P2-A11 受测基线：`c18e1cae6a2ca40805dfd39fdc8406f1f95ffd21`；正式任务加载的分析、检查点写入和运行器三份文件已保持和该提交逐字相同，后续 partial 检查、最终重放、平台证据、Schema 与文档均位于运行任务未加载的独立文件

## 1. 验收范围

阶段 2 要求应用面对外部移动、监听丢失、批量中断、同步冲突、权限/移动盘变化和长期资源压力后，仍能从文件系统与相邻 Sidecar 收敛。阶段退出还要求 Windows、macOS、Linux 的原生路径测试全部通过。

本报告提前汇总已完成证据并固定最后两项门禁的判定流程。它不是阶段通过声明；P2-A11 和 P2-A12 任一未完成时，阶段 2 必须保持 **In progress**，阶段 3 不得开始实现。

全过程可运行 `npm run status:p2-exit` 获取机器状态。该只读编排器同时检查 Git、A11 final/partial、A12 hosted readiness/原始归档、外部汇总、本地故障与最终收据；它会把状态归入长稳运行/失败、托管环境阻塞/可触发、外部汇总待生成/损坏、本地故障待运行/待提交、候选脏、最终待生成/待提交或 accepted，并只输出当前唯一下一动作。命令不会执行该动作，也不会构建、推送、触发 workflow 或写证据；只有矛盾/失败/损坏证据返回非零。

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
| P2-A04 | Pass locally | 1,000 项批次在第 317 项后真实 abort，重启重建状态并继续到 1,000/1,000；候选收据执行器已就绪 |
| P2-A05 | Pass locally | 事务后外部改写的 Sidecar 保持原字节并成为 conflict，其余项条件恢复 |
| P2-A06 | Pass locally | 丢失/溢出触发根范围完整扫描，增量模型最终由完整文件系统结果替换 |
| P2-A07 | Pass locally | Dropbox/Syncthing/通用冲突副本与原件均保留，不自动删除或合并标量字段 |
| P2-A08 | Pass locally | 扫描中拔盘/撤权返回非权威 root-offline/permission-denied，应用与其他根保持运行 |
| P2-A09 | Pass locally | 重叠根拒绝；符号链接循环不跟随、不重复计数并输出明确诊断 |
| P2-A10 | Pass locally | 缓存根轮换的两个进程中断点均可启动恢复，素材与 Sidecar SHA-256 不变；候选收据执行器已就绪 |
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

基线隔离使用以下只读门禁复核；非零退出或机器输出 `accepted=false` 都必须停止验收并解释差异：

```text
npm run audit:p2-soak-baseline
```

门禁固定受测 SHA，要求当前 HEAD 是其后代；第一组逐文件核对正式任务已经加载的分析器、检查点写入器和运行器，第二组核对 Rust/桌面产品源码、依赖锁定、夹具生成器与 soak 负载。两组都检查从基线到当前工作树的 tracked 差异和 scope 内未跟踪文件。截至本报告输出 `accepted=true` 且两个 `changedPaths` 均为空；后续每次阶段 2 证据提交后都必须重跑，不能仅凭允许路径白名单推断“没有产品变化”。

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
9. 使用 `inspectResourceStabilityReport` 从原始样本确定性重放，重放报告与最终 JSON 语义完全一致；随后再人工复核异常趋势，更新 P2-06 报告和本报告并提交最终 JSON。

任务非零退出、信号终止、机器睡眠造成采样不足或任一自动 failure 都必须保留失败证据并重新运行完整 8 小时；不得拼接两次运行或手工修改 JSON。

## 5. P2-A12 托管矩阵关闭流程

当前工作流 [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) 已定义 `ubuntu-24.04`、`macos-15`、`windows-2025` 三个独立 `platform-paths` leg，`fail-fast: false`，并定义依赖三者的 `platform-matrix-evidence` 汇总 job；`workflow_dispatch` 允许在工作流进入默认分支后显式运行。阶段退出需要用户先建立 GitHub origin、安装并认证 GitHub CLI、把候选 `main` 推送到 `origin/main`，然后执行：

```text
npm run audit:p2-hosted-readiness
```

预检不会安装软件、发起认证、创建仓库、修改 remote、推送或触发 workflow。它要求 CLI/认证/remote/默认分支/本地分支/upstream/远端 commit/tracked 清洁状态和手动入口全部匹配；`ready=false` 时按依赖顺序输出结构化 `remediations`，当前 Homebrew 环境会先给出 `brew install gh`，随后才是认证、用户选择 GitHub URL、发布 main 等动作。只有 `ready=true` 时才输出绑定当前 SHA 的 `gh workflow run`、`gh run list/watch` 与指定 run/attempt 的一体化证据采集命令，并按输出触发真实 hosted run，然后归档：

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

运行成功后使用预检报告最后给出的：

```text
npm run collect:p2-hosted-evidence -- --run-id <run-id> --attempt <attempt>
```

采集器重新查询指定 attempt 的运行元数据，要求 completed/success、手动事件、workflow、分支、候选 SHA 与 URL 全部精确匹配；三个原生平台 leg、矩阵汇总和完整质量门禁五个 job 还必须各出现一次且 completed/success。随后把该 attempt 的四个 artifact 下载到受保护临时目录并调用原有归档器。归档器会再次重放、确认受测 commit 是当前 HEAD 祖先，并把原始字节目录级原子保存到 `docs/reports/evidence/p2-a12-platform-evidence/`。只有归档成功才清理临时下载，并以不可覆盖、相同输入可幂等复核的方式发布 `docs/reports/evidence/p2-a12-hosted-run.json`；下载或重放失败会保留现场路径。不得省略显式 run ID/attempt、改用“最新 artifact”或手工复制结果替代该步骤。

## 5.1 P2-A11/P2-A12 统一机器结论

两个正式证据均就位后执行：

```text
node tools/verify-phase-2-external-gates.mjs
```

工具会从全部原始样本确定性重放 P2-A11，从归档的三个源 JSON 重放 P2-A12，并用四个归档文件的实际字节重放 `p2-a12-hosted-run.json`，核对同一 run/attempt/commit、五个成功 job 与 quality 结果；同时要求 Git 关系满足 `soak commit <= hosted matrix commit <= current HEAD`。通过结果以不可覆盖、相同输入可幂等复核的方式写入 `docs/reports/evidence/p2-external-gates.json`，字段由 [`phase-2-external-gates.schema.json`](../../schemas/phase-2-external-gates.schema.json) 固定，包含三个证据的 SHA-256、精简 summary、run URL、三个 runner 摘要和五个 hosted job，不复制大体积原始样本；拒绝结果只输出到终端并返回非零，不落下可能被误认成正式证据的文件。

只有统一报告 `accepted=true` 且 `failures=[]` 时，P2-A11/P2-A12 两项外部门禁才可一起视为满足。它不替代 P2-A01 至 P2-A10、完整质量门禁、数据安全复核或退出评审，因此不能单独把阶段 2 标成 Accepted。

## 5.2 P2-A04/P2-A10 候选故障收据

P2-A04 与 P2-A10 已有本地真实进程证据，但阶段退出还要从最终候选提交重复执行并发布统一机器收据：

```text
npm run test:p2-local-fault-gates
```

执行器要求 Node.js 24、无参数、输出文件尚不存在且 Git tracked/untracked 工作树完全干净，然后进行一次锁定依赖的 Release build。它记录候选 commit、Rust/Cargo/Node/平台信息和两个故障二进制 SHA-256，在唯一的系统临时根中完成以下三个真实 abort/recover 用例：

1. 批量事务在 `applied=317` 后 abort，第二进程发现 UUIDv7 `Active` 日志并继续到 1,000；
2. 缓存根改名后 abort，第二进程确认用户素材/Sidecar 不变并重建缓存；
3. 新缓存根建立后 abort，第二进程做同样恢复与用户文件校验。

任一进程状态、恢复输出、故障点数量、来源、二进制摘要或临时目录清理不满足时，命令返回非零、保留故障现场且不生成正式收据。全部满足后才删除本次精确创建的临时根；报告还必须通过独立收据检查器，随后才以不可覆盖方式写入 `docs/reports/evidence/p2-local-fault-gates.json`。[`p2-local-fault-gates.schema.json`](../../schemas/p2-local-fault-gates.schema.json) 只接受成功/abort 状态、固定 317/1000 边界、固定顺序的两个缓存故障点和已清理状态。

该命令会构建并运行 Rust 故障负载，因此不在 P2-A11 采样期间执行。当前执行器、Schema、离线严格检查器与七项纯判定测试已通过；正式候选收据仍 pending，必须等 soak 正常结束、partial 消失且候选证据提交后再运行。该本地收据与第 5.1 节外部门禁互相独立，两者都不能替代另一方。

## 5.3 阶段 2 最终机器退出收据

外部门禁、本地故障收据与实际结论更新都提交后，从完全干净的候选提交执行：

```text
npm run verify:p2-exit
```

最终验证器不把 `p2-external-gates.json` 当作不可复核的声明。它重新读取 A11 原始样本、A12 三个平台源 artifact/矩阵 artifact 和 hosted-run 收据，重建外部门禁并要求与已提交汇总逐字语义相同；随后用独立检查器验证 A04/A10 收据的完整字段、Node/Rust/Cargo 环境、二进制摘要、进程状态、317/1000 边界、两个缓存故障点顺序与临时目录清理。

Git 侧还必须同时满足：

1. `soak commit <= hosted matrix commit <= local fault commit <= final candidate HEAD`；
2. A11 固定加载脚本和产品输入相对正式 soak 基线零漂移；
3. 本地故障执行所绑定的干净提交中已包含当前逐字相同的 `p2-external-gates.json`；
4. 当前候选提交中已包含逐字相同的 `p2-local-fault-gates.json`；
5. 本地故障执行后只有 `README.md` 与 `docs/` 可以变化，任何工具、Schema、工作流、依赖或产品源码变化都要求从新的干净提交重跑故障门禁。

任一条件失败时只输出拒绝报告并返回非零。全部通过后，生成的报告还要由一条不读取原始输入的独立检查路径核对精确字段、摘要格式、固定边界、来源提交与确定性时间；只有自检也接受，才以不可覆盖方式生成 `docs/reports/evidence/p2-phase-2-exit.json`。其 accepted-only 结构由 [`phase-2-exit-evidence.schema.json`](../../schemas/phase-2-exit-evidence.schema.json) 固定，记录两个输入收据 SHA-256、四段提交关系、候选 SHA、来源基线和最小验收摘要；归档后可运行 `npm run inspect:p2-exit` 再次离线检查。当前纯分析的成功、综合篡改、非法路径和存档篡改四项测试与 Schema 编译已通过；正式退出命令必须等待 A11/A12 和本地候选故障收据全部就位，当前不能生成 accepted 结果。

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
| P2-A01 至 P2-A12 全部通过 | Pending：A11 running，A12 pending；外部与最终退出验证器均已就绪 |
| 完整扫描与增量模型最终一致 | Pass locally |
| 崩溃测试无截断 Sidecar | Pass locally；候选提交统一故障收据 pending |
| 连续 8 小时无无界资源增长 | Pending |
| 三平台核心路径原生测试通过 | Pending |
| P0/P1 数据安全缺陷为零 | Final audit pending；当前报告无已知未解决项 |

## 8. 阶段退出操作

只有第 4、5 节证据都通过后，执行一次候选提交审计：

1. 确认 P2-A11 final JSON、P2-A12 已归档 consolidated matrix/三个源 artifact、`p2-a12-hosted-run.json`/hosted logs 与实现 commit 可追溯，并生成 `p2-external-gates.json`；
2. 执行 `npm run audit:p2-soak-baseline`，确认正式 soak 生成器、原始判定器和该任务已编译的产品代码均未变化；若任一 scope 出现差异则重新跑相应门禁；
3. 提交所有候选代码与上述正式证据，确认没有 partial、临时 fixture、token 或本机路径，且 `git status` 完全干净；
4. 执行 `npm run test:p2-local-fault-gates`，检查不可覆盖收据通过 Schema 并绑定该干净候选提交；失败时保留现场、修复后从新的干净提交完整重跑；
5. 添加本地故障收据，更新 P2-03/P2-05/P2-06/P2-08、本报告、`docs/progress.md` 和 README，运行文档/Schema 检查与最终快速质量门禁，然后提交；此后不得修改 README/docs 之外的路径；
6. 再次确认工作树完全干净，执行 `npm run verify:p2-exit`，要求输出 `accepted=true`、`failures=[]` 并通过最终 Schema；
7. 把本报告改为实际 Accepted、添加最终退出收据并提交，再把阶段 3 从 Not started 改为 In progress。

## 9. 当前结论

P2-A01 至 P2-A10 已有本地自动化/故障注入证据，A04/A10 的最终候选收据执行器和阶段 2 最终退出验证器已就绪，但为隔离 soak 尚未执行正式候选故障负载。P2-A11 正在执行，P2-A12 仍受没有 Git remote 的外部托管矩阵阻塞。因此阶段 2 当前结论是 **Not accepted / In progress**，不得提前宣称完成。
