# ADR-034：版本绑定的 Obsidian 插入、临时反向索引与无改写恢复

- 状态：Accepted
- 日期：2026-08-20
- 对应：P4-03、P4-05、P4-06、P4-A01、P4-A02、P4-A04 至 P4-A08
- 实施门禁：阶段 3 退出后

## 背景

Obsidian 素材选择器会跨越异步搜索、预览和多选。如果用户在弹窗打开期间切换笔记、移动文件或编辑原选择区，直接调用编辑器写入可能把引用插到错误笔记。在线桌面查询与离线插件索引也不能因为返回字段相似就产生不同排序、引用类型或稳定 ID 规则。

反向引用需要读取 Vault 中的 Markdown，但不能把 Obsidian 的解析缓存当作唯一真相，也不能把笔记正文、绝对 Vault 路径或一份长期 backlink 数据库交给桌面端。文件移动和丢失恢复同样不能靠插件猜测候选、静默改写 Markdown 或把旧路径当作稳定身份。

## 决策

1. “搜索并插入素材”命令只在恰好一个活动 Markdown 编辑器和一个 selection/caret 时启用。打开选择器时捕获编辑器实例、`TFile` 实例与路径、selection、文档长度和 SHA-256，形成 `InsertionTargetToken`；不保存为插件数据。
2. 选择器在线时调用 ADR-033 的当前用户 IPC，离线时调用 Phase B 内存索引。两种 provider 都执行 ADR-028 的同一查询 AST、作用域、排序和 conformance corpus；界面明确显示 online/offline/index-building，不把断线静默解释为零结果。
3. 查询结果返回短期 opaque `assetHandle`、显示字段和 catalog revision，不返回绝对路径。多选在当前稳定结果顺序中编号；插入顺序就是确认瞬间的可见结果顺序，不使用点击先后，也不在确认后重新排序。
4. 确认前对全部选择执行一次引用预检。当前 Vault 内素材生成带完整消歧路径的标准 `![[vault-relative-path]]`；Vault 外素材只生成规范 `![escaped-alias](material://<uuid>)`。外部素材缺少稳定 ID 时禁止插入并提供“在 Material Eagle 中创建 ID”入口，不在只读查询中写 Sidecar。
5. 引用预检绑定 provider、授权 revision、catalog/index revision、当前 Vault path fingerprint 和每项 observed version。任一项目失效、歧义、越权或不可安全编码时整批失败；不部分插入，不复制素材进 Vault。
6. 提交前重新取得活动 Markdown view，并逐项核对 `InsertionTargetToken`。编辑器、文件对象或路径、selection、文档长度或文档 SHA-256 任一变化，都关闭写入路径并要求用户从当前光标重新打开选择器。
7. 通过预检后，把按顺序生成的引用以 `\n` 拼接，并通过一次 `Editor.transaction({ replaceSelection })` 写入，固定 origin `material-eagle-insert-v1`。一次确认对应一次 Obsidian undo；插件不调用 Vault 文件级改写 API 绕过编辑器历史。
8. 插件使用固定版本、随包分发的 Markdown AST 解析器扫描实际 image destination。只有 destination 精确等于 `material://<canonical-uuid>` 的 image node 才进入反向索引；代码、转义文本、HTML 注释、普通正文和带 host/path/query/fragment 的伪造值不计入。
9. 反向索引是 `assetId -> note entry/occurrence` 和 `note -> assetId` 的纯内存双向表。初次扫描来自 `Vault.getMarkdownFiles()`；后续监听 metadata changed/deleted 以及 Vault create/modify/delete/rename。rename 必须独立处理，不能假设 metadata changed 会触发。解析失败或超预算笔记产生显式 incomplete 诊断，不能返回“完整”结果。
10. 桌面端按精确 asset ID 请求 backlink 时，插件只返回当前 Vault 的相对笔记路径、位置、出现次数、index revision 和短期 `noteHandle`，不返回正文或绝对路径。桌面端点击结果只回传 handle；插件重新校验连接、revision 和当前 `TFile` 后调用 Obsidian workspace API 打开并定位。
11. Obsidian 引用打开桌面素材时优先调用 IPC `open-asset`。桌面未运行时可由用户动作启动 `material-eagle://asset/<uuid>`；该 OS deep link 只允许规范 UUID 和空 query/fragment，只具有“打开应用并选择当前唯一素材”的能力，不能读取、导出、写元数据或携带路径。
12. 素材与相邻 Sidecar 一起移动到另一个当前授权且已批准的根后，Phase A 以稳定 ID 解析唯一新位置，原 Markdown 不变。事件收敛期间停止提供旧 lease，并显示 resolving，而不继续读取旧路径。
13. 只移动素材、只移动 Sidecar、删除、根离线、授权撤销和重复 ID 是不同恢复状态。插件只显示 Retry、Copy ID 和打开桌面恢复入口；候选匹配与 Sidecar 重新关联完全委托 ADR-019 的显式桌面流程，多候选永不自动选择。
14. 成功重新关联保持原稳定 ID，因此 Markdown 和 backlink key 都不改写。Vault 内标准引用的文件移动完全遵循 Obsidian 自身链接维护设置；Bridge 不拦截、不补写、不承诺在用户关闭 Obsidian 自动链接更新时修复路径。
15. backlink 扫描、查询、定位、渲染和恢复不得持久化素材结果、笔记正文或绝对路径。插件禁用/卸载、Vault 关闭或根撤销会清空相应内存索引、handle、预览和连接，但不会修改 Markdown、素材或 Sidecar。

## 影响

- 异步选择器无法把结果写进已切换或已变化的笔记；代价是编辑期间需要重新打开选择器。
- 在线和离线搜索共享语义，差异只体现在可用数据与稳定 ID 创建能力，并在界面可见。
- 多选插入顺序和 undo 边界可以自动验收，不依赖点击速度。
- 反向引用可从 Vault 与文件系统重建；桌面端仅在插件在线时看到当前结果，不拥有第二份 Vault 索引。
- 移动恢复以稳定 ID 和相邻 Sidecar 为依据，不会因为相似文件名、哈希唯一或旧路径而静默指错素材。

## 不采用

- 弹窗关闭时向“当前活动编辑器”无条件插入；
- 将每个选中项分别调用 `replaceSelection` 或文件级 append；
- 用点击先后、异步返回先后或前端对象枚举顺序决定批量插入顺序；
- 为缺少 ID 的外部素材在后台静默创建 Sidecar；
- 只依赖 `MetadataCache.resolvedLinks`/`embeds` 作为 `material://` 反向引用真相；
- 把完整笔记内容、绝对 Vault 路径或持久 backlink 快照传给桌面端；
- 以路径、文件名、mtime 或唯一哈希自动改写引用；
- 在恢复期间修改 Markdown 以追随当前候选。
