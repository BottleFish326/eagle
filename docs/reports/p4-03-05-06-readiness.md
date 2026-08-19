# P4-03/P4-05/P4-06 搜索插入、双向定位与恢复实施准备报告

> 状态：Architecture ready；阶段 3 退出前不计作阶段 4 开始
>
> 日期：2026-08-20

## 1. 准备结论

Obsidian 选择器的在线/离线查询一致性、稳定顺序、多选引用预检、单事务 undo、异步目标防误写、`material://` AST 反向索引、双向定位以及移动/丢失恢复已经定案。反向索引只驻留插件内存；桌面端不保存笔记内容或 Vault 绝对路径；除用户确认插入外，扫描、定位和恢复都不修改 Markdown。

## 2. 已完成资产

| 资产 | 结论 |
|---|---|
| ADR-034 | 固定 revision-bound 选择器、临时 backlink 和无改写恢复 |
| `specs/obsidian-search-navigation-and-recovery-protocol.md` | 固定 provider、目标 token、单 transaction、AST 扫描、双向 IPC、状态机与验收证据 |
| ADR-033/runtime protocol | 提供机器级授权、在线 socket/pipe、离线 Phase A/B 和安全媒体读取基础 |
| ADR-005/015/019/028/031 | 提供两类引用、Vault 边界、显式重新关联、查询和批量引用语义 |
| 固定 Obsidian typings | 已核实 transaction、selection、Vault/Metadata 事件和 openLinkText API；rename 必须单独监听 |

## 3. 已关闭的歧义

| 问题 | 决策 |
|---|---|
| 弹窗期间用户切换/编辑笔记 | editor/file/path/selection/full-document digest 任一变化均拒绝插入 |
| 多选顺序 | 确认时可见结果的稳定排序顺序，并在 UI 显示 ordinal |
| undo 粒度 | 所有引用通过一次 `Editor.transaction({ replaceSelection })` 写入 |
| 在线/离线差异 | 同一 AST/排序/语料；provider 和 incomplete 状态明确显示 |
| 外部素材没有稳定 ID | 禁止插入并打开桌面处理，不在只读搜索中写 Sidecar |
| backlink 解析真相 | 固定 Markdown AST image destination；不只依赖 Obsidian cache |
| 桌面如何打开笔记 | 短期 noteHandle 回传，桌面不能提交任意笔记路径 |
| 插件如何启动/定位桌面 | 在线 `open-asset`，离线用户动作触发只含 UUID 的受限 deep link |
| 成对移动 | 在已批准根中按唯一稳定 ID 重新解析，Markdown 不变 |
| 单边移动或多候选 | 只显示状态/入口，交由 ADR-019 显式恢复，绝不自动改写 |

## 4. 实施切片

### P4-03A：Provider adapter 与选择器

- 在线 IPC/离线 Phase B 统一查询接口；
- query AST、scope、sort、cursor/revision 一致性；
- keyboard listbox、多选 ordinal、preview 和资源上限；
- provider/index/parse error 的非静默状态。

### P4-03B：引用预检与编辑器事务

- `InsertionTargetToken` 捕获和 SHA-256 复核；
- 当前 Vault fingerprint 匹配和两类引用批量解析；
- stable ID required/unsafe path/all-or-nothing failures；
- 一次 transaction、一次 undo、目标变化拒绝测试。

### P4-05A：Markdown reverse index

- 固定 AST parser、精确 UUIDv7 image destination；
- 初始 Vault 扫描、note 双向表、位置和 duplicate occurrence；
- create/modify/delete/rename 增量更新与溢出重建；
- 16 MiB note/incomplete 诊断和不返回陈旧结果。

### P4-05B：双向 IPC 与定位

- 握手 Vault session/fingerprint/capabilities；
- `open-asset` 与只含 UUID 的 desktop deep link；
- server-initiated paginated `backlinks.query`；
- 60 秒 opaque noteHandle 与 Obsidian open/cursor 定位；
- 断线、revision 变化和多 Vault session 隔离。

### P4-06A：恢复状态组合

- 成对跨已批准根移动；
- 素材/Sidecar 单边移动、删除、根离线/撤权、重复 ID；
- resolving 时撤销旧 media lease；
- Retry/Copy ID/Open desktop 动作和零 Markdown 改写。

### P4-03/05/06B：独立验收

- 独立 query oracle 和 Markdown parser oracle；
- target-switch/edit、分页 revision、事件溢出和 stale handle 故障注入；
- P4-A01/A02/A04-A08 全链路摘要；
- 两版 Obsidian、中英文/空格路径和多 Vault 实机证据。

## 5. 通过条件

- P4-A01/A02/A06 的引用文本、顺序、单次 undo 和目标保护全部通过；
- online/offline 对相同已索引素材输出相同查询结果和引用类型；
- P4-A08 product tuple 与独立 Markdown scanner 完全相等且无 incomplete 诊断；
- desktop/plugin 双向请求都不能注入任意绝对素材或笔记路径；
- P4-A04/A05 的移动、缺失、撤权和歧义状态不使用旧文件、不自动选候选；
- 除显式插入目标笔记外，Markdown、素材和 Sidecar 摘要保持不变；undo 后插入笔记摘要恢复；
- plugin unload/断线清除 selector、handle、reverse index、preview/lease 和连接资源；
- 未新增数据库、IndexedDB、LocalStorage、持久素材结果或 backlink snapshot。

## 6. 尚未开始

- 未实现搜索 modal/provider adapter 或编辑器 transaction；
- 未引入正式 Markdown AST scanner/reverse index；
- 未扩展双向 IPC、desktop deep link 或 noteHandle；
- 未实现恢复占位与组合测试；
- 未执行 P4-A01/A02/A04-A08；
- 未把阶段 4 状态改为 In progress。

以上实现等待阶段 3 退出评审后开始。
