# Obsidian Vault 内引用协议

本文档定义 P1-08 的 Vault 配置、路径解析、复制与拖放协议。Vault 外 `material://<uuid>` 仍是阶段 0 技术原型，不属于阶段 1 发布承诺。

## 1. 配置文件

桌面端在操作系统应用配置目录保存 `obsidian-vaults.yml`：

```yaml
schema: 1
vaults:
  - id: 0198a9b2-43c0-7cb0-a733-6dc58f829815
    path: /Users/name/Documents/Design Notes
    name: Design Notes
    enabled: true
```

文件遵循 `schemas/obsidian-vaults.schema.json`，使用临时文件、`fsync` 和同目录原子替换写入。未知字段在读取与改写后保留。移除或停用配置不删除、不移动 Vault 或素材文件。

## 2. 解析输入和安全边界

`resolve_obsidian_vault_references` 接收：

```json
{
  "input": {
    "vaultId": "<uuid>",
    "assetKeys": ["<catalog-key>"]
  }
}
```

素材必须已存在于当前内存目录。后端不接受前端提供的任意绝对素材路径。解析顺序为：

1. 根据稳定 Vault ID 查找已启用配置；
2. 从目录按 `assetKey` 取回真实素材路径；
3. 分别规范化 Vault 与素材真实路径；
4. 使用 `strip_prefix` 判断素材是否位于目标 Vault；
5. 将相对路径分隔符统一为 `/`；
6. 检查 Obsidian WikiLink 保留字符；
7. 返回原生 WikiLink 与独立 URL 编码路径。

符号链接解析后的真实目标若逃出 Vault，按 `outside-vault` 失败。单项失败不阻断同一批次中其他素材。

## 3. 返回结构

成功项：

```json
{
  "assetKey": "/vault/设计 素材/封面 图.png",
  "vaultId": "<uuid>",
  "vaultName": "Design Notes",
  "assetPath": "/vault/设计 素材/封面 图.png",
  "relativePath": "设计 素材/封面 图.png",
  "urlEncodedPath": "%E8%AE%BE%E8%AE%A1%20%E7%B4%A0%E6%9D%90/%E5%B0%81%E9%9D%A2%20%E5%9B%BE.png",
  "markdown": "![[设计 素材/封面 图.png]]"
}
```

失败项包含 `assetKey`、稳定 `kind` 与可诊断 `message`。稳定失败类型包括：

- `asset-not-found`；
- `vault-not-found`；
- `vault-disabled`；
- `vault-unavailable`；
- `asset-unavailable`；
- `outside-vault`；
- `unsafe-wikilink`；
- `internal`。

## 4. 路径与编码规则

Obsidian 原生 WikiLink 使用 Vault 根相对路径、正斜杠、原始空格和 Unicode，例如 `![[设计 素材/封面 图.png]]`。目录不得省略，即使 Vault 中只有一个同名文件也不生成短名称。

`urlEncodedPath` 按 UTF-8 字节和 RFC 3986 unreserved 集编码，保留 `/` 作为目录分隔符。该字段供未来 Markdown 链接或 Obsidian URI 消费者使用；它不替换原生 WikiLink 中的路径。Obsidian 官方说明 URL 编码是 Markdown 链接目标的要求，WikiLink 默认使用更紧凑的原始路径。

`#`、`|`、`^`、`:`、`%`、`[`、`]` 和控制字符会与 WikiLink 的标题、别名、块引用或语法边界冲突。P1-08 对包含这些字符的路径返回 `unsafe-wikilink`，不猜测、不重命名原文件，也不生成可能指错文件的链接。

## 5. 复制与拖放

- 桌面复制调用 Tauri 官方 clipboard-manager，仅授予 `allow-write-text`，不授予读取权限；
- 写入内容只有 `markdown`，不包含绝对路径、Vault ID 或机器标识；
- 浏览器开发预览使用浏览器剪贴板，只作用于内存演示数据；
- 网格卡片仅在成功解析后设置 `draggable=true`；
- `dragstart` 写入 `text/plain` 和 `text/markdown`，值均为标准 `![[...]]`；
- 自定义 MIME 只包含 Vault ID、相对路径和 Markdown，绝不包含绝对路径；
- 拖放是文本引用，不传递文件 URL，不要求 Obsidian 复制附件。

## 6. 依据

- [Obsidian：内部链接](https://obsidian.md/help/links)
- [Obsidian：嵌入文件](https://obsidian.md/help/embeds)
- [Tauri：Clipboard 插件](https://v2.tauri.app/plugin/clipboard/)
