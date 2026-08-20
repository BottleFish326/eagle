# P3-01F 可选 PDFium 首页渲染 worker 决策

- 状态：Deferred by design；PDF 基础格式支持不受阻塞
- 日期：2026-08-21
- 范围：PDF 首页 PNG provider 的二进制分发、许可与进程隔离边界
- 决策依据：ADR-026 与 `extended-format-protocol.md`

## 1. 决策

当前阶段不把 PDFium 动态库或未固定的系统 PDF 工具打入 Material Eagle，也不从 `PATH`
调用用户安装的 renderer。PDF 保持已经验收的页数、首页面尺寸、安全结构标记与通用 PDF
卡片；首页预览在 `core-only` profile 明确返回
`unsupported-feature/pdfium-worker-unavailable`。

这是能力降级，不是文件损坏：PDF 继续进入扁平素材目录、合并相邻 Sidecar、参与
Tag/filter 和 Obsidian 引用。加密 PDF、对象流和 xref stream 也保持素材可见，但不在主进程
解析或解密；它们等待未来固定且隔离的 worker，而不是被误报为损坏。

## 2. 暂缓原因

PDFium 可按 BSD-3-Clause 许可再分发，但首页渲染不是单一 Rust crate 能完成的纯代码路径。
`pdfium-render` 采用动态绑定，实际发布物仍需为 Windows、macOS 和 Linux 分别固定 PDFium
二进制、依赖库、源版本、许可文本和摘要。仓库当前的 worker bundle 只允许精确登记的
可执行文件与运行库；尚没有一套经过三平台成品重放的 PDFium bundle，也没有针对恶意 PDF
的进程级内存、超时、输出洪泛和崩溃证据。

因此，仅凭开发机能加载 PDFium 或存在公开预编译包不能成为发行依据。把未绑定的动态库
放入应用，或回退到系统工具，会破坏可复现构建、worker 替换防护和不执行文档动作的安全
边界。

## 3. 未来重新开启的强制门禁

只有新的独立 ADR 同时关闭以下条件，才允许新增 `bundled-pdfium` profile：

1. 固定 PDFium source revision、三平台二进制和所有动态依赖的 SHA-256，记录可重现的构建
   或来源链；
2. 每个平台的 worker bundle 逐文件绑定 executable、library、manifest、许可和 provider
   version，启动前后复核摘要，不搜索 `PATH` 或系统动态库目录；
3. worker 禁用 JavaScript、表单提交、Launch、附件打开、外部 URI、网络和任意文件访问，
   只接受已授权根内复核后的单一源文件；
4. 固定 10 秒硬超时、256 MiB 解码分配、2,048 最大输出边、stdout/stderr 与响应长度上限，
   覆盖取消、崩溃、输出洪泛和源文件替换；
5. 在 Windows、macOS 和 Linux 的最终安装产物中重放普通、截断、加密、对象流、超大声明、
   脚本/动作、附件和外部引用夹具，并证明源文件与 Sidecar 逐字节不变；
6. 随最终产品保留 PDFium 与间接依赖的许可/notice，并由发布流程检查它们没有缺失或漂移。

## 4. 依据

- [PDFium BSD-3-Clause license](https://github.com/chromium/pdfium/blob/main/LICENSE)
- [pdfium-render dynamic linking documentation](https://github.com/ajrcarey/pdfium-render)
- [PDFium pre-built binaries project](https://github.com/bblanchon/pdfium-binaries)

本决策只关闭 P3-01F 的显式缺失路径；不关闭 P3-A01、P3-A02、P3-01 或阶段 3。
