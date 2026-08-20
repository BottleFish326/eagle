# P3-01D 视频容器属性验收报告

- 状态：Completed locally（容器属性）；可选视频帧 worker 已按发行边界正式暂缓
- 日期：2026-08-21
- 范围：P3-01D 的 MP4/MOV/WebM 轨道、时长、尺寸与 codec 中性降级
- 非结论：不代表 P3-01、P3-A01、P3-A02 或阶段 3 通过

## 1. 验收结论

MP4、MOV 与 WebM 现在可以在不解码媒体 packet 的前提下派生容器属性。扫描器仍以素材
文件和相邻 Sidecar 为唯一真相源，只把时长、宽高、音视频轨道数和已知 codec 写入可
重建的运行期 `AssetRecord`；扫描前后的夹具逐字节比较一致，不创建素材副本，也不改写
文件名、目录、Tag 或 Sidecar。

未知 codec 保留为中性视频素材：类型、时长和轨道数仍可用，`codec` 留空，不产生损坏
问题。视频帧预览仍明确返回 provider 不可用；打包与许可审计已决定在满足独立重开门禁
前保持暂缓，详见 `p3-01d-video-frame-decision.md`，不阻塞基础容器属性能力。

## 2. 固定实现边界

| 项目 | 判定 |
|---|---|
| 依赖 | `symphonia = 0.6.1` 精确锁定，关闭默认 feature，只启用 `isomp4` 与 `mkv` demuxer；不启用音视频 decoder |
| ISO BMFF 预检 | 顶层 box 最多 4,096 个；必须存在 `ftyp` 与唯一 `moov`；单个元数据 box 最大 16 MiB |
| WebM 预检 | 必须存在 EBML/Segment/Info/Tracks；顶层元素最多 4,096 个；单个元数据元素最大 16 MiB、累计最大 32 MiB |
| demux I/O | 总读取最大 32 MiB、seek 最多 4,096 次、metadata tag 最大 64 KiB、visual metadata 禁止 |
| 属性限制 | 最多 256 条轨道、时长最大 365 天、宽高必须为 1–65,535 |
| 时间限制 | 复用扫描器每文件 5 秒协作式 deadline；零剩余时间直接返回 `resource-limited` |
| 源隔离 | 拒绝符号链接和非普通文件；解析前后复核长度与修改时间；只读打开素材 |
| 故障映射 | 截断/结构损坏为 `invalid-native-metadata`，超限为 `resource-limited`，未知 feature/codec 保持中性降级 |

## 3. 固定夹具与清单

仓库生成器确定性产生 9 个小型二进制夹具：正常 MP4、MOV、WebM，截断 MP4，PNG 伪装
MP4，MP4 伪装 WebM，未知 sample-entry codec，超 365 天时长声明，以及 65,536 像素
WebM 宽度声明。所有夹具均由仓库生成并采用 MIT 许可；生成器测试要求跟踪文件与重新
生成结果的大小和 SHA-256 完全一致。

格式清单现包含 22 个源文件、1,405,718 字节源内容和 24,338 字节引用计数。新增 9 项
均覆盖 Windows、macOS 与 Linux 的 `core-only` 期望；正常视频精确约束 2,000 ms、
320 × 180、1 条视频轨和 1 条音轨，未知 codec 不伪造 codec 名称，伪装文件以内容 MIME
为准并报告 `mime-mismatch`。

## 4. 可重复证据

```bash
npm run generate:video-format-fixtures
npm run verify:format-fixtures
node --test tools/generate-video-format-fixtures.test.mjs tools/format-fixture-manifest.test.mjs
cargo test -p asset-core -p asset-media -p asset-filesystem --all-targets
cargo clippy --workspace --all-targets -- -D warnings
npm run ci
```

- `asset-media`：3 项测试通过，覆盖三容器正常属性、未知 codec、截断、超限和零 deadline；
- `asset-filesystem`：54 项测试通过，其中两项视频扫描测试同时证明内容优先识别、故障
  隔离、派生属性和源文件逐字节不变；
- Node 清单/生成器：7 项定向测试通过，真实摘要、尺寸、平台覆盖和确定性生成均接受；
- 隔离实现提交 `334cecd`（合入 main 为 `0d5627d`）上完整 `npm run ci` 通过：工具测试
  93 项、Rust workspace
  全部测试、桌面 UI 52 项、Obsidian bridge 8 项、S 数据集、严格 Clippy/TypeScript、
  Tauri release 和 bridge production build 均接受。

## 5. 未关闭范围

- 受控视频帧 worker、固定 PNG 参考与取消/内存/崩溃隔离（按独立决策暂缓）；
- P3-01E 的 MP3/WAV/FLAC 属性与封面（后续已完成，见 `p3-01e-acceptance.md`）；
- P3-01F 的 PDF 属性与首页预览；
- 全格式 P3-A01 一致性和 P3-A02 超大/损坏/恶意集合正式门禁。

视频帧 worker 的打包/许可决策已完成：当前保持 `unsupported-feature`，不得从 `PATH`
调用任意 FFmpeg。P3-01E 后续已完成，下一动作是 P3-01F PDF；P3-A01、P3-A02 与
P3-01 保持未通过。
