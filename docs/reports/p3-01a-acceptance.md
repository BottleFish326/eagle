# P3-01A 注册表与通用卡片验收报告

- 状态：Completed locally
- 日期：2026-08-20
- 范围：P3-01A；不代表 P3-01、P3-A01 或 P3-A02 通过
- 前置阶段：阶段 2 Accepted

## 1. 验收结论

P3-01A 的“注册表与通用卡片”范围已在本地完成。格式识别、属性提取和预览提供器不再相互绑定；系统缺少 codec 或 preview provider 时仍保留素材基础记录、文件字段、扁平 Tag、相邻 Sidecar 和后端查询可见性。实现没有引入素材数据库，也没有移动、复制、改名或转码源文件。

## 2. 已完成内容

| 边界 | 实现与证据 |
|---|---|
| 静态注册表 | 15 个正式格式的 ID、扩展名、MIME 和 kind 在编译期注册；测试验证唯一性、一致性和逐格式签名 |
| 内容优先识别 | 最多读取 64 KiB；内容签名覆盖冲突扩展名并产生 `mime-mismatch`；受控扩展名只在签名不足时回退 |
| 通用素材记录 | SVG、AVIF/HEIC/HEIF、视频、音频与 PDF 在 provider 缺失时仍建立 `AssetRecord`、合并 Sidecar，不执行 raster 尺寸解码 |
| 固定夹具清单 | 首个真实 SVG 夹具记录精确大小和 SHA-256；验证器拒绝 Schema 错误、越界、符号链接、非普通文件、摘要/大小漂移、占位摘要、平台缺失/重复和错误参考 PNG |
| preview 能力 | `codec-unavailable`、`preview-unavailable` 与 `invalid-content` 分离，并预留资源超限和超时稳定原因 |
| 缓存身份 | layout 3、entry Schema 2；provider ID/version 进入 cache key、descriptor 和 ready 回执，旧/未知 provider 项由维护回收 |
| 桌面 UI | provider/codec 缺失显示中性扩展名与类型卡片；损坏、不可读、超限和超时使用错误状态，测试不依赖颜色文本判定 |
| 查询贯通 | 集成测试执行 `scan_root → AssetCatalog → type:image|video|audio|pdf`，四类记录均由 Rust 后端筛选 |

## 3. 可重复验收

```bash
npm run verify:format-fixtures
node --test tools/format-fixture-manifest.test.mjs
cargo test -p asset-filesystem -p asset-preview -p asset-catalog
npm --prefix apps/desktop test
npm run ci
```

局部结果：格式清单验证器接受真实清单；filesystem 44 项、preview 18 项、catalog 7 项、桌面端 52 项测试通过。完整 CI 必须在提交后的干净工作树重新执行，结果记录在对应 Git 提交。

## 4. 未关闭范围

- P3-01B 的 SVG 有界属性、静态 PNG、脚本/事件/外部引用隔离；
- P3-01C 至 F 的 AVIF/HEIC、视频、音频和 PDF 属性/preview provider；
- 全格式正常、损坏、伪装、超限和恶意夹具矩阵；
- P3-A01 全格式一致性与 P3-A02 单文件隔离、取消和内存边界正式证据。

因此下一动作固定为 P3-01B，不提前宣称 P3-01 或阶段 3 通过。
