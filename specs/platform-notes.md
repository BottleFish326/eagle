# 阶段 0 平台差异记录

> 最后更新：2026-08-14

## 当前验证范围

| 平台 | 编译 | 单元/集成测试 | 文件监听实测 | 状态 |
|---|---|---|---|---|
| macOS ARM64 | 通过 | 通过 | 通过 | 已验证 |
| Linux x86_64 GNU | `cargo check` 通过 | 未运行 | 未运行 | 运行时待 CI/实体平台 |
| Windows x86_64 MSVC | `cargo check` 通过 | 未运行 | 未运行 | 运行时待 CI/实体平台 |

交叉检查命令：

```bash
rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-msvc
cargo check --workspace --all-targets --target x86_64-unknown-linux-gnu
cargo check --workspace --all-targets --target x86_64-pc-windows-msvc
```

## 已知差异

### macOS

- APFS 路径可能把 `/tmp` 规范化为 `/private/tmp`；测试必须比较 realpath；
- 文件监听一次复制会产生一个 create 和多个 modify 事件，必须去抖；
- 父目录支持 `fsync`，原子替换后会刷新父目录。

### Windows

- 路径默认大小写不敏感，但必须保留原始显示大小写；
- `File::open` 不能可移植地用于目录 `fsync`，当前原型只保证替换前文件内容已刷新；
- 最终实现需要验证 `NamedTempFile::persist` 的替换语义、占用文件失败和杀毒软件干扰；
- 符号链接测试可能需要开发者模式或提升权限。

### Linux

- 路径大小写敏感；
- `notify` 使用 inotify，必须处理 watch 数量上限和队列溢出；
- 不同桌面环境的移动盘和网络盘挂载行为需要实测。

## 阶段退出要求

当前交叉编译只能证明类型和条件编译正确，不能替代运行时验收。正式公开版本前必须在 Windows、Linux CI 或实体环境运行：

- Sidecar 原子写入与冲突测试；
- 文件创建、修改、移动、删除和事件风暴；
- Unicode、长路径、大小写和权限测试；
- 8 小时稳定性测试。
