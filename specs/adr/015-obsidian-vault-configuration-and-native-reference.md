# ADR-015：Obsidian Vault 配置与原生引用边界

- 状态：Accepted
- 日期：2026-08-14

## 背景

P1-08 要求配置一个或多个 Obsidian Vault、判断素材归属、复制标准嵌入并从网格拖入 Obsidian。该能力不能引入素材数据库、不能复制或重命名素材，也不能让 WebView 用任意绝对路径绕过已授权目录。路径中的空格、Unicode、同名文件、WikiLink 保留字符和符号链接还会影响引用正确性。

## 决策

1. Vault 授权保存为应用配置目录中的可读 `obsidian-vaults.yml`，Schema 版本为 1。每项包含 UUIDv7、规范绝对路径、显示名称和启用状态；写入采用临时文件、同步与同目录原子替换，保留未知字段。
2. Vault 配置不是素材或笔记真相源。移除与停用只改变授权记录，不删除、移动、复制或改写 Vault、素材、Sidecar 与 Markdown。
3. 引用命令只接受 `vaultId` 和目录中的 `assetKey`，由 Rust 目录取回真实素材路径。Vault 路径与素材路径均规范化后再执行前缀判断；符号链接真实目标逃出 Vault 时拒绝引用。
4. 成功结果始终包含完整 Vault 相对目录，分隔符固定为 `/`。即使文件名在当前 Vault 唯一，也不缩短为 basename。
5. 原生嵌入固定为 `![[vault-relative/path.ext]]`。空格和 Unicode 在 WikiLink 中保持原样；同时返回按 UTF-8/RFC 3986 生成的 `urlEncodedPath`，供要求 URL 编码的 Markdown 链接或 URI 消费者使用。
6. `#`、`|`、`^`、`:`、`%`、方括号和控制字符属于 WikiLink 结构风险。P1-08 返回 `unsafe-wikilink`，不修改原文件名，也不尝试生成可能错误解析的引用。
7. 前端按当前可见键批量解析当前目标 Vault。只有解析成功的卡片可拖动；拖放只写入标准 Markdown 文本，不提供文件 URL、不触发附件复制。
8. 复制使用 Tauri 官方 clipboard-manager，只开放 `allow-write-text`。应用不申请读取剪贴板权限；写入内容只有标准 WikiLink。
9. 多 Vault 允许目录嵌套，因为目标 Vault 由稳定 ID 显式选择，不执行“最接近 Vault”猜测。重复的规范 Vault 路径仍拒绝配置。

## 后果

- Vault 内引用在关闭自定义插件后仍是可移植的 Obsidian 标准嵌入；
- 同名素材由完整目录消歧；
- Vault 外素材在阶段 1 明确显示不可引用，不会意外泄露绝对路径；
- 少数含 WikiLink 保留字符的文件需要用户在文件系统中自行重命名，应用不会代为重构；
- 当前 UI 对可见结果批量解析，十万级素材的窗口化与分块解析将在 P3-07 一并优化；
- P1-09 可以统一应用配置的恢复入口，但不得改变本 ADR 的授权和引用语义。

## 拒绝的方案

- 只按文件名生成 `![[logo.png]]`；
- 让前端提交任意绝对路径给引用命令；
- 拖出 `file://` 让 Obsidian复制一份附件；
- 为解决保留字符而自动重命名或复制素材；
- 使用 IndexedDB 保存 Vault 与素材归属；
- 申请剪贴板读取或图片写入权限；
- 对多个可能匹配的 Vault 自动猜测目标。
