# P2-02 移动与孤立文件处理验收报告

- 状态：Passed locally
- 日期：2026-08-19
- 对应：P2-02、P2-A01、P2-A02、P2-A03
- 决策：[ADR-019](../../specs/adr/019-stable-identity-and-explicit-relink.md)

## 1. 交付范围

本工作项完成以下链路：

- Sidecar 创建/编辑时记录素材大小、64 KiB 首尾采样快速指纹和完整 SHA-256；
- 完整扫描完成时按根原子收敛，不再在开始时清空卡片；
- 扫描前后唯一稳定 ID 生成旧键到新键的映射，前端迁移选择和选择锚点；
- 识别相邻素材缺失的孤立 Sidecar，并显示丢失素材路径；
- 只对大小、快速指纹、完整 SHA-256 全部确认的素材生成重新关联候选；
- 多个相同内容候选保持歧义，必须由用户明确选择；
- 后端保存不透明候选 ID，确认前重新校验授权根、Sidecar 摘要、稳定 ID 和全部素材指纹；
- 采用无覆盖的“创建硬链接后移除旧 Sidecar”，目标已存在或内容变化时停止；
- 根目录界面提供“孤立 Sidecar”“丢失素材”“待确认移动”诊断及确认入口。

未引入数据库、IndexedDB、LocalStorage 或权威移动映射。诊断与候选只存在于运行期；素材和 Sidecar 仍是唯一真相源。

## 2. 扫描收敛协议

扫描开始时保留旧根记录，批次结果进入运行期视图。扫描成功后，目录以完整扫描结果替换该根，并输出：

- `removedKeys`：磁盘已不存在或旧移动路径对应的运行期键；
- `movedAssets`：稳定 ID 唯一匹配时的 `fromKey -> toKey`；
- `restoredRecords`：扫描取消或失败时需要恢复的扫描前记录。

前端先恢复记录、再删除旧键并迁移选择。重复稳定 ID 不生成移动映射。原始素材、Sidecar、其他根、缩略图和配置均不在运行期收敛的删除边界内。

## 3. 候选与确认安全边界

### 3.1 只读诊断

诊断从已配置根和当前内存目录读取数据。扫描 Sidecar 不会移动、复制、删除或补写文件。没有指纹的旧 Sidecar 标记为“缺少指纹”，不会根据路径相似度、文件名或单独的大小猜测。

### 3.2 三级匹配

1. 用 Sidecar 中的素材字节数筛选；
2. 对同大小素材读取首尾最多 64 KiB，计算带长度域的 `sha256-sample-64k-v1`；
3. 快速指纹通过后读取完整素材并比较 SHA-256。

即使只剩一个完整哈希候选也必须点击确认。两个相同内容候选都标记为歧义，界面提供逐项“选择此候选”，默认保持未解决。

### 3.3 显式文件操作

前端确认只发送后端生成的候选 UUID，不发送源/目标路径。后端重新读取并校验候选，然后以硬链接的 no-clobber 语义创建目标 Sidecar，再删除源 Sidecar：

- 目标已存在（包括并发创建）时不覆盖；
- 创建目标失败时源 Sidecar 保持不变；
- 创建后删除源失败时最多保留两份完整 Sidecar，不丢失元数据；
- 素材文件始终只读，确认前后字节摘要一致。

## 4. 自动化验收

| 用例 | 证据 | 结果 |
|---|---|---|
| P2-A01 成对移动素材与 Sidecar | 目录收敛测试以唯一稳定 ID 生成移动映射；旧键删除、新键保留，UI 单元测试迁移选择与锚点 | Pass |
| P2-A02 只移动素材 | 产生 1 个孤立 Sidecar、1 个丢失素材和 1 个三级确认候选；诊断后磁盘无变化 | Pass |
| P2-A02 只移动 Sidecar | 原素材成为无 Sidecar 素材，移走的 Sidecar 正确指向原素材候选 | Pass |
| Sidecar 移到同名错误素材旁 | 快速指纹不匹配时不合并 ID/Tag，错误 Sidecar 进入孤立诊断并指回原素材候选 | Pass |
| P2-A03 相同内容候选 | 两个完整 SHA-256 相同候选均标记 `ambiguous`，未自动选择 | Pass |
| 目标并发出现 | 候选显示后创建目标 Sidecar，确认返回 `DestinationExists`；源和目标内容都未被覆盖 | Pass |
| 候选 API 边界 | 前端检查命令只传根 ID，确认命令只传候选 ID | Pass |
| 指纹持久化 | 首次元数据编辑写入大小、快速指纹和完整 SHA-256，素材摘要不变 | Pass |
| 旧协议兼容 | 快速指纹字段可选，Schema 仍为 v1；无指纹 Sidecar 可读但不产生猜测候选 | Pass |

## 5. 本地门禁

执行：

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
npm --prefix apps/desktop run format:check
npm --prefix apps/desktop run check
npm --prefix apps/desktop test
npm --prefix integrations/obsidian-bridge run check
npm --prefix integrations/obsidian-bridge test
node tools/verify-s-fixture.mjs
npm --prefix apps/desktop run tauri:build -- --no-bundle
npm --prefix integrations/obsidian-bridge run build
```

结果：全部通过。

- Rust：87 项测试；
- 桌面 TypeScript：36 项测试；
- Obsidian Bridge：8 项测试；
- S 数据集：1,000 素材、200 Sidecar、999 个有效尺寸、1 个损坏图片隔离、0 个扫描问题，44 毫秒，素材摘要未变化；
- Tauri release 与 Obsidian Bridge production 构建成功。

P2-02 没有改变 Obsidian 标准引用格式；成对移动后稳定 ID 与 UI 状态已保持，Vault 外路径可移植性仍按既有策略处理。

## 6. 已知边界

- 没有指纹的历史 Sidecar 不会自动补写，因此分离后可能没有安全候选；
- 跨文件系统或不支持硬链接的重新关联会安全失败，不会退化为可能覆盖的 rename；
- 硬链接创建后、源删除前的进程终止可能留下两份完整 Sidecar，后续扫描会显示诊断；P2-03 的纯文件事务当前用于批量元数据写入，并未把单次重新关联伪装成跨文件系统原子操作；
- P2-A04 至 P2-A10 已由后续工作项完成本地验收；P2-A11 正式长稳任务运行中，P2-A12 Windows/Linux 原生托管结果待完成。

## 7. 结论

P2-02 已通过本地自动化验收。成对移动可保持稳定身份和选择，只移动一侧会进入明确诊断，内容相同或不完整证据不会被系统猜测。唯一会修改磁盘的重新关联动作需要用户明确确认，并在后端再次验证且禁止覆盖；后续 P2-03 至 P2-08 的本地实现不改变这些安全边界。
