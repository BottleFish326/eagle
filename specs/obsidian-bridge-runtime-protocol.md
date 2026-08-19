# Obsidian Bridge 授权、IPC 与离线索引协议

本文定义 ADR-033 的机器级 manifest、插件批准、桌面控制面、离线索引和媒体 lease。阶段 4 实施；当前用于关闭 P4-01/P4-02/P4-04 的架构待定项。

## 1. 机器级授权 Manifest

桌面端设置页允许对每个已启用 LibraryRoot 单独打开“允许 Obsidian Bridge”。生成文件：

```text
<app-config>/obsidian-authorization.yml
```

逻辑结构由 [`obsidian-authorization.schema.json`](../schemas/obsidian-authorization.schema.json) 约束。每次添加、移除、启停或规则变化都以 expected file version 原子写入并递增 revision。根条目从当前 library config 复制必要 capability；manifest 不是素材真相，删除后插件失去授权但素材/Sidecar 不变。

桌面端只发布满足以下条件的根：ID 唯一、canonical absolute path、当前可访问、没有配置重叠、recursive=true、followSymlinks=false、ignore 已验证。manifest 文件损坏或权限过宽时插件全部拒绝，而不是尝试部分授权。

## 2. 插件批准

插件本地数据只保存：

```text
BridgeApproval {
  vaultInstanceId,
  installationId,
  roots: [{ rootId, pathFingerprint, enabled }]
}
```

`vaultInstanceId` 是当前 Vault 插件数据中的随机 UUIDv7；`pathFingerprint = SHA-256("material-eagle-obsidian-root-v1\\0" || platformPathKey)`。首次 root、installation 变化或指纹变化时显示 manifest 中的路径/名称并要求批准。未知 root 默认拒绝；manifest remove/disable 立即压倒旧批准。插件用户也可单独关闭已批准根。

插件按 Windows/macOS/Linux 的固定 Material Eagle 应用配置位置发现 `obsidian-authorization.yml`，不在 Vault 数据中保存 manifest 路径，也不提供任意 manifest/root 路径输入。插件不允许从笔记、URI、IPC response 或 Markdown 属性添加根；发现文件后仍需逐根批准。

## 3. Endpoint discovery 与控制协议

桌面集成开关打开且应用运行时创建：

```json
{
  "schema": 1,
  "protocol": 1,
  "installationId": "<uuidv7>",
  "endpoint": "<unix-socket-or-windows-pipe>",
  "pid": 1234,
  "startNonce": "<256-bit-base64url>",
  "updatedAt": "<rfc3339>"
}
```

文件固定 `<app-config>/obsidian-endpoint.json`，owner-only、原子写入，仅作 discovery。Unix socket 所在目录 0700、socket 0600；Windows named pipe ACL 只含当前 SID。endpoint 不监听 TCP。

连接后第一帧：

```text
Hello {
  protocol,
  installationId,
  startNonce,
  client: {
    pluginVersion,
    obsidianVersion,
    approvalRevision,
    vaultSession: {
      vaultInstanceId,
      vaultPathFingerprint,
      displayName,
      capabilities: [backlinks-query, open-note]
    }
  }
}
```

`vaultInstanceId` 是插件数据中的随机 UUIDv7；path fingerprint 使用 ADR-034 的 domain-separated SHA-256，只用于匹配桌面端已配置 Vault，不发送 Vault 绝对路径。displayName/fingerprint 只保留于活动连接和瞬态 UI，不写日志。

服务端回 `HelloAck { protocol, capabilities, manifestRevision, build }`。installation/nonce/protocol 不符即关闭。每帧 4-byte big-endian length + UTF-8 JSON，最大 1 MiB；frame 带 `kind=request|response|event` 和 direction，request ID 在单连接双向唯一，deadline 最大 30 秒，超时/取消释放后端资源。服务端只有在客户端握手声明 capability 后才能发起 backlink/note 请求。

协议 1 capability：

```text
query-assets
resolve-references
open-asset
health
```

插件可向服务端发起上述只读请求；服务端可向已声明 capability 的插件发起 `backlinks.query` 和 `notes.open`。双向方法的 payload、短期 noteHandle、相对笔记路径隐私边界和 revision 规则由 [`obsidian-search-navigation-and-recovery-protocol.md`](obsidian-search-navigation-and-recovery-protocol.md) 固定。

服务端按 manifest 和客户端批准 root ID 交集限制查询。响应不含绝对素材路径、Sidecar 内容或 token。查询每页最多 256 项，opaque cursor 绑定 query/scope/sort/catalog revision 并在 60 秒或连接关闭时失效；不得用单个超限 frame 截断全量结果。`open-asset` 只改变本机桌面 UI 选择；未来 `ensure-stable-id` 属于独立写 capability，默认不存在。

## 4. 离线最小索引

插件即使连接在线，也保持授权/ID 解析本地能力。重建：

### Phase A：引用优先

- 从当前 Vault Markdown 收集被引用 UUID，形成有界 priority set；
- 遍历授权根但优先解析 `*.asset.yml`，按既有 4 MiB 上限和安全 YAML 规则验证 Schema/ID/相邻素材；
- 建立 `id -> candidates[]`，唯一项可渲染，重复项为 ambiguous；
- 每批增量发布引用恢复状态，单个损坏 Sidecar 隔离。

### Phase B：搜索富化

- 按与桌面相同格式 registry 识别素材；
- 合并 name、rootId、relativePath、kind、extension、tags、favorite、rating 和 stable ID；
- 无 Sidecar 素材可搜索；Vault 外插入前需要稳定 ID，离线模式只给出“在桌面端创建 ID”入口；
- 使用与正式查询协议一致的 TypeScript 参考执行或共享 conformance corpus，不自行发明弱化语法。

索引只在 `Map`/倒排集合中存在。应用重启、插件 reload 或用户“重建”都从 manifest、素材和 Sidecar 重建。禁止写 plugin index JSON、IndexedDB、LocalStorage 或 root cache。

## 5. Watch 与一致性

- 每根 watcher 有 120 ms quiet / 750 ms max batch、4,096 event 硬上限；
- 原子保存临时文件折叠，Sidecar/素材变更使对应记录失效；
- rename 半事件、溢出、权限错误或 watcher error 触发该授权根完整重扫；
- manifest revision 变化先撤销不再允许根的对象 URL/lease，再更新 watcher/index；
- plugin approval remove 立即执行相同撤销；
- 根离线保留诊断但不使用旧 path 继续读取。

Watcher 事件不持久化、不成为第二真相源。

## 6. 每次读取安全路径

按 ID 渲染时：

1. 语法必须是规范 UUID，且 index 只有唯一 candidate；
2. root 仍在当前 manifest/approval 交集；
3. root 和 asset 分别 realpath；
4. 逐组件确认 asset 在 root 内，拒绝符号链接逃逸；
5. 相邻 Sidecar 当前仍含同一 ID；
6. 文件头/MIME 在允许渲染 capability；SVG 脚本/外部引用按 P3 provider 安全策略；
7. size/mtime 与索引观察一致，否则失效重扫；
8. 才创建 object URL 或 media lease。

Markdown 中的 path、query、fragment、host 或 alias 不参与文件解析。

## 7. Object URL 与 Media Lease

静态图片且 size ≤ 32 MiB：有界读取、正确 MIME Blob、每个 MarkdownRenderChild 独立 object URL；同时最多 4 个加载、活动 Blob 总字节最多 128 MiB，卸载/变化/撤权时 revoke。

其他支持媒体使用插件内临时 server：

```text
MediaLease {
  token: 32 random bytes,
  assetId,
  rootId,
  observedVersion,
  mime,
  expiresAt,
  ownerViewId
}
```

- 绑定 IPv4 `127.0.0.1:0`，URL `/v1/media/<base64url-token>`；
- Host 必须精确匹配实际 loopback address/port；
- 只允许 GET/HEAD，拒绝 query、目录、编码斜杠和额外 path segment；
- 支持单个合法 bytes Range，拒绝 multi-range，返回正确 200/206/416；
- `Content-Type` 来自已验证 registry，`X-Content-Type-Options: nosniff`、`Cache-Control: no-store`、无 CORS；
- 默认 16 个并发 stream、每 view 4 个、lease 5 分钟且活动可续；
- 每个请求重新执行授权交集、realpath、Sidecar ID 和版本检查；
- root revoke、view unload、source changed 或 plugin unload 关闭 stream/lease；无 lease/stream 时关闭 server。

## 8. 错误与降级

```text
authorization-missing
authorization-invalid
approval-required
root-revoked
root-offline
index-building
id-not-found
duplicate-id
sidecar-invalid
asset-changed
unsafe-path
unsupported-mime
codec-unavailable
desktop-offline
ipc-version-mismatch
media-lease-expired
range-invalid
resource-limited
```

错误占位不修改 Markdown。桌面 IPC 断开只影响加速/定位；本地索引与已有授权继续工作。

## 9. 验收证据

- P4-A03：删除全部插件运行期索引，关闭桌面应用并重启 Obsidian，Phase A 从文件系统恢复所有固定外部引用；
- P4-A09：伪造 UUID/path/query、重复 ID、符号链接、MIME 伪装、Host/Range/lease 攻击全部拒绝；
- P4-A10：manifest 或插件批准移除根后，新读取立即停止，object URL/lease/watcher/index 清理，其他根继续；
- 在线/离线相同 ID 对同一当前文件解析一致；IPC 响应无绝对路径；
- 100,000 素材/20,000 Sidecar 记录 Phase A 首个请求恢复时间、全量完成时间、RSS、句柄、watcher 和队列；
- 插件禁用/卸载前后 Markdown、素材、Sidecar、manifest 摘要完全不变。
