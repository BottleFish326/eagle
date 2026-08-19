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

本地/托管统一命令：

```text
cargo test --locked -p asset-filesystem p2_platform
```

GitHub Actions `platform-paths` job 使用 `ubuntu-24.04`、`macos-15` 和 `windows-2025`，`fail-fast: false`。纯路径夹具在三平台执行；原生 Unicode、重叠根夹具在三平台执行；权限撤销、拔盘模拟和符号链接循环在 Unix 执行。Windows 符号链接与真实长路径还需要具备相应权限和策略的实体/托管运行补充。

## 6. 失败语义

- 根在扫描开始前不可用：拒绝启动，读取最新根状态；
- 根在扫描中不可用：返回 `RootUnavailable`，不提交权威结果；
- 只有子目录读取失败且根仍可访问：保留为有路径的 `ScanProblem`，扫描可继续；
- 监听通道断开：请求一致性扫描并发送失败事件，若根不可用则同时切换离线状态；
- 符号链接、Windows 可移植性警告和平台碰撞诊断都不得自动改名或删除。
