# P2-05 缓存生命周期验收报告

- 状态：Passed locally
- 日期：2026-08-19
- 对应：P2-05、P2-A10
- 决策：[ADR-022](../../specs/adr/022-bounded-thumbnail-cache-lifecycle.md)

## 1. 交付范围

本工作项完成以下链路：

- 缓存默认限制为 20,000 个条目、PNG 与描述文件合计 1 GiB、最后使用后 30 天；
- 命中更新缓存 PNG 的 mtime，以文件系统实现 LRU，不使用数据库或权威清单；
- 每项使用 `<key>.png` 与 `<key>.json` 文件对，描述记录不可逆源令牌、解码器版本和请求尺寸；
- 容量估计越界时立即维护，正常生成按最多 64 次写入的有界间隔维护；
- 启动回收不完整、旧解码器、过期和超容量项；所有可用根至少完整扫描成功且并行扫描结束后再按内存目录回收孤立项；
- 设置页显示当前用量、上限、保留周期与解码器，并提供不清空有效项的手动回收；
- 全量清理使用带所有权标记的同级目录轮换，清理中断后可在启动时安全收敛；
- 未启用扫描快照，目录和索引仍只存在于运行期，可由素材与 Sidecar 重建。

没有引入 SQLite、IndexedDB、LocalStorage 或其他权威缓存索引。JSON 描述不包含素材路径、Tag、评分、收藏、备注、别名或 Sidecar 内容。

## 2. 文件布局与精确失效

布局标记升级到 v2，固定根目录名继续为 `thumbnails-v1`：

```text
thumbnails-v1/
  .material-eagle-thumbnail-cache
  ab/
    <64hex>.png
    <64hex>.json
```

缓存键继续包含路径键、稳定 ID、源大小、完整 mtime、请求长边和解码器版本。JSON 中另存同一解码器版本及源令牌，维护无需反查或保存原路径即可判断版本兼容与当前目录归属。升级解码器只回收描述为旧版本的项；布局不兼容才执行整版轮换。

PNG 和 JSON 都以同分片临时文件写入、同步并原子持久化。缺少任一文件、描述损坏、键/尺寸/源令牌不匹配或 PNG 无法读取时不会命中，并在请求或维护时删除派生残项。

## 3. 维护时机与回收顺序

维护按以下顺序归类：

1. 临时文件、不完整文件对、损坏描述和未知文件；
2. 解码器版本不兼容项；
3. 不属于完整内存目录源令牌集合的孤立项；
4. 最后使用超过 30 天的过期项；
5. 按 PNG mtime 从旧到新回收，直到条目数和总字节都在限制内。

启动不使用目录快照判断孤立项，因为目录尚未完成扫描。后端扫描协调器记录本次进程中已完整扫描成功的根；只有全部启用且可用根都具备权威目录、同时没有活动扫描时才执行孤立回收。失败或取消不授予目录权威，防止极小素材根先完成时误用部分目录；手动命令分别以 `recovery-busy` 或 `recovery-incomplete` 拒绝活动扫描和未完成根。诊断只记录触发方式与计数，错误只记录类别，不写入路径。

## 4. 中断安全清理

清理持有缓存排他锁并执行：

1. 验证固定父目录、固定根名、真实目录和正式保护标记；
2. 在根内创建并同步专用 tombstone 所有权标记；
3. 把完整根原子改名为 `.material-eagle-thumbnail-cache-gc-<UUIDv7>` 并同步父目录；
4. 创建、验证并同步新的正式空根；
5. 删除旧 tombstone 并再次同步父目录。

启动只删除 UUID 合法且专用标记内容匹配的 tombstone。自动化另建相似名称但没有标记的目录，重启后其中的私有测试文件保持不变，证明名称前缀本身不构成删除授权。

## 5. P2-A10 真实进程终止验收

专用 `cache-fault` CLI 创建真实 PNG 和合法 Sidecar，记录二者 SHA-256，生成一个缩略图，再在两个持久化边界直接调用 `abort`：

```text
EAGLE_CACHE_FAULT_POINT=after-cache-rename cache-fault clear <workspace>
exit=134
cache-fault recover <workspace>
recovered disposition=maintained cache=1 asset=true sidecar=true

EAGLE_CACHE_FAULT_POINT=after-cache-recreate cache-fault clear <workspace>
exit=134
cache-fault recover <workspace>
recovered disposition=maintained cache=1 asset=true sidecar=true
```

两个用例都确认：启动成功、旧缓存条目为零、可见请求重新生成而非命中、重建后条目为一、原图摘要不变、Sidecar 摘要不变。测试临时目录验收后已移动到系统废纸篓。

阶段 2 候选退出时，这两处崩溃会与 P2-A04 一起由 `npm run test:p2-local-fault-gates` 重新执行。执行器对 `after-cache-rename`、`after-cache-recreate` 各要求且只允许一个用例，核对 seed/abort/recover 的真实进程状态与恢复输出，记录 Release 二进制 SHA-256，并只在两例都通过且精确临时根已删除后发布不可覆盖的 `evidence/p2-local-fault-gates.json`。当前执行器、纯判定测试与 [`p2-local-fault-gates.schema.json`](../../schemas/p2-local-fault-gates.schema.json) 已就绪；为隔离正在运行的 P2-A11 资源样本，正式候选收据将在 soak 结束并提交后执行。

## 6. 自动化验收矩阵

| 验收点 | 自动化证据 | 结果 |
|---|---|---|
| 条目硬边界与 LRU | 上限设为 2，命中刷新第一项后写入第三项；第二项淘汰，第一/第三项可读，最终恰好 2 项 | Pass |
| 保留期限 | 人工把一个 PNG 最后使用时间调整到策略外，维护只把该项归为过期 | Pass |
| 解码器精确失效 | 四项中只修改一个描述的解码器版本，维护仅归类并移除该旧版本项 | Pass |
| 孤立回收 | 完整目录快照排除一个源令牌，维护移除对应 PNG/JSON 文件对 | Pass |
| 半项恢复 | 删除一项 JSON 模拟中断写入；重启报告 `maintained` 并回收残留 PNG | Pass |
| 安全全量清理 | 报告两个派生文件，缓存不可读，原图摘要和 Sidecar 字节保持 | Pass |
| tombstone 所有权 | 合法前缀/UUID 但无专用标记的目录不被启动回收 | Pass |
| P2-A10 真实崩溃 | 根改名后与新根建立后分别进程 `abort`，重启均恢复并核对用户文件 SHA-256 | Pass |
| 前后端协议 | Rust 序列化、TypeScript wire test、演示适配器和设置页状态全部覆盖维护报告与五种启动状态 | Pass |

## 7. 本地质量门禁

执行：

```text
npm run ci
```

该命令完整执行格式检查、Rust/TypeScript 静态检查、所有单元测试、S 数据集跨模块验收、Tauri Release 构建和 Obsidian Bridge production 构建。结果全部通过：

- Rust：103 项测试；
- 桌面 TypeScript：42 项测试；
- Obsidian Bridge：8 项测试；
- `cargo clippy --workspace --all-targets -- -D warnings`：通过；
- S 数据集：1,000 素材、200 Sidecar、999 个有效尺寸、1 个损坏图片隔离、0 个扫描问题，原始素材摘要不变；
- Tauri release、桌面 Vite 与 Obsidian Bridge production build：通过。

## 8. 已知边界

- LRU 使用文件 mtime；外部工具若主动修改缓存目录时间，会影响淘汰顺序，但不会影响用户数据或缓存命中正确性；
- 并发解码允许在维护锁获得前短暂超过估算容量，所有进行中的写入释放读锁后立即收敛；
- 缓存维护需要遍历普通文件目录；P2-06 已把它纳入统一任务限流、后台优先级和长时间资源监测，正式 8 小时 A11 仍在运行；
- P2-A08/P2-A09 已通过本地验收；P2-A11 与 P2-A12 仍是阶段退出门禁。

## 9. 结论

P2-05 已通过本地专项验收。缩略图缓存有明确且可见的容量、期限和版本规则，素材目录收敛后会回收孤立项；全量清理在两个真实崩溃边界中都能重启恢复，并证明原图与 Sidecar 不变。P2-06/P2-08 已完成本地实现，最终候选仍须重新执行 A10 故障收据并完成 A11/A12。
