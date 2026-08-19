# 原生拖放与剪贴板输出协议

本文定义 P3-06 的两种拖出模式、多选顺序、授权复核、剪贴板格式和三平台验收。阶段 2 退出后实施。

## 1. 能力边界

当前 Tauri 2 官方 API 的 `start_dragging` 是窗口移动；`onDragDropEvent` 是文件拖入监听。原始文件拖出通过项目本地 Rust 适配层调用固定版本 `drag` crate；不得把接受任意路径的通用 IPC 暴露给 WebView。

运行期能力：

```text
DragCapabilities {
  nativeFiles: available | unavailable,
  referenceText: available | unavailable,
  htmlClipboard: available | unavailable,
  platform,
  sessionType?: x11 | wayland | other,
  reasonCode?
}
```

capability 不可用时保留复制路径/引用按钮并说明原因，不以假成功、窗口拖动或下载临时副本代替。

## 2. 选择与顺序

- 拖动未选中卡片时只创建该卡片快照；
- 拖动已选中卡片时使用整个当前精确选择；
- 顺序固定为当前视图排序，而不是点击顺序、路径字典序或文件系统遍历顺序；
- 20 项是 P3-A09 强制基线；更大选择先显示数量和平台能力，不静默截断；
- 虚拟网格只需渲染拖动起点，快照成员可以不在当前窗口内。

预检返回：

```text
DragPreflight {
  id,
  snapshotId,
  mode: native-files | references,
  itemCount,
  vaultId?,
  requiresStableIdCount,
  unavailableCount,
  estimatedTextBytes?,
  failures[]
}
```

拖动开始前 confirmation 绑定该预检。目录或文件版本变化时重新预检，不使用旧路径。

## 3. 原始文件拖出

后端构造私有类型：

```text
AuthorizedDragPayload {
  operationId,
  mode: copy,
  canonicalFiles: Vec<AuthorizedFile>,
  icon: BoundedDragIcon
}

AuthorizedFile {
  assetKey,
  rootId,
  canonicalPath,
  observedSize,
  observedModifiedUnixMs,
  observedQuickFingerprint
}
```

调用原生库前再次检查：

1. snapshot/preflight 未过期；
2. 根仍启用、在线且 canonical path 在根内；
3. 路径是当前目录记录对应的普通文件，不是符号链接；
4. size/mtime/快速指纹与预检一致；
5. 列表非空、无重复、顺序未改变；
6. `DragMode` 固定 Copy。

任何预检失败都在 native drag 启动前停止。callback 后再次检查 size/mtime/快速指纹；若拖动期间变化，返回 `source-changed-after-start`，但不能撤回目标应用已经持有的原生 payload。成功 drop 不解释为产品复制成功；文件管理器或目标应用负责目的地行为。产品只证明提交的源列表正确以及本应用没有修改源文件。

## 4. Obsidian/文本引用拖出

引用模式先选择一个已启用 Vault：

- 素材真实路径位于 Vault 内：标准 `![[完整/相对路径.ext]]`；
- Vault 外且存在稳定 ID：`![alias](material://<uuid>)`；
- Vault 外且缺少 ID：`stable-id-required`，在单独确认的事务完成前不可开始拖动；
- 保留字符、越界、离线或歧义逐项失败，不默认省略。

用户可选择“只拖动成功项”，该选择生成新的精确预检集合。最终 `text/plain` 和 `text/markdown` 内容完全相同，每项一行，以 `\n` 连接且无尾随空行。20 项次序必须与快照一致。

自定义 MIME `application/x-material-eagle-reference+json` 是可选优化，Schema 只含：

```json
{
  "schema": 1,
  "vaultId": "<uuid>",
  "items": [
    {
      "stableId": "<uuid-or-null>",
      "relativePath": "<vault-relative-or-null>",
      "markdown": "<reference>"
    }
  ]
}
```

禁止 `assetPath`、授权根路径和文件内容字段。Obsidian bridge 可识别自定义类型以保留顺序，但没有插件时标准文本仍可粘贴/拖入。

## 5. 剪贴板

批量输出先由后端完整生成，前端只能选择稳定输出类型：

| 类型 | 系统写入 | plain fallback |
|---|---|---|
| 路径 | `writeText` | 同内容 |
| Markdown | `writeText` | 同内容 |
| 稳定引用 | `writeText` | 同内容 |
| HTML image | `writeHtml` | 同顺序 Markdown |

HTML 每项固定为：

```html
<img src="ENCODED_REFERENCE" alt="ESCAPED_ALIAS">
```

Vault 内 `ENCODED_REFERENCE` 是相对路径；Vault 外是 `material://<uuid>`。输出不包含 CSS、事件属性、脚本、`data:`、`blob:`、绝对路径或令牌。所有项准备成功并满足内存/大小预算后只调用一次系统 API；准备失败不会调用剪贴板。系统 API 自身失败时返回 `clipboard-unavailable`，由于不申请读取权限，不承诺恢复操作系统已经改变的剪贴板状态。

权限清单只增加：

```text
clipboard-manager:allow-write-text
clipboard-manager:allow-write-html
```

明确不授予 read、clear、write-image。

## 6. 结果与错误

```text
drag-completed
drag-cancelled
native-drag-unavailable
snapshot-expired
preflight-stale
asset-missing
source-changed
source-changed-after-start
root-offline
authorization-lost
symlink-not-followed
stable-id-required
vault-unavailable
unsafe-reference
payload-too-large
platform-error
clipboard-unavailable
```

日志只记录操作 ID、模式、数量、耗时、平台和错误种类。绝对路径、Vault 路径、引用文本和拖放坐标不进入持久支持日志。

## 7. P3-A09 三平台验收

每个平台使用 20 个带固定顺序标识、Unicode/空格/同名目录的素材：

1. 拖到系统文件管理器的隔离测试目录，核对目标文件名/字节和源素材 SHA-256/路径不变；
2. 用测试 drop receiver 记录 native payload 次序，证明后端提交的 20 个路径顺序正确；文件管理器自身的展示排序不作为 payload 顺序；
3. 拖入当前 Vault 笔记，Vault 内项为 WikiLink、Vault 外项为 stable reference，Markdown 行顺序准确；
4. 禁用 Obsidian bridge 后，Vault 内引用仍可渲染，外部引用保留可读源码；
5. 取消拖动、drop 期间删除、撤权、符号链接逃逸和缺失稳定 ID 均安全失败；
6. 证明没有产品临时素材目录、没有 Sidecar 随文件拖出、没有 Move；
7. 复制 HTML 后由独立 clipboard receiver 核对 HTML 和 plain fallback，权限快照确认没有 read/clear/write-image。

Windows 使用 Explorer/NTFS，macOS 使用 Finder/APFS，Linux 至少记录 Nautilus/ext4 的实际桌面 session；X11 与 Wayland能力分别记录。自动 receiver 与人工应用证据缺一不可。

## 8. 依据

- [Tauri Window `start_dragging`：拖动窗口](https://docs.rs/tauri/latest/tauri/window/struct.Window.html#method.start_dragging)
- [Tauri `onDragDropEvent`：监听拖入](https://v2.tauri.app/reference/javascript/api/namespacewindow/#ondragdropevent)
- [`drag` 2.1.1：macOS、Windows、GTK 原生拖出](https://docs.rs/drag/2.1.1/drag/)
- [`tauri-plugin-drag` 2.1.1 command source](https://docs.rs/crate/tauri-plugin-drag/2.1.1/source/src/commands.rs)
- [Tauri Clipboard plugin](https://v2.tauri.app/plugin/clipboard/)
