# 格式 worker 进程与 wire 协议

- 状态：Implemented boundary；codec backend pending
- Schema：1
- 对应：P3-01C 至 P3-01F、ADR-026

## 1. 进程模型

复杂或原生解析器使用“一请求一进程”。桌面进程绝不从 `PATH` 搜索 worker，只接受
应用提供的绝对路径、固定 SHA-256、provider ID 和 provider version。启动前后都核验
binary 摘要；binary、工作目录和源路径在使用前 canonicalize，binary/工作目录不得是
符号链接，源必须是授权 canonical root 下的普通文件。父进程在请求构造与执行时均
重做源路径 canonicalization 和 containment 检查，拒绝中途替换的逃逸符号链接。

子进程只接收 `--stdio-once`，继承环境被清空，stdin/stdout/stderr 全部管道化。请求
完成、返回错误、崩溃或超时后进程必须退出；不维护跨素材的 codec 状态。父进程持续
排空两个输出管道，防止子进程通过填满 pipe 阻塞超时控制。

## 2. 请求 frame

stdin 只包含一个 frame：

```text
u32 big-endian JSON byte length
UTF-8 JSON request bytes
EOF
```

JSON 最大 64 KiB，Schema 1 字段为：

```text
schema, requestId(UUIDv7), providerId, providerVersion,
operation(metadata|thumbnail), sourcePath, sourceSize,
sourceModifiedUnixNs, limits
```

`sourcePath` 不用有损 UTF-8 display path。Unix 传输原生字节的 lower-hex；Windows
传输 UTF-16 code unit 的 big-endian lower-hex。worker 只能在相同平台解码对应格式。
`limits` 固定约束：缩略图单边 16–2,048、源单边不超过 65,535、解码分配不超过
256 MiB、PNG 不超过 32 MiB、单请求硬超时不超过 10 秒。metadata 请求不得携带
`maxEdge`，thumbnail 请求必须携带。

## 3. 响应 frame

stdout 只包含一个 frame：

```text
u32 big-endian JSON header byte length
UTF-8 JSON response header
exact optional PNG payload bytes
EOF
```

header 最大 64 KiB 并逐字回显 request ID、provider ID/version。`ready` 响应提供主图
属性；thumbnail 另声明 PNG byte length、SHA-256、width 和 height。父进程同时核对
声明长度、全局/请求上限、PNG signature、IHDR 宽高、摘要、目标尺寸和 EOF，不接受
尾随字节。metadata 响应不得带 PNG，thumbnail ready 响应必须带 PNG。

稳定 worker 错误为：

```text
codec-unavailable, unsupported-feature, invalid-content,
resource-limited, timed-out, source-changed, unreadable,
decode-failed, internal
```

错误消息为 1–1,024 字节，不能包含素材绝对路径。父进程另外区分 binary substitution、
授权逃逸、进程 crash、timeout、stdout overflow、协议损坏、身份不符、源变化和 PNG
损坏，不能把这些情况折叠为 codec 缺失。

## 4. 诊断与数据安全

stderr 不属于 wire response。父进程最多保留 4 KiB，并把当前素材绝对路径替换为
`<source>`；其余内容仍进入应用既有的有界脱敏诊断流程。stdout 超过 header + 请求
PNG 上限后继续排空但不再保留，最终返回明确 overflow。

worker 对素材只读，输出仅回到 stdout；PNG 最终写入应用拥有的派生缓存。worker 不
创建 Sidecar、不修改/复制/重命名素材、不建立权威数据库，也不把容器中的 URL、路径、
附件或动作作为新的发现入口。

## 5. 当前证据与未关闭范围

`format-worker` 的单元测试覆盖请求/响应 round-trip、原生路径、限制、payload 配对和
尾随字节；真实子进程测试覆盖固定成功帧、worker SHA-256 替换、授权根逃逸、崩溃、
硬超时、stdout 洪泛、源变化、逃逸符号链接替换、逐请求资源上限、stderr 路径脱敏及
PNG 完整性。

协议边界通过不表示 codec 可用。正式 binary 当前明确返回 `codec-unavailable`；只有
固定 libheif backend、三平台随包 binary/依赖、security limits 和恶意/资源夹具全部
通过后，P3-01C 才能开放 `bundled-codecs` capability。
