# P3-01D 可选视频帧 worker 决策

- 状态：Deferred by design；基础视频格式支持不受阻塞
- 日期：2026-08-21
- 范围：MP4/MOV/WebM 静态首帧 provider 的打包与许可边界
- 决策依据：ADR-026 与 `extended-format-protocol.md`

## 1. 决策

当前阶段不把 FFmpeg 或系统媒体工具打入 Material Eagle，也不从 `PATH` 调用用户安装的
`ffmpeg`。MP4/MOV/WebM 保持已经验收的容器属性与通用视频卡片；静态预览在
`core-only` profile 明确返回 `unsupported-feature/video-frame-worker-unavailable`。

这是一项能力降级，不是文件损坏：视频继续进入扁平素材目录、合并相邻 Sidecar、参与
Tag/filter 和 Obsidian 引用，容器时长、尺寸、轨道数与已知 codec 继续可用。P3-01D
基础格式支持因此完成；P3-A01 以清单中的显式 unavailable 期望验收，不伪造成功预览。

## 2. 暂缓原因

FFmpeg 官方许可清单要求 LGPL 构建至少关闭 `--enable-gpl` 与 `--enable-nonfree`，优先
动态链接，随分发物提供精确对应源码、修改 diff、完整构建命令、下载入口和产品内声明，
并对每个外部 LGPL 依赖重复合规审计。H.264 还具有因发行地区和商业模式而异的专利
风险，不能仅凭编译成功视为可发布。

仓库当前固定的 vcpkg baseline
`33e5269bbfc24fb252bc48a3e624c8193afdccce` 提供 FFmpeg 8.1.2；其默认 feature 同时启用
`avcodec`、`avdevice`、`avfilter`、`avformat`、`swresample` 与 `swscale`，远大于“只解码
首个 H.264/VP8/VP9 帧”的最小边界。即使显式关闭默认 feature，标准 port 仍构建 avcodec
内部的广泛编解码能力，而不是确定性的 decoder allowlist。

现有 worker manifest 只摘要绑定一个可执行文件；官方建议的动态 FFmpeg 分发还需要绑定
多个未改名 DLL/dylib/so、对应源码和许可材料。直接复用当前 schema 会留下可替换动态库
和不完整 LGPL 归档，因此不能进入产品。

## 3. 未来重新开启的强制门禁

只有新的独立 ADR 同时关闭以下条件，才允许新增 `bundled-video-frame` profile：

1. 固定 FFmpeg source commit/tarball 与 SHA-256，保存完整 configure 命令和零/显式 patch；
2. 自有 overlay port 使用 `--disable-everything`，只开放本地文件 I/O、MOV/Matroska
   demux、必需 parser、H.264/VP8/VP9 decoder、颜色转换与缩放；encoder、mux、filter、
   device、network、GPL、nonfree 和外部 codec 全部关闭；
3. worker bundle schema 升级，逐文件绑定可执行文件、FFmpeg 动态库、许可、源码下载信息
   与 provider version，启动前后都复核摘要；
4. Linux/macOS/Windows 成品中保留 FFmpeg 库名，验证没有未打包依赖，并在 About/EULA/
   发布页完成 LGPL 声明与对应源码提供；
5. 使用真实含 packet 的可再分发视频夹具，证明首帧字节一致、10 秒硬超时、256 MiB
   解码上限、取消、崩溃、输出洪泛、未知 codec 和源变化隔离；
6. 完成 H.264/HEVC 目标发行地区和商业模式的单独法律/专利评审。

## 4. 依据

- [FFmpeg License and Legal Considerations](https://ffmpeg.org/legal.html)
- [固定 vcpkg baseline 的 FFmpeg port manifest](https://github.com/microsoft/vcpkg/blob/33e5269bbfc24fb252bc48a3e624c8193afdccce/ports/ffmpeg/vcpkg.json)
- [固定 vcpkg baseline 的 FFmpeg portfile](https://github.com/microsoft/vcpkg/blob/33e5269bbfc24fb252bc48a3e624c8193afdccce/ports/ffmpeg/portfile.cmake)

下一开发切片为 P3-01E 音频属性与有界封面；本决策不关闭 P3-A01/P3-A02。
