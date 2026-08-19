# P3-06 拖放与剪贴板实施准备报告

> 状态：Design ready；阶段 2 退出前不计作阶段 3 开始
>
> 日期：2026-08-20

## 1. 准备结论

P3-06 已固定原始文件/引用双拖柄、精确选择顺序、后端路径授权、三平台 native adapter、Copy-only 语义、Vault 内外引用、多格式剪贴板和实机验收。官方 Tauri API 只覆盖窗口拖动与文件拖入，因此原始文件拖出将使用固定 `drag` crate 的本地 Rust 适配层；不会启用接受前端绝对路径的通用插件 IPC。

## 2. 已完成资产

| 资产 | 结论 |
|---|---|
| `specs/adr/031-authorized-native-drag-and-clipboard.md` | 固定双模式、后端授权、依赖边界和最小剪贴板权限 |
| `specs/drag-and-clipboard-protocol.md` | 固定预检、payload、顺序、错误和三平台 A09 验收 |
| ADR-015 / P1-08 | 已有 Vault ID、后端目录键解析、标准 WikiLink 与文本拖放基础 |
| ADR-029 / P3-04 准备 | 已固定精确选择快照、只读预检、稳定 ID 显式创建和单次剪贴板提交 |

## 3. 依赖审计结论与门禁

- Tauri `start_dragging` 明确是窗口拖动，不能满足文件拖出；
- `onDragDropEvent` 是接收外部文件，不是发送；
- `drag` 2.1.1 提供 Windows、macOS、Linux GTK 的文件/数据拖出；
- `tauri-plugin-drag` 2.1.1 的命令允许前端提交绝对 `PathBuf`，不直接采用；
- 实现固定 `drag = "=2.1.1"` 并放在只接收私有授权类型的本地 adapter 后；
- 合并前仍需 `cargo deny`、传递依赖/许可证快照、三平台构建和实机最小 spike。

## 4. 实施切片

### P3-06A：纯 payload 生成器

- 从 selection snapshot 生成有序文件/引用预检；
- Vault 内外、缺失 ID、保留字符和失败项显式处理；
- HTML/Markdown/自定义 MIME 结构化转义测试。

### P3-06B：本地 native-drag adapter

- 精确锁定和审计 `drag` 依赖；
- 私有 `AuthorizedDragPayload`，主线程调用和 callback 回收；
- Copy-only、固定有界图标、不注册通用路径 IPC；
- macOS/Windows/Linux feature 隔离构建。

### P3-06C：桌面交互

- 两个可见、可聚焦拖柄；
- 未选/已选卡片的精确语义；
- 预检失败清单、创建稳定 ID 的独立确认；
- 拖动取消和 capability 降级提示。

### P3-06D：剪贴板格式

- `writeText` 与 `writeHtml` 单次提交；
- HTML/plain fallback 独立接收验证；
- capability 配置只增加 `allow-write-html`；
- 确认没有读取、清空或图片写权限。

### P3-06E：P3-A09 实机矩阵

- 20 文件拖到 Finder、Explorer、Nautilus；
- drop receiver 核对 payload 顺序；
- 中英文 Vault 的 20 条引用插入；
- X11/Wayland、取消、撤权、源变化和无中间副本证据。

## 5. 通过条件

P3-A09 只有在三平台实机证明以下事实后通过：

- 文件 payload 是 20 个当前授权真实路径，顺序和选择快照一致；
- 模式固定 Copy，源路径和 SHA-256 不变，Sidecar 未随拖动交付；
- Obsidian payload 按当前 Vault 生成正确内/外引用并保持顺序；
- 缺失 ID、越界、离线和源变化不会启动过期拖动；
- HTML/文本剪贴板完整一次提交且权限最小；
- 应用没有创建专有素材副本或接受前端任意路径。

## 6. 尚未开始

- 未加入 `drag` 依赖或 native adapter；
- 未增加 `allow-write-html` 权限；
- 未扩展桌面多选拖柄；
- 未执行 P3-A09 三平台实机验收；
- 未把阶段 3 状态改为 In progress。

以上实现等待阶段 2 退出评审完成后开始。
