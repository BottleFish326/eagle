# 平台与文件系统差异记录

> 最后更新：2026-08-20

## 当前验证范围

| 平台 | 编译 | P2 路径测试 | 文件监听实测 | 状态 |
|---|---|---|---|---|
| macOS ARM64 | 通过 | 9 项通过 | 通过 | P2-A08/P2-A09 本机通过 |
| Linux x86_64 GNU | 交叉 `cargo check` 通过 | 托管矩阵已配置，待首次运行 | 未运行 | P2-A12 待 CI/实体平台 |
| Windows x86_64 MSVC | 交叉 `cargo check` 通过 | 托管矩阵已配置，待首次运行 | 未运行 | P2-A12 待 CI/实体平台 |

交叉检查命令：

```bash
rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-msvc
cargo check -p asset-filesystem --all-targets --target x86_64-unknown-linux-gnu
cargo check -p asset-filesystem --all-targets --target x86_64-pc-windows-msvc
cargo test --locked -p asset-filesystem p2_platform
```

托管矩阵定义在 `.github/workflows/ci.yml` 的 `platform-paths` job，目标为 Ubuntu 24.04、macOS 15 和 Windows 2025。随后 `platform-matrix-evidence` 下载三个 commit-bound JSON，重新解析原始 Cargo 输出并生成唯一汇总结论。当前仓库没有 Git 远程且本机没有 `gh` 可执行文件，因此不能把“工作流已配置”写成“托管运行已通过”。

## 已知差异

### macOS

- APFS 路径可能把 `/tmp` 规范化为 `/private/tmp`；测试必须比较 realpath；
- Unicode 路径身份使用 NFD 规范键，但不假定卷的大小写模式；根先 canonicalize，展示路径不被改写；
- 文件监听一次复制会产生一个 create 和多个 modify 事件，必须去抖；
- 父目录支持 `fsync`，原子替换后会刷新父目录。

### Windows

- 路径比较键按 NFC 后小写归一，但必须保留 canonical path 的显示大小写；
- 保留设备名、禁用字符、尾随点/空格和传统 260 UTF-16 单元边界产生只读可移植性诊断；
- `File::open` 不能可移植地用于目录 `fsync`，当前原型只保证替换前文件内容已刷新；
- 最终实现需要验证 `NamedTempFile::persist` 的替换语义、占用文件失败和杀毒软件干扰；
- 符号链接测试可能需要开发者模式或提升权限；真实长路径读写也必须在原生运行中复核。

### Linux

- 路径大小写敏感；
- Unicode 组合形式不折叠，名称不同的文件必须保留为不同素材；
- `notify` 使用 inotify，必须处理 watch 数量上限和队列溢出；
- 不同桌面环境的移动盘和网络盘挂载行为需要实测。

## 统一降级语义

- 根配置不允许跟随符号链接；扫描发现链接时明确报告并跳过，因此循环不递归、目标不重复计数；
- 根添加前 canonicalize，并按当前平台逐组件拒绝重复或祖先/后代重叠；
- 非取消扫描在批次后和结束前复核根访问。拔盘、撤权、路径变为文件或其他 I/O 不可用会使扫描成为非权威失败；
- 桌面端恢复扫描前目录记录，把根标记为 missing、permission-denied、not-directory 或 unavailable，并停止/重建监听；
- 所有路径诊断都不重命名、移动、复制或删除源素材。

## 阶段退出要求

交叉编译和纯规则夹具不能替代运行时验收。阶段 2 退出前必须取得三平台 `p2_platform` 托管结果；正式公开版本前还必须在 Windows、Linux CI 或实体环境运行：

- Sidecar 原子写入与冲突测试；
- 文件创建、修改、移动、删除和事件风暴；
- Unicode、长路径、大小写和权限测试；
- 8 小时稳定性测试。
