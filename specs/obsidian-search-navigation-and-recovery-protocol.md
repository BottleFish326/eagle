# Obsidian 搜索插入、双向定位与恢复协议

本文细化 ADR-034，约束 P4-03、P4-05、P4-06 的实现和 P4-A01/A02/A04-A08 证据。授权、传输、离线素材索引与媒体读取继续遵循 ADR-033；引用语法继续遵循 ADR-005/015/031。

## 1. 术语与运行期对象

```text
InsertionTargetToken {
  editorIdentity,          // 仅进程内对象身份，不序列化
  fileIdentity,            // TFile 对象身份
  filePath,
  selection: { anchor, head },
  documentBytes,
  documentSha256
}

SelectorSnapshot {
  provider: online | offline,
  providerSessionId,
  authorizationRevision,
  catalogRevision,
  queryAst,
  scopeRootIds,
  sort,
  orderedResults[]
}

SelectedAsset {
  assetHandle,             // provider session 内 opaque
  observedVersion,
  stableId?,
  ordinal
}
```

`editorIdentity`、`fileIdentity`、handle、session、revision 和摘要都只驻留内存。`documentBytes` 是 UTF-8 字节数，不是 JavaScript code unit 数。

插件数据可保存一个随机 UUIDv7 `vaultInstanceId` 以及 ADR-033 的根批准记录；不得保存 Vault 绝对路径。插件在握手中发送：

```text
VaultSession {
  vaultInstanceId,
  vaultPathFingerprint,
  displayName,
  capabilities: [backlinks-query, open-note]
}
```

`vaultPathFingerprint = SHA-256("material-eagle-obsidian-vault-v1\0" || platformPathKey)`。它只用于与桌面端已配置 Vault 匹配；displayName 和 fingerprint 只留在活动连接/桌面瞬态 UI，不写支持日志。

## 2. 命令启用与目标捕获

插件注册“Material Eagle: Search and insert assets”。命令只有同时满足以下条件才可执行：

- Obsidian desktop；
- 当前活动 view 是 Markdown view；
- `view.file` 存在且扩展名为 `md`；
- `editor.listSelections()` 恰好一项；
- 当前没有另一个提交中的 selector session。

打开 modal 前同步捕获 editor/file 对象、path、selection 和 `editor.getValue()`，在 worker/Web Crypto 中计算 SHA-256 后才允许确认。计算期间可展示选择器，但 Insert 禁用。多个 cursor 返回 `multiple-selections-unsupported`，不猜测要向哪个位置写入。

## 3. 搜索 Provider

### 3.1 在线

插件通过用户态 IPC 请求：

```text
assets.query {
  queryAst,
  scopeRootIds,
  sort,
  pageSize <= 256,
  cursor?
}
```

响应项仅含 `assetHandle`、稳定 ID（若已有）、name、kind、extension、tags、favorite、rating、尺寸/时长等已授权显示字段、observedVersion 和能力。禁止返回绝对路径或 Sidecar 原文。cursor 绑定连接、查询、scope、sort 和 catalog revision；失效时整页重取，不能拼接两个 revision 的结果。

### 3.2 离线

插件 Phase B 索引实现同一 request/response adapter。它使用同一 parser、query AST、null 语义和排序决胜规则，并运行 [`query-conformance-manifest`](query-conformance-manifest.md) 语料。离线 `assetHandle` 绑定 index generation；重扫后旧 handle 全部失效。

### 3.3 UI 与有界资源

- 输入支持名称自由词以及 Tag、类型、扩展名、收藏等正式谓词；解析错误显示 span/message，不能显示为零结果；
- 结果使用 combobox/listbox 语义、roving active descendant、Space 切换选择、Enter 预检，Escape 关闭；
- 选择状态按 `assetHandle`，筛选隐藏项不参与本次插入；
- 当前可见已选项按结果顺序显示从 1 开始的 ordinal；确认快照之后不再接受页面或排序变化；
- modal 最多保留 512 个已加载结果、128 个选中项、4 个并发预览、128 MiB 活动 Blob；超限明确提示；
- online/offline/index-building/partial/incomplete 状态使用文字和图标，不能只用颜色。

“可见已选项”指确认时 `orderedResults` 中仍匹配当前查询且处于已加载稳定页面的选择；已经被新查询隐藏的选择会先被取消并向用户播报，避免插入不可见旧选择。

## 4. 引用预检

确认时冻结 `SelectorSnapshot` 和选择集，按 `orderedResults` 顺序提取选择项。provider 接收一批：

```text
references.resolve-for-vault {
  vaultPathFingerprint,
  authorizationRevision,
  catalogRevision,
  items: [{ assetHandle, observedVersion }]
}
```

全部项目必须在单次逻辑快照中重新解析：

1. handle 属于当前 provider session/revision；
2. 素材仍唯一、存在且位于批准根；
3. 当前文件版本匹配；
4. 对当前 Vault root 和素材重新 realpath；
5. 若素材在 Vault 内，生成 P1 协议的完整 Vault-relative WikiLink；
6. 若素材在 Vault 外，Sidecar 必须含唯一规范 UUIDv7；
7. alias 取显示名，NFC 后去除 CR/LF/control，反斜杠、`[`、`]` 按 Markdown 转义；空 alias 使用 `asset`；
8. 外部 destination 必须精确为 `material://<uuid>`，不得追加 query、fragment、path 或 token。

返回值只有有序 `markdown[]` 和批次 revision token。任何单项失败都返回有序失败清单，`markdown` 为空。当前版本不向插件开放隐式 `ensure-stable-id`；缺 ID 项显示打开桌面端操作。

## 5. 单事务插入

provider 预检成功后立即重新校验目标：

```text
active MarkdownView exists
&& activeView.editor === captured editor
&& activeView.file === captured file
&& activeView.file.path === captured path
&& editor.listSelections() deep-equals captured selection
&& utf8Length(editor.getValue()) === captured bytes
&& sha256(editor.getValue()) === captured digest
```

任一条件失败返回 `insertion-target-changed`，保留选中清单供用户查看，但 Insert 失效；用户必须从新目标重新运行命令。

成功时：

```ts
editor.transaction(
  { replaceSelection: markdown.join("\n") },
  "material-eagle-insert-v1",
);
```

插件只调用一次 transaction。它不自动添加素材副本、frontmatter、空行或尾随换行；用户原 selection 被整体替换。成功后关闭 modal 并把焦点还给编辑器。一次 Undo 必须完整恢复插入前文档和 selection。

## 6. `material://` Markdown 扫描

### 6.1 产品扫描器

插件随包固定一个 Markdown AST 解析依赖版本。对 `Vault.getMarkdownFiles()` 返回的每个文件调用 `cachedRead`，解析后只遍历 image node：

```text
node.type == image
&& node.url == "material://" + canonicalUuidV7
```

规范 UUID 使用小写连字符形式；非 UUIDv7、大小写变体、额外 slash、host、userinfo、port、percent encoding、query 或 fragment 都忽略并计入安全诊断。AST 自然排除 fenced/inline code、HTML comment 和转义字面量。一个 note 中同一 ID 的多次出现保留独立 source position。

### 6.2 索引结构与事件

```text
ReverseIndex {
  revision,
  byAssetId: Map<UUID, Map<noteKey, Occurrence[]>>,
  byNoteKey: Map<noteKey, Set<UUID>>,
  diagnostics: Map<noteKey, Diagnostic>
}
```

初次扫描按 Vault-relative path 排序，最多 2 个并发读取/解析任务；单 note UTF-8 最大 16 MiB。超限、读取失败或 AST 失败都记录 incomplete，桌面查询必须携带 `complete=false`，不能把遗漏包装为空结果。事件使用 150 ms quiet / 750 ms max batch、1,024 note 上限；溢出执行完整重建。

事件映射：

| 事件 | 行为 |
|---|---|
| metadata `changed` / Vault `create` / `modify` | 移除该 note 旧项后重读并提交新项 |
| metadata `deleted` / Vault `delete` | 移除 note 及全部 occurrence/handle |
| Vault `rename` | 立即使旧 handle 失效，以新 path 重读；不得等待 metadata changed |
| Vault 关闭 / plugin unload | 清空整个 index、队列和 handle |

每个 note 的替换是原子内存提交；失败时删除旧结果并标 incomplete，防止返回陈旧 backlink。

### 6.3 独立验收 Oracle

P4-A08 使用与产品不同的解析实现，在静止 Vault 上从磁盘重新扫描全部 Markdown，并输出排序后的 `(assetId, vaultRelativeNotePath, startOffset, endOffset)`。产品结果先分页取完并按同一 tuple 排序后逐项比较。夹具至少包括：中文/空格路径、多次引用、代码块、inline code、转义、HTML 注释、普通 link、伪造 scheme、query/fragment、rename、delete 和超预算诊断。只有 product `complete=true`、diagnostics=0 且 tuple 完全相等才通过。

## 7. 双向 IPC 与定位

ADR-033 frame 增加 `kind=request|response|event` 和 `direction`，request ID 在单连接双向唯一。只有双方握手声明的 capability 可发起对应请求。

### 7.1 Obsidian → 桌面素材

用户点击已渲染引用的“在 Material Eagle 中显示”：

```text
open-asset { assetId }
```

桌面端重新按授权根和当前 catalog 解析；唯一时打开主窗口并选择素材，not-found/duplicate/unauthorized 明确失败。不得接收路径。若 IPC 离线，用户可明确点击 `material-eagle://asset/<uuid>`；桌面 deep-link parser 只接受该 host/path 形状和空 query/fragment，执行同一只选择流程。

### 7.2 桌面 → Obsidian backlink

桌面内部 UI 请求活动插件连接：

```text
backlinks.query {
  assetId,
  pageSize <= 256,
  cursor?
}
```

插件返回：

```text
{
  vaultSessionId,
  reverseIndexRevision,
  complete,
  diagnosticsCount,
  items: [{
    noteHandle,
    notePath,              // Vault-relative，仅显示
    occurrences: [{ line, column, startOffset, endOffset }]
  }],
  nextCursor?
}
```

`noteHandle` 是至少 128-bit 随机值，绑定 connection/vault session/index revision/note identity，60 秒过期。桌面点击时只发送：

```text
notes.open { noteHandle, occurrenceIndex }
```

插件从 handle 表取回当前 note，验证仍是 Vault 内 Markdown 文件且对应 occurrence 仍存在，然后 `workspace.openLinkText` 打开；取得活动 Markdown editor 后把 selection/cursor 定位到 source span。handle 失效时要求刷新 backlink，不接受桌面提供任意 notePath。

backlink response 可在当前用户 IPC 中包含相对笔记路径，但不含正文、frontmatter、绝对路径或附件。桌面只保留当前面板所需页面，关闭面板/断线即清除；日志只记录数量和散列 session ID。

## 8. 移动、丢失与重新关联

外部引用状态机：

```text
index-building -> resolved | missing | ambiguous | unauthorized | root-offline
resolved --source/watch change--> resolving
resolving -> resolved | missing | ambiguous | unauthorized | root-offline
missing/ambiguous/root-offline --retry/rebuild--> resolving
```

| 文件系统变化 | 插件行为 | 用户恢复路径 |
|---|---|---|
| 素材 + 相邻 Sidecar 移到另一已授权/已批准根 | 撤销旧媒体，Phase A 以唯一 ID 找到新 pair，原引用继续 | 无需修改笔记 |
| 移到未批准或已撤权根 | `unauthorized`，不得读取 | 明确批准根，或移回授权根 |
| 只移动素材 | 原 Sidecar orphan，新素材 unlinked，ID 为 `missing` | 打开桌面 ADR-019 重新关联 |
| 只移动 Sidecar | 原素材 unlinked，移动 Sidecar orphan，ID 为 `missing` | 打开桌面诊断，显式选择 |
| 删除/断开素材或根 | `missing` 或 `root-offline`，不使用旧 lease | Retry、Copy ID、打开桌面诊断 |
| 同一 ID 出现多个有效 pair | `ambiguous`，全部拒绝渲染 | 用户在桌面处理重复；插件不挑选 |
| 桌面重新关联成功且 ID 不变 | watcher/重扫后恢复 `resolved` | Markdown 无需改写 |

占位必须显示稳定错误类别和可用动作；不得显示绝对路径。Retry 只重建/复核索引，Copy ID 只复制 UUID，打开桌面只传 UUID。任何恢复动作都不直接调用 Obsidian 编辑 API。

Vault 内 `![[...]]` 不进入上述状态机。文件移动后是否改写标准链接由 Obsidian 的内部链接维护设置决定，插件只重新解析当前 Markdown，不替用户更改该设置。

## 9. 稳定错误

```text
no-active-markdown-editor
multiple-selections-unsupported
selector-session-stale
query-invalid
provider-offline
index-building
index-incomplete
result-limit
selection-limit
asset-handle-expired
catalog-revision-changed
asset-version-changed
stable-id-required
reference-unsafe
insertion-target-changed
insertion-failed
id-not-found
duplicate-id
root-offline
root-unauthorized
note-too-large
note-parse-failed
backlink-incomplete
note-handle-expired
deep-link-invalid
```

错误文案可以本地化，kind、失败顺序和是否重试必须稳定。

## 10. 验收矩阵

| 用例 | 必须记录的证据 |
|---|---|
| P4-A01 | Vault 内 path/ref、插件关闭后原生渲染、源摘要不变 |
| P4-A02 | Vault 外 UUID/ref、Vault 未出现素材副本、无绝对路径/token |
| P4-A04 | 移动前后同 UUID、原 Markdown 摘要相同、新 root 唯一解析 |
| P4-A05 | 各 missing/ambiguous/offline 状态、动作、Markdown 摘要不变 |
| P4-A06 | 查询 oracle、显示 ordinal、一次 transaction、一次 undo、目标变化拒绝 |
| P4-A07 | IPC 与 cold-start deep link 都只按 UUID 选中正确素材 |
| P4-A08 | product 与独立 parser 的完整 tuple/diagnostic 对比 |

每个用例前后都计算 Markdown、素材和 Sidecar 摘要；只有 P4-A01/A02/A06 的显式 editor transaction 可以改变目标笔记，且 undo 后摘要必须恢复。其他用例零文件写入。

## 11. 实现依据

当前固定的 Obsidian 类型定义提供 `Editor.transaction`/`listSelections`/`getValue`、`Vault.getMarkdownFiles` 与 create/modify/delete/rename 事件、MetadataCache changed/deleted 事件以及 `Workspace.openLinkText`。其中 MetadataCache 文档明确说明 rename 不触发 changed，因此实现必须注册 Vault rename 事件。正式实现仍需在当前稳定版和上一稳定版 Obsidian 实机验证这些契约。
