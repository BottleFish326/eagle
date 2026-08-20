# P3-01C AVIF/HEIC 就绪报告

- 状态：In progress；core-only boundary accepted locally
- 日期：2026-08-20
- 范围：P3-01C 的格式识别、真实正常夹具和 worker 缺失降级
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
| 原素材 | 只读且摘要不变 | 只读且摘要不变 |

这条降级不是文件损坏，也不会把素材从目录、Tag 或 `type:image` 查询中删除。

## 3. 本地证据

```bash
npm run import:libheif-fixtures
npm run verify:format-fixtures
cargo test -p asset-filesystem -p asset-preview
cargo clippy --workspace --all-targets -- -D warnings
```

- 格式清单：6 个源文件、832,225 字节源内容、95 字节参考 PNG；
- filesystem：51 项通过，其中真实 AVIF/HEIC 从内容识别且无 codec 也能扫描；
- preview：21 项通过，其中真实 AVIF/HEIC 均稳定降级且缓存保持为空；
- ISO BMFF：4 项专门边界测试覆盖 compatible brand、box 越界、异常长度与序列 brand。

## 4. 未关闭范围与下一动作

P3-01C 仍缺少固定版本、decoder-only、三平台随应用打包的 libheif worker。该 worker
必须实现有界请求协议、主图宽高/方向/Alpha 等属性、受限 PNG 输出、10 秒硬超时、
256 MiB 分配上限、崩溃重启及脱敏 stderr，并为损坏、截断、伪装、超大声明、未知
codec 和资源超限补齐固定夹具与三平台证据。

在这些项目完成前，P3-01C 保持 **In progress**，P3-A01/P3-A02 不判定通过。
