# P3-01 扩展格式能力与 P3-A01/P3-A02 验收报告

- 状态：Accepted
- 日期：2026-08-21
- 正式产品提交：`2bf0de2bb6efa905f0062367d7191725f9253ce8`
- 托管运行：`32412621089`，attempt 1，整体 `success`
- 证据：`evidence/p3-a01-a02-format-evidence.json`
- 非结论：不代表阶段 3 或 P3-A03 至 P3-A11 通过

## 1. 验收结论

P3-A01 与 P3-A02 在同一产品提交、同一 GitHub-hosted workflow run 和同一 attempt 上通过。
固定清单的 43 个正常/边界格式夹具和 31 个对抗夹具分别在 `core-only` 与
`bundled-codecs` 配置下于 Linux、macOS、Windows 重放；12 份原始报告全部接受，汇总失败
列表为空。因此 P3-01 扩展格式能力管线与两项门禁判定为 **Accepted**。

该结论不改变数据模型边界：扫描器只解释普通素材文件及相邻 Sidecar，文件派生属性只进入
可删除运行时记录和预览缓存。测试没有移动、重命名、转码或改写素材与 Sidecar，也没有把
派生属性提升为第二真相源。

## 2. P3-A01 格式一致性

正式执行器逐项比较内容识别、类型、文件派生属性、预览结果、问题码与取消行为。六个平台/
配置组合共享同一清单摘要 `20d0f1f6…`，每份 P3-A01 报告都复核 43/43 夹具、31 个对抗
夹具、源字节总量和扫描前后摘要不变。

覆盖范围包括阶段 3 已承诺的安全 SVG、AVIF/HEIC、MP4/MOV/WebM、MP3/WAV/FLAC 和 PDF
属性/降级，以及阶段 1 图片格式。缺少可选 codec、视频帧 worker 或 PDFium 时返回稳定通用
卡片，不隐藏素材、不阻断扫描，也不回退到未固定的 `PATH` 工具。

## 3. P3-A02 隔离、取消与资源边界

每个平台/配置均执行 3 次对抗集合。汇总器从原始时间/RSS 样本重新计算结论，并拒绝稀疏
采样、源漂移、取消丢失、提交不匹配或超过 512 MiB 的进程树峰值。

| 配置 | 平台 | 最大 RSS | 样本数 |
|---|---|---:|---:|
| core-only | Linux | 8,036 KiB | 3 |
| core-only | macOS | 8,880 KiB | 11 |
| core-only | Windows | 6,868 KiB | 15 |
| bundled-codecs | Linux X64 | 25,904 KiB | 87 |
| bundled-codecs | macOS ARM64 | 21,360 KiB | 196 |
| bundled-codecs | Windows X64 | 22,436 KiB | 53 |

超大、截断、伪装和主动内容只使单文件富化失败；取消仍可协作收敛，其他素材继续产生记录。

## 4. 托管成品与独立重放

运行 `32412621089` 的 12 个 job 全部成功或按平台条件正确跳过。三平台成品 job 从已验证
worker artifact 组装桌面应用，再从 Linux Debian 包、macOS `.app` 和 Windows NSIS 包内重新
探测 worker；最终归档：

- Linux X64：`p3-01c-desktop-Linux-X64-2bf0de2…`；
- macOS ARM64：`p3-01c-desktop-macOS-ARM64-2bf0de2…`；
- Windows X64：`p3-01c-desktop-Windows-X64-2bf0de2…`。

汇总 artifact 下载后的 SHA-256 为
`28a63384388776a2e4cf354e8bb7058315fbc43db5119fb6dd851599d9253f7e`。本地随后只下载
12 份原始 P3-A01/A02 报告，使用 Node 24 重新计算每份 SHA-256、重放所有原始 RSS 样本并
生成 final JSON；结果与托管汇总逐字节一致。归档 JSON 还通过 JSON 解析与敏感信息审计，
未包含本机用户路径、用户名、私钥或凭据模式。

## 5. 后续范围

P3-01/P3-A01/P3-A02 已关闭。阶段 3 下一门禁是 P3-A03：在已完成高级查询 parser、索引、
独立 oracle、L 性能和桌面字段编辑器的基础上，补齐由真实普通文件与 Sidecar 扫描产生逻辑
记录的端到端收据，并证明原始素材 SHA-256 前后一致。P3-A04 至 P3-A11 仍未执行。
