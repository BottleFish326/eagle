# P2-08 平台与文件系统兼容验收报告

- 状态：Implemented locally；P2-A12 hosted matrix pending
- 日期：2026-08-20
- 对应：P2-08、P2-A08、P2-A09、P2-A12
- 决策：[ADR-025](../../specs/adr/025-platform-path-and-offline-root-semantics.md)
- 协议：[路径兼容与离线根目录协议](../../specs/path-compatibility-protocol.md)

## 1. 已交付范围

- 新增无持久状态的路径兼容模块，提供 Windows、macOS、Linux 三种确定性身份键和逐组件路径关系；
- Windows 夹具覆盖大小写、Unicode 组合形式、保留设备名、禁用字符、尾随点/空格和传统长路径边界；
- macOS 夹具覆盖 NFC/NFD 等价，同时保留卷大小写模式差异；Linux 夹具保留大小写和 Unicode 拼写差异；
- 根目录重叠从字符串前缀比较升级为 canonical path 的平台逐组件比较；
- 扫描继续禁止跟随符号链接，但现在为每个跳过的链接输出明确问题；
- 非取消扫描在每次批次发布后和权威完成前复核根目录访问，扫描中拔盘或撤权会返回非权威失败；
- 桌面后端在失败时恢复扫描前目录记录，扫描/监听失败事件携带实时根访问状态；
- UI 把对应根标为 missing、permission-denied、not-directory 或 unavailable，保留根配置和原有素材视图并显示降级提示；
- CI 新增独立三平台 `platform-paths` job，不要求 Linux 安装完整 Tauri 图形依赖即可执行核心路径套件；
- 新增 P2-A12 机器可读证据器，精确核对每个平台应列出/执行的测试名、通过数、Git commit、Node/Rust/runner 环境和 hosted 来源；
- 三个 leg 无论成功失败都上传 90 天 JSON artifact，缺测试、ignored、非 GitHub-hosted 或 Windows symlink skip 都明确拒绝；
- 新增独立 matrix 汇总器和 CI job，下载同一提交的三个 artifact、重新解析原始 Cargo 输出，并拒绝缺平台、重复平台、跨提交、跨 workflow run/attempt、产物改名或内容摘要异常；
- 源/汇总 artifact 都绑定 run attempt，分别命名为 `p2-a12-source-<runner>-<sha>-attempt-<n>` 与 `p2-a12-matrix-<sha>-attempt-<n>`，避免 rerun 冲突或混入旧证据；只有汇总 JSON 为 `accepted=true` 且 `failures=[]` 时代表机器验收通过；
- 新增最终汇总证据 JSON Schema，固定成功与失败报告、验证 job 环境、三个源 artifact 摘要和精确测试 summary 的可移植字段契约；跨文件一致性继续由重放汇总器判定；
- 三个源报告与汇总 job 还必须共享 GitHub repository/server；最终报告从受校验字段生成稳定 workflow `runUrl`，正式归档不再依赖人工抄写链接；
- 新增离线归档器：下载四个 artifact 文件后再次计算摘要和重放 matrix，确认受测 commit 是当前 HEAD 祖先，再把原始字节以目录级原子 rename 保存到固定证据目录；相同输入可幂等执行，差异输入绝不覆盖；
- CI 支持 `workflow_dispatch` 显式触发；新增只读托管就绪预检，核对 GitHub CLI/认证、GitHub origin/默认分支、`main -> origin/main`、本地与远端精确 commit、tracked 清洁状态及手动触发入口，并生成绑定 commit/run attempt 的触发、定位、等待、下载和归档命令；
- 新增指定运行证据采集器：只接受显式 `run-id`/`attempt`，重新读取 GitHub 运行元数据并要求 completed/success、`workflow_dispatch`、`CI`、`main`、候选 SHA 和规范 run URL 全部精确匹配；五个必需 job 必须各出现一次且 completed/success。随后在受保护临时目录下载该 attempt 的两个 artifact 模式、调用既有四文件重放归档器，只有归档成功才清理下载目录并以不可覆盖方式发布 `p2-a12-hosted-run.json` 收据，失败现场保留并报告路径；
- 新增托管运行收据 Schema 和离线重放器，把 workflow/run/attempt/commit、五个 job 的 ID/URL/时间/结论及四个归档文件摘要绑定到同一矩阵；阶段 2 统一外部门禁会重新计算归档摘要并拒绝收据、job 或 matrix 的任何错配；
- Windows leg 显式启用长路径策略并要求原生符号链接夹具可创建，验证 260+ UTF-16 路径扫描、Sidecar 创建/替换和循环跳过；
- Linux leg 增加大小写同名素材并存和扫描中移动根目录的非权威失败夹具，既有撤权夹具继续验证 permission-denied。

所有路径规则、状态和扫描批次仍是运行期解释。没有新增素材数据库、在线状态数据库或权威扫描快照，也没有改名、移动、复制、删除任何源素材。

## 2. 专项自动化证据

本机执行：

```text
cargo test --locked -p asset-filesystem p2_platform
```

macOS ARM64 共 10 项通过：

| 验收点 | 自动化证据 | 结果 |
|---|---|---|
| Windows 大小写/Unicode | `Design/Café.PNG` 与大小写、组合形式变体得到相同 Windows 键 | Pass |
| Windows 保留规则 | `CON.txt`、`bad?.png`、尾随点/空格和 260+ UTF-16 单元均返回稳定诊断 | Pass |
| macOS Unicode | NFC 与 NFD 名称等价，但 `Logo.png` 与 `logo.png` 不在规则层强制折叠 | Pass |
| Linux 路径身份 | 大小写和 NFC/NFD 拼写都保持不同 | Pass |
| 原生 Unicode | `Café-素材.png` 被扫描且相对路径与文件名未重写 | Pass |
| 根重叠 | 相同根、父包含子、子位于父内三种关系都明确拒绝 | Pass |
| 禁止跟随链接 | 配置文件即使显式写入 `followSymlinks: true` 也在加载时被拒绝 | Pass |
| 符号链接循环 | 根内链接回根自身，扫描只计 1 个真实素材、1 条显式跳过问题并终止 | Pass |
| 扫描中拔盘 | 首批后删除模拟移动盘，返回 `RootUnavailable(missing)`，不产生 completed | Pass |
| 扫描中撤权 | 首批后撤销根权限，返回 `RootUnavailable(permission-denied)`，权限随后恢复 | Pass |

桌面 TypeScript 另有纯状态测试确认只有失败根切换为 permission-denied，其他根与配置记录保持不变。Rust wire test确认失败事件把 `rootAccessStatus` 序列化为稳定 kebab-case 值。

单平台证据器使用 Node 24 执行 9 项纯分析测试，覆盖 macOS 10/Linux 12/Windows 9 项精确正例，以及缺项、非零、ignored、无摘要、Windows symlink skip 和非 hosted/commit mismatch 负例；全部通过。matrix 汇总器另有 10 项纯分析测试，覆盖三平台精确正例，以及缺/重复平台、跨 commit/run 或与汇总 job 不一致、原始输出或存储摘要篡改、非零进程、自托管 runner、Windows symlink 未强制、false verdict、错误 Node/工具链/命令/时间/摘要/产物名等负例；全部通过。2 项 CLI 端到端测试还实际创建三份隔离 artifact、调用汇总进程并复核 SHA-256/原子输出，同时确认缺一平台时退出非零且仍写出拒绝 JSON。

归档器另有 2 项真实文件测试，覆盖四文件重放、原始字节保存、目录级原子发布、幂等复核，以及 matrix 字段或源文件字节改变时写前拒绝。

托管就绪预检另有 3 项纯测试，覆盖已发布且作为 GitHub 默认分支的 `main` 完整正例、工具/远端/默认分支/本地分支/上游/commit/工作树/工作流缺口的拒绝，以及 HTTPS、SCP 风格 SSH 和 `ssh://` 三种 GitHub origin 规范化。

指定运行采集与收据重放现有 6 项测试，覆盖成功运行与五个必需 job 元数据、artifact pattern、运行身份/状态错配、download → archive → cleanup → receipt 严格顺序，以及运行拒绝时零下载、错误 pattern/归档摘要拒绝、归档失败时保留临时证据和归档字节/收据篡改拒绝。

阶段退出统一验证器另有 2 项测试，以完整 28,800 秒采样数量的合成证据同时重放 P2-A11/P2-A12、生成确定性结论并验证幂等写入；soak summary 篡改或 commit 顺序未经证明时均拒绝。

本机真实 `cargo --list`/`cargo test` 输出交给同一单平台分析器后，得到 macOS expected/listed/executed 各 10 项、summary 10 passed/0 failed/0 ignored。以上只验证证据链实现，不替代 hosted P2-A12。

## 3. P2 验收项结论

| 验收项 | 当前结论 | 证据/缺口 |
|---|---|---|
| P2-A08 扫描中撤权或拔盘 | Pass locally | 两种故障均转为非权威失败；后端恢复前态，UI 只标记对应根离线且不退出应用 |
| P2-A09 符号链接循环和重叠根 | Pass locally | 不跟随链接、无重复计数、有明确问题；三种根重叠明确拒绝 |
| P2-A12 三平台路径兼容 | Pending | macOS 原生 10 项通过；Linux 预期 12 项、Windows 预期 9 项；逐平台证据与跨平台重放汇总 job 均已配置，但尚无远程仓库可触发 |

P2-08 实现可合并，但不能把 P2-A12 或阶段 2 标为 Accepted。必须取得 GitHub Actions Ubuntu/macOS/Windows 三个 matrix leg 的实际通过结果；条件编译通过只证明这些原生用例可构建，不证明 NTFS/ext4 行为。

## 4. 跨目标检查

以下命令无警告通过：

```text
cargo check --locked -p asset-filesystem --tests --target x86_64-unknown-linux-gnu
cargo check --locked -p asset-filesystem --tests --target x86_64-pc-windows-msvc
cargo clippy --locked -p asset-filesystem --tests --target x86_64-unknown-linux-gnu -- -D warnings
cargo clippy --locked -p asset-filesystem --tests --target x86_64-pc-windows-msvc -- -D warnings
```

交叉检查只证明类型、依赖和条件编译正确，不等同于在 NTFS/ext4 上执行测试。托管矩阵配置为 `ubuntu-24.04`、`macos-15`、`windows-2025` 且 `fail-fast: false`。

## 5. 完整质量门禁

完整执行并通过 `npm run ci`：

- Rust：128 项测试；
- 桌面 TypeScript：46 项测试；
- Obsidian Bridge：8 项测试；
- Clippy、Rust/TypeScript 格式和静态检查：通过；
- S 数据集：1,000 素材、200 Sidecar、999 个有效尺寸、1 个损坏图片隔离、0 个扫描问题，56 毫秒，原始素材摘要不变；
- Tauri release、桌面 Vite 与 Obsidian Bridge production build：通过。

## 6. 后续验收动作

1. 建立 GitHub origin、安装并认证 GitHub CLI，把当前 `main` 推送为 `origin/main`；
2. 运行 `npm run audit:p2-hosted-readiness`，只有输出 `ready=true` 时才使用报告内的 commit-bound 命令触发 `ci.yml`；该预检只读，不创建仓库、不推送、不触发 workflow；
3. 确认 `platform-paths` 的三个 matrix leg、`platform-matrix-evidence` 和 `quality` 均实际通过，核对 CLI 返回的 run ID/attempt 后执行 `npm run collect:p2-hosted-evidence -- --run-id <run-id> --attempt <attempt>`；采集器会再次验证运行身份和五个 job，精确下载并调用归档器，不能改用“最新运行”；
4. 确认 Windows leg 实际执行强制符号链接、260+ 路径扫描及 Sidecar 原子替换，不能以 skip 计为通过；
5. 确认 Linux leg 实际执行大小写并存、权限撤销和移动根目录掉线；
6. 确认最终 `p2-08-platform-matrix.json` 是 `accepted=true`、`failures=[]`、同一 Git commit/run/attempt，且三个源 artifact 的 SHA-256、expected/listed/executed 和 summary 均被重放核对；
7. 确认采集器输出 `accepted=true`、`failures=[]`、`temporaryDownloadRemoved=true`，提交固定证据目录及 `p2-a12-hosted-run.json`；
8. 运行 `verify-phase-2-external-gates.mjs`，只有统一报告 `accepted=true` 后才评估阶段 2 退出；
9. P2-A11 连续 8 小时仍按 P2-06 报告独立执行，二者全部通过后才评估阶段 2 退出。

若首次运行有 leg 失败，必须使用 “Re-run all jobs” 进行正式复验；只重跑失败 job 会保留不同 attempt 的成功 leg，汇总器会按设计拒绝混合证据。
