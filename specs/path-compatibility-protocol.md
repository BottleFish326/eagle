# 路径兼容与离线根目录协议

本文定义 P2-08 的路径身份、只读兼容诊断、符号链接边界、根目录掉线语义和三平台验收方法。

## 1. 路径身份规则

| 目标平台 | 组件身份键 | 大小写 | Unicode | 说明 |
|---|---|---|---|---|
| Windows | NFC 后转小写 | 不敏感 | 组合形式等价 | 根仍先交给操作系统 canonicalize，展示保留真实路径 |
| macOS | NFD | 由卷决定 | 组合形式等价 | 不全局转小写；case-sensitive APFS 仍允许不同大小写文件 |
| Linux | 原字符串 | 敏感 | 不折叠 | `Logo.png`、`logo.png` 和不同组合形式均可为不同文件 |

`platform_path_key` 和 `path_relation_for_platform` 只做比较，不返回可写路径。配置根的 duplicate/ancestor/descendant 判定按逐组件关系执行，不做字符串前缀判断。

## 2. Windows 可移植性诊断

`inspect_relative_path_compatibility` 对相对路径报告：

- `CON`、`PRN`、`AUX`、`NUL`、`COM1` 至 `COM9`、`LPT1` 至 `LPT9`，包括带扩展名形式；
- 控制字符和 `< > : " | ? *`；
- 组件尾随点或空格；
- 整条输入达到传统 260 UTF-16 单元边界。

这些结果是跨平台同步提示，不是源文件重写指令。Linux/macOS 上合法但不适合 Windows 的素材仍参与扫描；长路径是否可完整操作由 Windows 原生矩阵继续验证。

## 3. 符号链接与重叠根

- 根配置固定 `followSymlinks: false`，旧配置若试图开启会加载失败；
- 扫描器不跟随任何文件或目录符号链接，每个遇到的链接产生明确 `ScanProblem`；
- 符号链接循环因此不会递归，链接目标也不计入当前根资产数；
- 新根 canonicalize 后若与已配置根相同、位于其内或包含它，添加操作明确失败；
- Obsidian 引用和支持检查继续对 canonical path 做授权根边界复核，不能借符号链接越界。

## 4. 根目录掉线状态机

```text
available + scan/watch
        |
        | missing / not-directory / permission-denied / unavailable
        v
non-authoritative failure
        |
        +-- 恢复扫描前目录记录
        +-- 停止该根监听
        +-- UI 标记实时访问状态并提示
        +-- 根恢复后由用户或监听重建触发完整扫描收敛
```

扫描批次是瞬态增量展示。只有结束前访问复核通过且 completion 为 `completed` 的扫描可以进行权威删除对账。取消、批次交付失败或根掉线都恢复扫描前记录。该过程不保存在线状态数据库，也不修改素材或 Sidecar。

## 5. P2-A12 自动化矩阵

本地专项命令：

```text
cargo test --locked -p asset-filesystem p2_platform
```

GitHub Actions `platform-paths` job 使用 `ubuntu-24.04`、`macos-15` 和 `windows-2025`，`fail-fast: false`。托管 leg 通过 Node 24 执行：

```text
node tools/verify-platform-paths.mjs --output <runner-temp>/p2-a12-platform-paths.json
```

证据器先 `--list` 再实际运行，要求 macOS 10、Linux 12、Windows 9 个精确具名测试全部列出且执行为 `ok`，0 failed/ignored/measured；缺项、新增未登记项、非零退出或摘要不一致都会拒绝。它还要求 clean Git、`GITHUB_SHA` 与 HEAD 一致、GitHub-hosted runner、runner OS/arch 可追溯；Windows 必须设置强制 symlink 环境且输出不得含 skip。每个 leg 即使失败也上传 90 天机器可读 JSON artifact，包含 commit、runner/runtime、命令、清单、实际执行项、summary 和原始进程输出。

三个 leg 结束后，`platform-matrix-evidence` job 使用 `actions/download-artifact@v8` 按当前 `github.sha` 下载三个独立 artifact，并执行：

```text
node tools/verify-platform-matrix.mjs \
  --input-directory <runner-temp>/p2-a12-input \
  --output <runner-temp>/p2-08-platform-matrix.json
```

最终 JSON 的可移植字段契约由 [`schemas/platform-matrix-evidence.schema.json`](../schemas/platform-matrix-evidence.schema.json) 固定；跨 artifact 的平台唯一性、原始输出重放、同一 run/attempt 和字段相互绑定仍由汇总器执行，因为这些约束不能只靠结构 Schema 表达。

汇总器不信任上游的布尔结论：它对每份原始 Cargo 输出重新解析具名测试和 summary，要求 darwin/linux/win32 各且仅各一份、Node 24、同一 HEAD/GitHub run/run attempt/workflow、GitHub-hosted runner、精确命令、完整工具链身份和与 runner OS/commit/run attempt 绑定的 artifact 名；这些字段还必须与当前汇总 job 的真实环境一致。源 artifact 使用 `p2-a12-source-<runner>-<sha>-attempt-<n>`，汇总 artifact 使用 `p2-a12-matrix-<sha>-attempt-<n>`，避免同一 workflow rerun 复用不可变旧产物或下载 pattern 误吞汇总结果。源 JSON 的 SHA-256 与逐平台摘要进入最终报告，原始 stdout/stderr 只保留在各平台 artifact，避免合并报告重复放大。只有 `p2-08-platform-matrix.json` 的 `accepted=true` 且 `failures=[]` 才是 P2-A12 的机器判定入口；单个 leg 的绿色状态或 JSON 不能独立关闭验收。

若某个 leg 失败，GitHub 的“仅重新运行失败 job”不会把旧 attempt 的成功 artifact 混入新结论，因此新汇总会因缺平台而拒绝。正式复验必须选择重新运行全部 job，让三个源 artifact 和汇总器来自同一 run attempt。

纯路径、原生 Unicode、重叠根和禁止 `followSymlinks` 配置的夹具在三平台执行。Unix 执行权限撤销、拔盘模拟和符号链接循环；Linux 额外执行大小写同名素材并存和移动根目录掉线。Windows leg 显式启用长路径策略，必须成功创建原生符号链接，并执行 260+ UTF-16 路径扫描及同路径 Sidecar 原子替换；强制环境下符号链接夹具不得静默 skip。

## 6. 失败语义

- 根在扫描开始前不可用：拒绝启动，读取最新根状态；
- 根在扫描中不可用：返回 `RootUnavailable`，不提交权威结果；
- 只有子目录读取失败且根仍可访问：保留为有路径的 `ScanProblem`，扫描可继续；
- 监听通道断开：请求一致性扫描并发送失败事件，若根不可用则同时切换离线状态；
- 符号链接、Windows 可移植性警告和平台碰撞诊断都不得自动改名或删除。
