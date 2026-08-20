# P3-01E 音频属性与封面验收报告

- 状态：Completed locally
- 日期：2026-08-21
- 范围：MP3/WAV/FLAC 的有界属性提取、MP3/FLAC 内嵌封面预览与故障隔离
- 非结论：不代表 P3-01、P3-A01、P3-A02 或阶段 3 通过

## 1. 验收结论

MP3、WAV 与 FLAC 现在都能在不播放或解码音频 sample 的前提下派生时长、采样率、
声道数、已知 codec 和可用位深；MP3 与 FLAC 的首个内嵌封面可通过独立 provider 进入
既有派生缩略图缓存。无封面、损坏标签、未知 WAV 编码和超限声明仍保留稳定音频素材
记录与 `type:audio` 可见性，不会因为属性或封面不可用而隐藏文件。

扫描器仍以素材文件和相邻 Sidecar 为唯一真相源。音频字段只进入可丢弃、可重建的
运行期 `AssetRecord.media`；封面只写应用拥有的缓存。正常和对抗夹具在扫描/预览前后
逐字节一致，不创建素材副本，不改写文件名、目录、Tag 或 Sidecar。

## 2. 固定实现边界

| 项目 | 判定 |
|---|---|
| 依赖 | `symphonia = 0.6.1` 精确锁定并关闭默认 feature；只增加 `id3v1`、`id3v2`、`mp3`、`wav`，保留既有 `isomp4`/`mkv`；未启用未使用的 FLAC decoder |
| MP3 | ID3v2 synchsafe 声明最大 16 MiB且必须落在文件内；随后必须有 MPEG frame sync；优先使用容器时长，缺失时从首帧固定 bitrate 与音频字节数派生有界时长 |
| WAV | 必须为 RIFF/WAVE；chunk 最多 4,096 个，声明范围必须在 RIFF/file 边界内，非 `data` chunk 最大 16 MiB，必须同时存在 `fmt ` 与 `data` |
| FLAC | 自有只读 metadata-block walker；首块必须为 34 字节 STREAMINFO；最多 4,096 块，单块最大 16 MiB、累计最大 32 MiB；不读取或解码音频 frame |
| parser I/O | Symphonia 路径总读取最大 32 MiB、seek 最多 4,096 次；tag 与单封面各最大 16 MiB；扫描复用每文件 5 秒 deadline |
| 属性限制 | 最多 256 条 track、时长最大 365 天、采样率最大 768 kHz、声道数最大 64、位深最大 64 |
| 封面输出 | provider 固定为 `embedded-mp3-cover` / `embedded-flac-cover`、版本 `bounded-audio-cover-v1`；只接受 PNG/JPEG/GIF/WebP，65,535 单边、256 MiB 解码分配及 2,048 最大输出边沿用既有缩略图边界 |
| 源隔离 | 拒绝符号链接和非普通文件；解析前后复核长度与修改时间；素材只读打开，封面只进入派生缓存 |
| 降级 | 无封面为 `preview-unavailable`；未知 WAV codec 不产生损坏 issue；截断/结构损坏为 `invalid-native-metadata`；超限为 `resource-limited` |

## 3. 固定夹具与清单

仓库生成器确定性产生 10 个音频夹具：正常 MP3、带 PNG 封面的 MP3、正常 WAV、正常
FLAC、带 PNG 封面的 FLAC、截断 ID3、PNG 伪装 MP3、MP3 伪装 WAV、未知 WAV codec，
以及只用 10 字节声明超 16 MiB ID3/封面的资源攻击。所有夹具均为仓库生成、MIT 许可；
生成器测试要求跟踪字节与重新生成结果完全一致。

格式清单现包含 32 个源文件、1,463,323 字节源内容和 24,623 字节引用计数。新增 10 项
均覆盖 Windows、macOS 与 Linux 的 `core-only` 期望；MP3 固定为 521 ms、44.1 kHz、
双声道，WAV/FLAC 固定为 1,000 ms、8 kHz、单声道、16 bit。两个封面夹具复用清单中
SHA-256 为 `ead9b240…` 的 16 × 16 PNG，并由 preview 测试验证输出与源文件隔离。

## 4. 可重复证据

```bash
npm run generate:audio-format-fixtures
npm run verify:format-fixtures
node --test tools/generate-audio-format-fixtures.test.mjs tools/format-fixture-manifest.test.mjs
cargo test -p asset-media -p asset-filesystem -p asset-preview --all-targets
cargo clippy --workspace --all-targets -- -D warnings
npm run ci
```

- 实现提交 `343e0b6`，依赖收窄提交 `55a37e8`；
- `asset-media`：6 项总测试通过，其中 3 项音频测试覆盖正常属性、双容器封面、截断、
  伪装、未知编码、超限和零 packet decode；
- `asset-filesystem`：56 项测试通过，音频扫描测试精确断言属性、内容优先识别、故障
  隔离和所有源文件逐字节不变；
- `asset-preview`：24 项测试通过，双 provider、无封面、损坏和超限均有稳定结果且不
  产生错误缓存项；
- 干净提交 `55a37e8` 上完整 `npm run ci` 通过：工具测试 95 项、Rust workspace 全部
  测试、桌面 UI 52 项、Obsidian bridge 8 项、S 数据集、严格 Clippy/TypeScript、Tauri
  release 与 bridge production build 均接受。

## 5. 未关闭范围

- P3-01F 的 PDF 页数、首页面尺寸、安全结构检查与首页预览降级；
- 已按发行边界暂缓的视频帧 worker；
- 全格式 P3-A01 一致性和 P3-A02 超大/损坏/恶意集合正式门禁。

因此 P3-01E 判定为 **Completed locally**，下一动作是 P3-01F；P3-01、P3-A01、
P3-A02 与阶段 3 保持未通过。
