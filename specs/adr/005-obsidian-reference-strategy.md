# ADR-005：Obsidian 引用策略

- 状态：Accepted
- 日期：2026-08-14

## 背景

Obsidian 对 Vault 内附件提供标准内部链接和嵌入；Vault 外素材若要在笔记中长期引用，需要稳定 ID 和桌面插件。

## 决策

1. 素材位于目标 Vault 内时，生成标准 `![[vault/relative/path.ext]]`。
2. 同名文件始终生成包含目录的无歧义路径。
3. 素材位于 Vault 外时，使用 `![alias](material://<uuid>)`。
4. 外部引用只包含稳定 ID，不包含绝对路径、端口或访问令牌。
5. Obsidian 插件按相同 sidecar Schema 建立可重建最小索引。
6. 桌面素材管理器未运行时，插件仍能从用户授权根目录解析外部引用。
7. 插件禁用时不得改写 Markdown；Vault 内引用继续工作，外部引用保留可读源码。
8. 移动端只承诺 Vault 内标准引用。

## 后果

- Vault 内笔记保持最大的 Markdown 可移植性；
- Vault 外引用依赖桌面插件；
- 外部素材和 sidecar 一起移动后，无需改写 Markdown；
- Obsidian Publish 和第三方阅读器不会原生渲染 `material://`。

## 依据

- [Obsidian 嵌入文件](https://obsidian.md/help/embeds)
- [Obsidian 内部链接](https://obsidian.md/help/links)
- [Obsidian 插件开发](https://docs.obsidian.md/Plugins/Getting%20started/Build%20a%20plugin)
