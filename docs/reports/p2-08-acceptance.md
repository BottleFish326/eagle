# P2-08 平台与文件系统兼容验收报告

- 状态：Implemented locally；P2-A12 hosted matrix pending
- 日期：2026-08-19
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
- CI 新增独立三平台 `platform-paths` job，不要求 Linux 安装完整 Tauri 图形依赖即可执行核心路径套件。

所有路径规则、状态和扫描批次仍是运行期解释。没有新增素材数据库、在线状态数据库或权威扫描快照，也没有改名、移动、复制、删除任何源素材。

## 2. 专项自动化证据

本机执行：

```text
cargo test --locked -p asset-filesystem p2_platform
```

macOS ARM64 共 9 项通过：

| 验收点 | 自动化证据 | 结果 |
|---|---|---|
| Windows 大小写/Unicode | `Design/Café.PNG` 与大小写、组合形式变体得到相同 Windows 键 | Pass |
| Windows 保留规则 | `CON.txt`、`bad?.png`、尾随点/空格和 260+ UTF-16 单元均返回稳定诊断 | Pass |
| macOS Unicode | NFC 与 NFD 名称等价，但 `Logo.png` 与 `logo.png` 不在规则层强制折叠 | Pass |
| Linux 路径身份 | 大小写和 NFC/NFD 拼写都保持不同 | Pass |
| 原生 Unicode | `Café-素材.png` 被扫描且相对路径与文件名未重写 | Pass |
| 根重叠 | 相同根、父包含子、子位于父内三种关系都明确拒绝 | Pass |
| 符号链接循环 | 根内链接回根自身，扫描只计 1 个真实素材、1 条显式跳过问题并终止 | Pass |
| 扫描中拔盘 | 首批后删除模拟移动盘，返回 `RootUnavailable(missing)`，不产生 completed | Pass |
| 扫描中撤权 | 首批后撤销根权限，返回 `RootUnavailable(permission-denied)`，权限随后恢复 | Pass |

桌面 TypeScript 另有纯状态测试确认只有失败根切换为 permission-denied，其他根与配置记录保持不变。Rust wire test确认失败事件把 `rootAccessStatus` 序列化为稳定 kebab-case 值。

## 3. P2 验收项结论

| 验收项 | 当前结论 | 证据/缺口 |
|---|---|---|
| P2-A08 扫描中撤权或拔盘 | Pass locally | 两种故障均转为非权威失败；后端恢复前态，UI 只标记对应根离线且不退出应用 |
| P2-A09 符号链接循环和重叠根 | Pass locally | 不跟随链接、无重复计数、有明确问题；三种根重叠明确拒绝 |
| P2-A12 三平台路径兼容 | Pending | macOS 原生 9 项通过；Linux/Windows 仅交叉检查通过，托管运行尚无远程仓库可触发 |

P2-08 实现可合并，但不能把 P2-A12 或阶段 2 标为 Accepted。必须取得 GitHub Actions Ubuntu/macOS/Windows 三个 matrix leg 的实际通过结果，并补充 Windows 原生符号链接权限和长路径读写验证。

## 4. 跨目标检查

以下命令无警告通过：

```text
cargo check --locked -p asset-filesystem --all-targets --target x86_64-unknown-linux-gnu
cargo check --locked -p asset-filesystem --all-targets --target x86_64-pc-windows-msvc
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

1. 建立 Git 远程并推送当前工作流；
2. 确认 `platform-paths` 的三个 matrix leg 均实际通过并保存运行链接/提交号；
3. 在 Windows 原生环境补充启用/未启用符号链接权限、260+ 完整路径的扫描和 Sidecar 原子写入；
4. 在 Linux 实体或托管环境补充大小写并存、权限撤销和移动挂载点掉线；
5. P2-A11 连续 8 小时仍按 P2-06 报告独立执行，二者全部通过后才评估阶段 2 退出。
