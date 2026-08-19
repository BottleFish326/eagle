# ADR-031：后端授权的原生拖出与多格式剪贴板

- 状态：Accepted
- 日期：2026-08-20
- 对应：P3-06、P3-A09
- 实施门禁：阶段 2 退出后

## 背景

阶段 1 已支持把单个 Vault 内 WikiLink 作为 Web `DataTransfer` 文本拖入 Obsidian，但 P3-06 还要求把 20 个真实素材拖到 Finder、Explorer、Linux 文件管理器，并按当前 Vault 关系向 Obsidian 插入多条引用。Tauri 2 的官方 `start_dragging` 只拖动应用窗口，`onDragDropEvent` 只监听拖入；它没有官方的文件拖出命令。

第三方 `tauri-plugin-drag` 2.1.1 能启动三平台拖出，但其公开 IPC 直接反序列化前端提供的绝对 `PathBuf` 列表。直接授予该命令会绕过本项目“前端只提交目录键/不透明 ID、后端解析并复核授权根”的安全边界。

文件拖放与引用拖放也不能靠猜测目标应用来切换：把文件 payload 拖到 Obsidian 可能触发附件复制，而把 `material://` 文本拖到文件管理器不会交付原文件。用户需要在拖动前看到并选择明确语义。

## 决策

1. 产品提供两个独立、可键盘聚焦的拖柄：“拖出原始文件”和“拖出引用”。不根据鼠标位置、进程名或最终 drop target 猜测模式；拖柄、预检摘要和光标反馈始终说明本次 payload。
2. 两种模式都先使用 ADR-029 的精确 `SelectionSnapshot`。前端只提交 snapshot ID、模式、当前 Vault ID 和预检 confirmation；Rust 后端按固定顺序取回记录、canonicalize 当前路径、复核根授权/普通文件/非符号链接和文件版本。
3. 原始文件拖出使用本地 `native-drag` 适配层，直接固定依赖 `drag = "=2.1.1"`，不注册 `tauri-plugin-drag` 的通用 JS command。适配层只接受后端已经验证的私有 `AuthorizedDragPayload`，并在主线程调用原生拖放。
4. 原始文件模式固定 `Copy`，不暴露 Move。目标应用可以按用户明确 drop 复制文件，但本应用不创建中间副本、不移动/改名源文件、不复制 Sidecar、不把素材放进产品目录。drop 前后源素材 SHA-256 和路径必须不变。
5. 引用模式由后端为当前显式 Vault 逐项解析：Vault 内生成标准 `![[relative/path]]`，Vault 外生成 `![alias](material://<uuid>)`。缺少稳定 ID 的外部素材必须在拖动前按 ADR-029 显式创建；拖动过程中不弹出写入确认，也不静默创建 Sidecar。
6. 引用 payload 同时提供 UTF-8 `text/plain` 和 `text/markdown`，内容为按快照顺序、每项一行的完整引用。自定义类型只允许包含 schema、Vault ID、素材稳定 ID/根内相对路径和引用文本，不含绝对路径、访问令牌或源文件字节。
7. 原生拖出图标使用应用打包的固定图标或后端生成的有界合成图（最多 256×256、1 MiB、显示数量），不接受前端传入 base64/任意图像路径，不为拖动读取超大原素材。
8. 结果只报告 `dropped`、`cancelled`、`failed` 和稳定错误；目标应用身份及其是否最终复制/重排文件不可可靠推断，不作为权限依据。callback 后重新检查源文件版本；开始拖动后的变化只能报告 `source-changed-after-start`，不能声称已撤回目标应用持有的原生 payload。适配层结束后释放路径列表、图标和 callback session。
9. 剪贴板沿用 ADR-029 的完整预检和单次提交。文本类型使用 `writeText`；HTML 图片片段使用官方 `writeHtml(html, altText)` 并只新增 `clipboard-manager:allow-write-html`。不申请读取、清空或写图片权限。
10. HTML 输出在当前 Vault 内使用 RFC 3986 编码的 Vault 相对 `src`，Vault 外使用稳定 `material://`；属性值严格转义，不允许脚本、事件属性、`data:` 字节或任意用户 HTML。plain-text fallback 是同顺序 Markdown 引用。
11. 依赖进入实现前必须通过许可证/来源审计、精确锁定和三平台最小拖出 spike；Linux 同时覆盖 X11 与 Wayland 可用性记录。任一平台不支持时显示明确 capability，不把文本拖放或窗口拖动冒充文件拖出。

## 影响

- 前端和任意 Web 内容都不能把绝对路径注入通用拖出命令。
- 用户能明确选择“交付原文件”或“插入引用”，避免 Obsidian 意外复制附件。
- 文件管理器接收真实源路径，复制行为由用户 drop 决定；应用自身仍不重构素材目录。
- HTML/Markdown/文本剪贴板共用后端生成和转义，权限保持只写、最小化。
- 三平台原生拖出成为需要实机证据的发布能力，不能只靠 jsdom 或 headless 单元测试验收。

## 不采用

- 把 Tauri 的窗口 `start_dragging` 当成文件拖出；
- 直接启用接受前端绝对路径的第三方插件 IPC；
- 根据目标应用猜测文件/引用模式；
- 允许 Move 模式或拖动 Sidecar；
- 为跨应用拖动创建产品目录中的临时素材副本；
- 在拖动开始后静默生成稳定 ID；
- 请求剪贴板读取、清空或图片写入权限；
- 把绝对本机路径、文件字节或未转义 HTML 放入引用 payload。
