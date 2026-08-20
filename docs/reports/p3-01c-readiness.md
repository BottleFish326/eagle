# P3-01C AVIF/HEIC 就绪报告

- 状态：In progress；core-only boundary accepted locally，Linux/macOS libheif jobs verified，desktop preview/scan wiring implemented locally
- 日期：2026-08-20
- 范围：P3-01C 的格式识别、真实正常夹具、worker 隔离与固定 backend
- 非结论：不代表 P3-01C、P3-01、P3-A01 或 P3-A02 通过

## 1. 已关闭边界

ISO BMFF 分类器只读取 64 KiB 签名窗口内第一个 `ftyp` box，分别处理标准、扩展和
开放长度；只接受完整四字节 major/compatible brand，不会把声明 box 之后的字节误作
brand。AVIF 优先识别 `avif/avis`，HEIC/HEIF 覆盖静态与 sequence brand，MP4/MOV
继续由各自 brand 分类。识别过程不分配 brand 列表，也不依赖本机 codec。

两个正常夹具来自官方 `strukturag/libheif` 仓库的固定 commit，受上游
`examples/COPYING` MIT 许可约束。清单记录原始 URL、长度和 SHA-256；
`tools/import-format-fixtures.mjs` 只在内容逐字节符合固定长度与摘要时原子导入，已有
不同内容不会被覆盖。完整许可文本见 `fixtures/formats/THIRD_PARTY_NOTICES.md`。

## 2. 当前能力结果

| 路径 | AVIF | HEIC |
|---|---|---|
| 内容识别 | `image/avif` | `image/heic` |
| 扫描/Sidecar | 保留素材，继续合并 | 保留素材，继续合并 |
| core-only 属性 | `codec-unavailable` | `codec-unavailable` |
| core-only 预览 | `codec-unavailable`，不写 PNG 缓存 | `codec-unavailable`，不写 PNG 缓存 |
| Linux backend 属性 | 800 × 533、1 图 | 1280 × 854、2 图 |
| Linux backend 预览 | 64px 内 PNG | 64px 内 PNG |
| macOS backend 属性/预览 | 800 × 533、1 图；64px 内 PNG | 1280 × 854、2 图；64px 内 PNG |
| 桌面预览入口 | 仅在固定 bundle 清单和库根授权同时成立时调用 worker | 同左 |
| 桌面扫描属性 | `dimensions` + `media` 派生字段，不写 Sidecar | 同左，含图像数 |
| 原素材 | 只读且摘要不变 | 只读且摘要不变 |

这条降级不是文件损坏，也不会把素材从目录、Tag 或 `type:image` 查询中删除。

## 3. worker 隔离边界

`format-worker` 使用一请求一进程，而不是把 C/C++ codec 装入桌面进程。父进程只执行
绝对路径、非符号链接且 SHA-256 与构建清单相符的 worker；启动前后均重新核验摘要。
源文件在构造请求和执行前都重新 canonicalize 并证明位于授权根内，非 UTF-8 Unix 路径
和 Windows UTF-16 路径以原生字节编码传输。子进程清空继承环境并在固定目录运行，
不搜索 `PATH`。

桌面预览服务现已接入这条边界：应用只从资源目录下固定
`format-workers/libheif/manifest.json` 发现 worker。清单必须精确匹配 schema、运行平台、
架构、`bundled-libheif` provider/version、单层可执行文件名和小写 SHA-256；清单目录、
清单文件和可执行文件均拒绝符号链接。只有 catalog 记录携带的 `rootId` 能解析到启用且
当前可访问的 Library Root 时，父进程才把该根作为 worker 授权边界。清单缺失或无效时
保留 core-only 降级，并写入不含素材路径的诊断事件。

构建期 `package-format-worker.mjs` 从一个非符号链接的 release 二进制创建全新派生目录，
复制后重算摘要，以原子 rename 发布同一 schema 的 `manifest.json`。已有输出目录不会被
覆盖。独立 Rust verifier 随后使用与桌面启动完全相同的 loader 重放清单、平台、架构、
provider 和摘要校验；CI 只归档通过该重放的 bundle。

正式随包构建统一使用独立 vcpkg manifest，固定到
`33e5269bbfc24fb252bc48a3e624c8193afdccce`（libheif 1.23.1 port），关闭默认 feature，
只显式启用 AOM；libde265 由该 port 的核心依赖提供。这样 Windows 门禁不会隐式引入
x265 encoder，Linux/macOS 也不再依赖 runner 上偶然存在的 Homebrew/APT codec。
Unix 使用固定 vcpkg 静态库，门禁拒绝成品中的 libheif/AOM/libde265 等外部动态依赖；
Windows 使用 `x64-windows-static-md`。每个 job 必须真实解码两个固定样本、生成摘要清单
并由同一 runtime loader 重放后才归档；当前实现已进入托管 CI，结果尚未接受。

stdin 请求与 stdout JSON header 均使用四字节长度前缀，PNG payload 具有独立长度、
SHA-256 和 IHDR 尺寸约束。父进程并行、持续排空 stdout/stderr，但只保留受限字节；
stdout 洪泛返回 `output-too-large`，stderr 最多 4 KiB 且素材绝对路径替换为 `<source>`。
10 秒为生产硬上限，超时直接终止子进程。每次请求都会得到全新进程，前一次崩溃不会
污染下一次调用。

默认 core-only worker 返回 `codec-unavailable`；显式 `embedded-libheif` feature 固定
`libheif-rs 3.0.0` 与 libheif 1.23.1，只暴露 metadata/thumbnail 解码路径。backend 在
解析前设置 256 MiB 总内存/单块上限、像素/图像数/瓦片数/颜色配置上限，启用严格解码
与 HDR 到 8 bit 转换，并把输出写入有界 PNG writer。测试 worker 不进入应用 bundle，
仅用于证明成功帧、二进制替换、授权根逃逸、崩溃、超时、输出洪泛、源变化、路径脱敏
和 PNG 完整性。详细 wire contract 见
[格式 worker 协议](../../specs/format-worker-protocol.md)。

## 4. 本地证据

```bash
npm run import:libheif-fixtures
npm run verify:format-fixtures
cargo test -p asset-filesystem -p asset-preview
cargo test -p format-worker --all-targets
cargo clippy --workspace --all-targets -- -D warnings
# GitHub-hosted Ubuntu：
cargo test -p format-worker --all-targets --features embedded-libheif
```

- 格式清单：6 个源文件、832,225 字节源内容、95 字节参考 PNG；
- filesystem：51 项通过，其中真实 AVIF/HEIC 从内容识别且无 codec 也能扫描；
- preview：22 项通过，其中真实 AVIF/HEIC 均稳定降级且缓存保持为空，派生属性映射不改
  Tag/Sidecar 状态；
- ISO BMFF：4 项专门边界测试覆盖 compatible brand、box 越界、异常长度与序列 brand。
- format-worker：3 项协议单元测试与 4 项真实子进程集成测试通过。
- bundle/desktop 接线：2 项清单边界测试、22 项 preview 测试和 23 项桌面 Rust 测试通过；
  严格 Clippy 通过。完整仓库门禁必须在提交后以干净工作树重放。
- 构建期 bundle：2 项 Node 边界测试通过；本机 release worker 经清单生成后由 Rust
  runtime loader 成功重放。
- GitHub run `32384941995` 全部成功；其中 Linux/macOS `Fixed libheif worker backend` jobs
  均完成真实属性/PNG 解码、release bundle 生成、runtime loader 重放和 artifact 归档。
- GitHub run `32385977748` 全部成功，重新覆盖扫描派生属性提交的全仓质量、Linux/macOS
  worker bundle 与原有三平台路径门禁。
- GitHub run `32380847491` 全部成功；其 `Fixed libheif worker backend` job：6 项 feature 单元测试
  与 1 项真实 backend 集成测试通过；固定 AVIF/HEIC 均完成属性与受限 PNG 解码。

## 5. 未关闭范围与下一动作

P3-01C 仍缺少 Windows backend 的已接受托管证据和三平台随应用打包。下一轮 CI 已增加
Windows 真实样本 probe、Unix 动态依赖审计，以及依赖 Linux/macOS/Windows worker artifact 的 `deb`、`.app`
和 NSIS 构建；每个构建都会从成品中提取资源，再由生产 runtime loader 执行两个真实
样本。该门禁尚未产生已接受结果。扫描属性接线已本地实现，
但还要用真实应用 bundle 关闭端到端扫描门禁。构建期摘要清单生成与 runtime 重放门禁
已经实现，但尚未用随包三平台产物
关闭端到端门禁。正常 backend 还没有进入 `bundled-codecs` 夹具期望，损坏、截断、
伪装、超大声明、未知 codec 和资源超限固定夹具及三平台证据也未补齐。

在这些项目完成前，P3-01C 保持 **In progress**，P3-A01/P3-A02 不判定通过。
