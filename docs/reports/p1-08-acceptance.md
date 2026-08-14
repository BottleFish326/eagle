# P1-08 Obsidian Vault 内引用验收报告

> 验收日期：2026-08-14
>
> 验收状态：本地通过

## 1. 验收范围

本报告验收开发计划中的 P1-08：

- 配置一个或多个 Obsidian Vault 根路径；
- 判断目录中的素材是否位于当前目标 Vault；
- 生成完整且无歧义的 Vault 相对路径；
- 对空格和 Unicode 生成对应 URL 编码路径；
- 复制标准 `![[path/to/image.png]]`；
- 从素材网格拖出标准 Markdown 引用；
- 对同名文件保留不同目录；
- 隔离 Vault 外路径、符号链接逃逸、不可访问文件和 WikiLink 保留字符。

Vault 外 `material://` 正式集成、从 Obsidian 内搜索素材和双向定位属于阶段 4，不在本次范围。应用配置统一恢复与诊断入口属于 P1-09。

## 2. 核心实现

新增 `asset-link-resolver` Rust crate。Vault 授权保存为应用配置目录中的 `obsidian-vaults.yml`，由 `schemas/obsidian-vaults.schema.json` 描述。配置使用 UUIDv7 标识，支持增、改、停用和移除；写入使用临时文件、文件同步、原子替换和父目录同步，未知 YAML 字段在更新后保留。

解析命令不接受任意素材绝对路径。前端提交目标 `vaultId` 与目录中的 `assetKey`，后端从内存目录取回真实素材路径，对 Vault 和素材分别规范化，再使用路径组件前缀判断归属。符号链接解析后若逃出 Vault，按单项失败返回。

返回同时包含：

- `relativePath`：Obsidian 原生 WikiLink 使用的 Vault 相对路径；
- `urlEncodedPath`：按 UTF-8 字节与 RFC 3986 生成、保留 `/` 的编码路径；
- `markdown`：标准 `![[relativePath]]`。

空格和 Unicode 在 WikiLink 中保持原样，符合 Obsidian 原生链接格式；URL 编码字段供 Markdown 链接或 URI 消费者使用。`#`、`|`、`^`、`:`、`%`、方括号和控制字符会返回 `unsafe-wikilink`，应用不重命名原素材，也不生成可能错误解析的引用。完整协议见 `specs/obsidian-vault-reference-protocol.md`，架构决策见 ADR-015。

## 3. 复制、拖放与权限

桌面端使用 Tauri 官方 clipboard-manager 2.3.2，并把 capability 限制为：

```text
clipboard-manager:allow-write-text
```

未授予读取文本、读取图片、写入图片、HTML 或清空剪贴板权限。复制内容只有 `![[...]]`，不包含绝对路径和机器信息。

当前目标 Vault 的可见素材由 Rust 批量解析。只有成功项的网格卡片设置为可拖动；`dragstart` 写入相同的 `text/plain` 与 `text/markdown` 标准引用。自定义 MIME 只包含 Vault ID、相对路径与 Markdown，不包含绝对路径，不传递 `file://`，因此不会要求 Obsidian 复制附件。

## 4. P1 验收用例

| ID | 操作与证据 | 结果 |
|---|---|---|
| P1-A08 | 中文与空格目录生成 `![[设计 素材/封面 图.png]]`；前端复制测试断言剪贴板只收到该 WikiLink；阶段 0 已在 Obsidian 1.12.7 实机确认相同标准 Vault 内引用在插件停用后仍渲染 | 通过 |
| P1-A09 | `brand/logo.png` 与 `product/logo.png` 分别生成 `![[brand/logo.png]]`、`![[product/logo.png]]`；拖放测试断言两种文本 MIME 原样保留完整 Markdown | 通过 |

P1-A08 的 Obsidian 渲染证据沿用已归档的阶段 0 `P0-A06`：标准 `![[internal.png]]` 在插件停用后仍为完整 `app:` 图片，`complete=true`、`naturalWidth=1`。P1-08 没有引入新的 Markdown 方言，只把同一标准语法接入桌面目录与 UI。

## 5. 自动化测试

`asset-link-resolver` 新增 10 项测试：

| 场景 | 断言 | 结果 |
|---|---|---|
| 配置生命周期 | 添加、重命名、停用和移除后 YAML 可重载，Vault 文件保持存在 | 通过 |
| 重复路径 | 等价规范路径不能重复授权 | 通过 |
| 空格与中文 | WikiLink 保留原文，URL 编码逐 UTF-8 字节正确 | 通过 |
| 同名文件 | 两个 `logo.png` 都包含各自目录 | 通过 |
| Vault 外素材 | 返回 `outside-vault`，不生成引用 | 通过 |
| 保留字符 | 返回 `unsafe-wikilink`，不猜测路径 | 通过 |
| 符号链接逃逸 | 真实目标位于 Vault 外时拒绝 | 通过 |
| 未知字段 | 顶层与单 Vault 未来字段更新后保留 | 通过 |
| 配置重复 ID | 启动读取时拒绝无效配置 | 通过 |
| 编码函数 | 空格、Unicode 与目录分隔符结果固定 | 通过 |

桌面前端现为 8 个测试文件、20 项测试，新增覆盖 Tauri 命令名与参数、无绝对路径剪贴板内容、三种拖放 MIME 和结构化失败提示。Rust workspace 共 56 项测试通过；Obsidian 插件 8 项测试继续通过。

## 6. 浏览器界面验收

使用本地 Vite 演示适配器和实际浏览器页面检查组合界面：

| 操作 | 观察结果 | 结论 |
|---|---|---|
| 启动并加载目标 Vault | 顶栏显示 `Design Notes`，16 个 Vault 内素材均出现拖拽标识 | 通过 |
| 选择网格素材 | 检查器显示 Vault 名、`Vault 内` 状态和 `![[alpine-wayfinding.png]]` | 通过 |
| 打开 Vault 管理 | 显示当前目标、绝对根路径、启停、移除保护和添加表单 | 通过 |
| 检查布局 | 对话框、顶栏、网格与检查器没有遮挡或溢出 | 通过 |

浏览器演示只验证组件、状态与 HTML 拖放；正式系统剪贴板由 Tauri 插件、最小 capability 和 Release 构建验证。

## 7. 整仓质量门禁

```bash
npm run ci
```

结果：通过。覆盖格式检查、Rust Clippy、Rust/TypeScript/Obsidian 全部测试、S 数据集跨模块校验、桌面生产构建、Tauri Release 和 Obsidian 插件构建。

关键结果：

- S 数据集：1,000 素材、200 Sidecar、999 尺寸、1 个损坏图片隔离，素材摘要未变；
- 前端生产构建：36 个模块；CSS gzip 5.50 kB；JavaScript gzip 75.57 kB；
- Tauri Release：`target/release/material-eagle-desktop` 成功生成；
- clipboard-manager Rust 与 JavaScript 版本均固定为 2.3.2。

## 8. 已知限制与阶段边界

- 当前添加 Vault 仍输入绝对路径，原生目录选择器未加入；
- P1-08 对 WikiLink 保留字符采用显式拒绝，不自动重命名文件；
- 当前 UI 对可见结果执行一次批量引用解析，十万级窗口化与分块解析属于 P3-07；
- HTML 文本拖放已经通过 payload 自动化与浏览器卡片状态验收；跨应用拖放的最终兼容矩阵在阶段 1 退出前随当前稳定版与上一稳定版 Obsidian 再执行一次；
- Vault 外素材不会降级为绝对路径或自动复制；正式外部引用属于阶段 4。

## 9. 结论

P1-08 的多 Vault 配置、真实路径归属、完整目录消歧、中文与空格路径、URL 编码字段、标准 WikiLink、最小权限复制、网格文本拖放和失败隔离已实现，并通过自动化、浏览器界面和整仓 Release 门禁。可以进入 P1-09 应用配置与可恢复性。
