# ADR-033：Obsidian 机器级授权、用户态 IPC 与离线最小索引

- 状态：Accepted
- 日期：2026-08-20
- 对应：P4-01、P4-02、P4-04、P4-A03、P4-A09、P4-A10
- 实施门禁：阶段 3 退出后

## 背景

Obsidian 插件既要在桌面素材管理器运行时复用其查询能力，又要在应用关闭时独立解析 `material://<uuid>`。把绝对根路径和长期 bearer token 保存进 Vault 的插件 `data.json` 会随同步泄露机器信息；让远程网页可访问的固定 localhost HTTP API又会扩大攻击面。完全依赖持久化插件索引则会形成第二份容易陈旧的素材状态。

外部图片原型把小文件完整读入对象 URL，无法有效支持大视频、音频和 PDF Range。另一方面，直接给 `<img>`/`<video>` 任意 `file://` 或带路径的本地 URL 会绕过 UUID、授权根和 realpath 边界。

## 决策

1. 桌面端在其操作系统应用配置目录维护 `obsidian-authorization.yml`。它是普通、原子写入的机器级 capability manifest，包含 installation UUIDv7、revision、总开关和用户明确允许给 Obsidian 的根 ID/path/scan 规则。它不写入 Vault、素材根、Sidecar、缓存或数据库。
2. Manifest 使用 [`obsidian-authorization.schema.json`](../../schemas/obsidian-authorization.schema.json)，Unix 文件权限为 0600、父目录 0700；Windows 使用当前用户 ACL。它由桌面端从已配置 LibraryRoot 的子集生成，不能授权不存在、停用、重叠、跟随符号链接或当前不可访问的根。
3. Obsidian 插件第一次发现 installation/root 或 root path fingerprint 变化时逐项显示真实路径并要求批准。插件数据只保存 installation ID、root ID、路径 SHA-256 指纹和 enabled，不保存绝对路径或长期 IPC token。有效授权是 manifest enabled、root enabled、指纹匹配和插件批准的交集。
4. 桌面运行时控制面使用当前用户限定的 Unix domain socket（macOS/Linux）或当前 SID ACL 的 Windows named pipe。启动时在应用配置目录原子写 `obsidian-endpoint.json`，包含协议版本、installation ID、endpoint、pid、start nonce 和更新时间；退出时删除，插件必须把连接失败/握手不符视为离线。
5. 控制协议使用长度前缀的版本化 JSON、1 MiB 请求/响应上限、request ID、deadline 和显式 capability。API不接受绝对路径；查询、引用和打开定位只接受 root/filter/asset 的不透明 ID。连接默认只读，任何创建稳定 ID 的写能力必须由单独 capability、精确预检和用户确认授权。
6. Desktop IPC 只作为加速和桌面联动，不是外部引用真相源。插件始终能从当前 manifest 授权根直接建立内存最小索引；桌面断开时正在进行的只读查询可以明确失败并回退离线，不把 IPC 结果持久化。
7. 离线索引分两阶段：先扫描 Sidecar 建立稳定 ID → 当前路径候选以恢复渲染，再扫描已注册素材建立名称/Tag/类型/收藏搜索记录。索引只驻留内存，插件卸载/重启后重建；不写 `.obsidian` index、IndexedDB、SQLite、LocalStorage 或隐藏根目录文件。
8. 重复稳定 ID 产生歧义并拒绝解析。每次读取前重新 canonicalize 素材/根、拒绝符号链接逃逸、复核当前 manifest revision/approval 和 MIME；监听事件只是使索引失效，溢出/错误回退完整授权根重扫。
9. 小型静态图片在 32 MiB 上限内可使用对象 URL，并在 Markdown child 卸载/源变化/授权撤销时 revoke。大文件和需要 Range 的视频/音频/PDF 使用插件进程临时 loopback media lease server。
10. Media server 只绑定 `127.0.0.1` 随机端口；URL 只含 256-bit 随机、单素材、短期 opaque lease，不含 UUID/path/token query。只接受 GET/HEAD 和单 Range，校验 Host、限制并发/输出/MIME，设置 `nosniff`/`no-store`，不启用 CORS；每个请求再次复核授权与 realpath。
11. Lease 在视图卸载、过期、根撤销、源版本变化或插件卸载时立即失效；无 lease 时服务器关闭。桌面应用是否运行不改变该媒体安全路径，避免在线/离线两套渲染权限语义。
12. 插件禁用/卸载只停止 watcher、socket、lease server 并释放对象 URL，不改写 Markdown、素材、Sidecar、manifest 或桌面配置。

## 影响

- 根绝对路径保留在机器级桌面配置，不随 Vault 同步；插件批准只同步不透明 ID/指纹。
- 当前用户 IPC 避免固定网络查询 API，网页无法仅凭 localhost 端口调用素材搜索或路径能力。
- 桌面应用关闭后仍能重建 ID 解析和搜索，不依赖持久数据库。
- 小图对象 URL 与大媒体 Range 共享相同 UUID/根授权检查，生命周期可撤销。
- 首次安装、根移动或 manifest installation 变化需要用户在 Obsidian 中重新批准，这是安全边界而非自动迁移失败。

## 不采用

- 把根绝对路径、端口或长期 bearer token 写入 Markdown/Vault 插件数据；
- 暴露固定 localhost 查询 API 给任意网页；
- 让 IPC 接受任意文件路径；
- 只在桌面运行时解析外部引用；
- 把离线 ID/path/Tag 索引持久化为数据库或 `.obsidian` 大型快照；
- 直接使用 `file://`、永久 loopback URL 或 UUID 作为可猜测 media URL；
- 大媒体完整读入内存以伪装 Range；
- 插件卸载时删除或改写用户文件。
