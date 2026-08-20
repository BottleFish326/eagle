# P3-01C AVIF/HEIC 验收报告

- 状态：Accepted
- 日期：2026-08-21
- 验收提交：`97bdb56408ec78eca1f84517b371b8ae2ef3d6d2`
- 托管证据：[GitHub Actions run 32397240917](https://github.com/BottleFish326/eagle/actions/runs/32397240917)
- 非结论：不代表 P3-01、P3-A01、P3-A02 或阶段 3 通过

## 1. 结论

P3-01C 的内容识别、core-only 中性降级、固定 libheif 1.23.1 decoder-only backend、
一请求一进程隔离、桌面授权边界、扫描派生属性、正常/对抗夹具和三平台成品随包验证均
已关闭。Linux DEB、macOS `.app` 与 Windows NSIS 都从成品中重新定位 worker 清单，
再由生产 runtime loader 重放正常 AVIF/HEIC 与 7 个对抗样本；三项 job 全部成功。

正常参考 PNG 在三平台均按清单逐字节验证：AVIF 为
`536aa4fdc12f4ea10da01e0480b992553f872b8b87a1a82a38ffa414a93c68cd`，HEIC 为
`e1d73bde97bd44876f89376a49aca8b0ea8e89cacb5ab7348a0d919239ab1bd1`。素材只读且
SHA-256 不变；Tag、Sidecar 和扁平目录语义不受 provider 是否可用影响。

## 2. 托管门禁

run `32397240917` 绑定验收提交并以 `push` 事件执行，11 个 job 全部 `success`：

- 全仓格式、测试与构建；
- macOS、Linux、Windows 路径兼容矩阵及统一证据；
- macOS ARM64、Linux X64、Windows X64 固定 libheif worker backend；
- macOS `.app`、Linux DEB、Windows NSIS 的成品内 worker 重放。

Windows job 使用 7-Zip 从唯一 NSIS 安装包提取 11 个文件，证明恰有一份
`format-workers/libheif/manifest.json`，随后 runtime verifier 返回
`bundled-libheif libheif-1.23.1-r1`。Linux 与 macOS 执行同一 verifier；对抗输入在每次
调用后继续可用，证明单请求崩溃/失败不会污染后续素材。

## 3. artifact 与独立复核

GitHub 保存 3 个 worker bundle 和 3 个成品 artifact，均未过期且名称绑定完整提交。
成品 artifact archive SHA-256 为：

| 平台 | GitHub artifact SHA-256 | 下载后成品 SHA-256 |
|---|---|---|
| Linux X64 | `8e3a4eadc8594b8d59724508cd91378764bcdb8b39a3d40a51cc2f9bdbdddce5` | DEB `3370c2d4dac10d1e23c72a3aa6bf479b2a3fcec10f192ea1b64b32d8ff99c7e5` |
| macOS ARM64 | `d37777979d9521b8fac6a8f4ffa6ca7ddcfe8400a1e841c601d4c5e694f12116` | `.app` 内 worker `b37188b486eba7326682af82ba5a28847740e1b16dcdf7c2a780a13540619ffe` |
| Windows X64 | `8a6711dd0c9f99770f78ebc03273673a34f5be516ee46349417a70c00982e4e0` | NSIS `486b0e374568241b5a6199c5820ba5e5d5a8d7859bae49495346e1d572424183` |

独立下载复核还证明：Linux DEB 与 macOS `.app` 内清单/worker 分别和其已验证 worker
artifact 逐字节相同；三份清单声明的 worker SHA-256 与实物相同；固定 provider/version
及平台/架构正确。Windows 成品内部一致性由托管 job 在归档前重放，下载后的 NSIS 摘要
另行固定。对全部下载文件执行常见 token、云访问密钥和私钥头模式扫描，结果为零命中。

## 4. 边界与后续

P3-01C 只关闭 AVIF/HEIC 切片。完整实现、安全边界和历史失败收敛见
[P3-01C 详细报告](p3-01c-readiness.md)。P3-A01 仍缺视频、音频和 PDF 的全格式期望；
P3-A02 仍缺完整恶意集合的统一取消、内存和隔离门禁。因此下一动作是 P3-01D 视频，
不得把本报告解释为 P3-01 或阶段 3 通过。
