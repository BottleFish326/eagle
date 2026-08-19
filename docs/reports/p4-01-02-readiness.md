# P4-01/P4-02 插件权限与离线索引实施准备报告

> 状态：Architecture ready；阶段 3 退出前不计作阶段 4 开始
>
> 日期：2026-08-20

## 1. 准备结论

P4 的机器级根授权、插件逐根批准、桌面控制 IPC、两阶段离线内存索引和外部媒体生命周期已经定案。根路径不进入 Vault/Markdown，桌面关闭后不依赖持久索引，控制面不暴露 TCP，媒体 loopback 只使用短期随机 lease。

## 2. 已完成资产

| 资产 | 结论 |
|---|---|
| ADR-033 | 固定 manifest、用户态 IPC、内存索引、对象 URL/Range lease 决策 |
| `schemas/obsidian-authorization.schema.json` | 固定 installation/revision/root capability 文件结构 |
| `specs/examples/obsidian-authorization.example.yml` | 覆盖根、ignore 和未知字段保留 |
| `specs/obsidian-bridge-runtime-protocol.md` | 固定 handshake、离线两阶段索引、watch、读取和 media lease |
| P0 bridge prototype | 已验证 UUID/Sidecar、realpath、MIME、小图 object URL 和应用离线基础路径 |

Schema 使用 Draft 2020-12 严格编译；YAML 示例通过。负例覆盖非 UUIDv7 installation/root、revision 0、followSymlinks=true、多行 ignore 和缺失 path。根 ID/路径唯一、canonical/overlap、文件权限和 library subset 由语义验证器/桌面管理器强制。

## 3. 实施切片

### P4-01A：Authorization manager

- 从 LibraryRoot 子集生成 owner-only manifest；
- expected-version 原子写和 revision；
- 设置页逐根开关、撤销和权限诊断；
- 插件 discovery/approval/path fingerprint。

### P4-01B：User-scoped control IPC

- Unix socket/Windows pipe、owner ACL 和 endpoint discovery；
- frame/handshake/deadline/cancel/size 限制；
- root intersection 和无路径响应；
- desktop-offline/version mismatch 降级。

### P4-02A：Phase A ID resolver

- Sidecar 优先、Markdown referenced-ID priority；
- 重复/损坏隔离和增量恢复；
- 每次读取 realpath/授权/ID/version 复核；
- 无持久 index 审计。

### P4-02B：Phase B search index

- 注册格式、名称/Tag/类型/收藏最小记录；
- 与 query conformance corpus 对齐；
- 无 ID 素材搜索和离线插入限制；
- bounded watcher 和完整重扫收敛。

### P4-02C：Media lifecycle

- 32 MiB 小图 object URL；
- loopback opaque lease、Range、Host/MIME/header 检查；
- root/view/source/plugin 生命周期撤销；
- 攻击与资源夹具。

## 4. 通过条件

- P4-A03 在桌面关闭、索引清空后从文件系统恢复引用；
- P4-A09 所有路径/UUID/lease/Range/MIME 攻击均不能读取授权根外文件；
- P4-A10 根撤销立即停止读取且不影响其他根；
- 在线/离线解析一致，IPC 无任意路径能力；
- L 数据集离线索引和 watcher 资源有界；
- 插件设置不含绝对路径或长期 token，索引不落盘；
- 禁用/卸载不修改任何 Markdown、素材、Sidecar 或 manifest。

## 5. 尚未开始

- 未新增 authorization manager、socket/pipe server 或 endpoint discovery；
- 未替换原型 roots 文本框和全量小图读取；
- 未实现 Phase A/B 正式索引、watcher 或 media lease server；
- 未执行阶段 4 验收；
- 未把阶段 4 状态改为 In progress。

以上实现等待阶段 3 退出评审后开始。
