# P3-03 保存过滤器 / P3-A04/P3-A05 验收报告

- 状态：Accepted
- 日期：2026-08-21
- 最终实现提交：`06736488acfe06ecbddabc27a2c595396fbf6d3a`
- 正式证据：`evidence/p3-a04-a05-filter-gates.json`
- 证据 SHA-256：`4f1b2f909e4f908eed9aa658651a41ca8eb12fd0bc1309c64a5476aa9afdd76b`
- 非结论：不代表 P3-04 至 P3-07、P3-A06 至 P3-A11 或阶段 3 退出

## 1. 验收结论

P3-03 与 P3-A04/P3-A05 的固定条件均已满足。保存项只包含名称、查询、root scope、排序和
时间字段；执行时从当前文件与 Sidecar 重扫并重新解析查询，不持久化素材键、路径或结果
列表。`saved-filters.yml`、相邻 Sidecar、metadata transaction 和 `tag-renames-v1` 协调日志
都是普通可检查文件，没有新增数据库或权威索引。

桌面端已经提供保存过滤器的创建、编辑、改名、删除和执行入口。Tag 重命名的产品核心提供
精确 AST 预检、逐 filter update/retain 选择及跨 Sidecar/filter 恢复；生产级 Tag rename UI
仍按 P3-07 实施，不在本报告中提前关闭。

## 2. 可读保存格式与当前文件执行

`SavedFilterStore` 使用固定用户级 `saved-filters.yml`：

- Schema 1、最多 512 项、文件上限 1 MiB；
- UUIDv7 身份、Unicode 折叠名称唯一性和稳定 scope/sort；
- 读取前后复核 mtime/大小，写入以完整版本和 SHA-256 做乐观并发；
- 同目录临时文件、文件同步、原子替换和父目录同步；
- 单条无效、重复 ID/名称、未知排序和离线 root 分别隔离；
- 未知顶层字段与条目字段在有效编辑后继续保留；
- 查询执行只使用当前扫描产生的 `AssetRecord`，结果仅存在于运行期。

桌面 UI 对正常、不可用和无效项分别显示，文件整体无效时保持原文件不变。浏览器实测完成
创建与激活保存项，16 条当前记录能够即时重算，控制台无错误。

## 3. 精确 Tag AST 影响与重写

查询 parser 为 AND、显式 `tag:`、排除 Tag 和 `any:(...)` 成员返回 UTF-8 byte span 与节点
类型。重写只匹配语义值完全相等的 Tag，字段值、路径、自由文本、同名前缀和普通 exact
操作下的 namespace wildcard 不受影响。

替换按 span 从后向前执行，并在写入前重新解析、审计 AST 等价关系。无效 query 只进入
retain-only 诊断；可更新项必须逐项明确选择 update 或 retain，重复、遗漏或伪造选择都会
被拒绝。计划保存原始/计划完整 YAML 字节和两份 SHA-256，可在应用重启后继续使用。

## 4. Sidecar/filter 两阶段协调

`asset-tag-renames` 在固定私密目录 `tag-renames-v1` 保存 UUIDv7 YAML 日志。执行顺序为：

1. 生成 saved-filter 原始/计划字节，但不写；
2. 以 plan-only 方式完整保存 metadata transaction，尚不改 Sidecar；
3. 原子保存引用 transaction 的 coordinator；
4. 逐 Sidecar 移除 old、加入 new，目标已存在时按集合自然合并；
5. 以原 `saved-filters.yml` 完整版本为前置条件写计划字节；
6. 从 transaction 摘要和 filter 当前完整字节重建 `planned`、`sidecars-active`、
   `filters-pending`、`completed`、`conflict` 或 `restored`。

恢复支持继续、保留原 filter query 和条件恢复。filter 或 Sidecar 发生外部变化时状态转为
conflict，当前外部字节不被覆盖；恢复日志只有用户显式清理时才删除，清理不会触碰素材、
Sidecar 或 filter。

## 5. P3-A04：重启、删缓存与异常文件

正式 Release 门禁在独立进程中创建 4 个普通 SVG、4 个相邻 Sidecar、all-enabled 与
selected-roots 两类保存项，以及一个模拟派生缓存。随后第二个进程新增第 5 个当前文件，
验收器删除全部派生缓存，再由第三个进程正式重扫：

- 扫描 5 项、问题 0；
- all-enabled 当前匹配 4 项；
- selected-roots 当前匹配 5 项，并准确报告 1 个离线 root；
- 保存文件无 asset key、relative path、result、thumbnail 或 index snapshot；
- 5 个原素材 SHA-256 均与确定性输入一致。

对抗文件同时注入 invalid query、duplicate ID、duplicate name、unknown sort、未知字段和
离线 root。结果为 1 个合法项、1 个不可用项和 6 个隔离无效项；合法项仍可改名，未知字段
保留。外部追加字段后，旧版本写入被拒绝且外部字节逐字节保持。

## 6. P3-A05：真实进程终止与外部变化

正式门禁每次使用 64 个普通素材和相邻 Sidecar，所有终止均为 Release 二进制真实
`SIGABRT`：

| 终止点 | 恢复动作 | 最终状态 |
|---|---|---|
| coordinator 已落盘、Sidecar 未开始 | restore | restored |
| 第 17 个 Sidecar 落盘后、检查点前 | continue | completed / updated |
| 全部 Sidecar 完成、filter 阶段未开始 | restore | restored |
| filter 原子替换前 | retain | completed / retained |
| filter 原子替换后、完成检查点前 | restore | restored |

另有两个外部变化用例：filter 替换前由外部编辑 YAML，以及 filter 替换后由外部编辑一个
Sidecar。两者恢复均准确进入 conflict，外部标记保留。全部 7 个用例的 64 个原素材
SHA-256 均不变；目标 Tag 已存在的 Sidecar 最终仍只有一个集合成员。

## 7. 证据完整性与隐私

正式 JSON 绑定实现提交 `0673648`、Node.js 24.19.0、Rust/Cargo 1.97.1 和 Release 二进制
SHA-256。Schema 为 strict draft 2020-12 accepted-only；离线 inspector 会拒绝字段、顺序、
摘要、终止状态和恢复结果篡改。工具测试 116/116 通过。

证据只包含固定 root/operation UUID、计数、状态、工具链版本和进程 stdout/stderr 摘要；
不含绝对路径、文件名、query、Tag、Sidecar 正文或临时目录。证据生成后执行 Schema、
SHA-256 与敏感信息审计均通过，临时工作区全部删除。

P3-03/P3-A04/P3-A05 判定为 **Accepted**。阶段 3 继续进入 P3-04/P3-A06；P3-A06 至
P3-A11 仍未通过，阶段 3 不退出。
