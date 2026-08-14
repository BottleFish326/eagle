# 阶段 0 验收报告

> 状态：Accepted，阶段退出条件已满足
>
> 日期：2026-08-14
>
> 核心原型基线：`fbc9391`；本报告及可靠性补强随所在提交版本化

## 环境

- Apple M4、16 GiB、APFS SSD；
- macOS 26.5.2；
- Rust 1.97.1；
- Node.js 24.19.0 LTS（Node 官方发布包 SHA-256 校验通过）；
- Obsidian 1.12.7；
- Obsidian CLI 全局设置：已启用。

## 工作项结果

| 工作项 | 结果 | 证据 |
|---|---|---|
| P0-01 ADR | Pass | ADR-001 至 ADR-008 均为 Accepted |
| P0-02 夹具生成器 | Pass | S/M/L、固定规则、异常路径和标记保护清理 |
| P0-03 扫描 | Pass | 100,000 素材 1.454 秒完成 |
| P0-04 索引 | Pass | 独立线性过滤对照通过；查询 p95 19.082 ms |
| P0-05 Sidecar | Pass | 原子替换、并发摘要、未知字段及进程崩溃测试通过；CLI 提供 abort/reload/merge 冲突策略 |
| P0-06 监听 | Pass on macOS | 10,000 文件创建、移动和删除事件风暴最终收敛 |
| P0-07 Obsidian | Pass | 构建、自动化安全测试和 Obsidian 1.12.7 实机渲染通过 |

## 验收用例

| ID | 状态 | 结果 |
|---|---|---|
| P0-A01 | Pass | 删除缓存不影响扫描；S/M/L 记录与生成清单一致，单文件问题隔离 |
| P0-A02 | Pass | 10,000 条独立线性对照完全一致；100,000 条、1,000 次查询 p95 19.082 ms |
| P0-A03 | Pass | `before-temp`、`after-temp-sync` 保留原 Sidecar；`after-persist` 得到完整新 Sidecar |
| P0-A04 | Pass | 外部修改后摘要冲突被拒绝，外部内容未覆盖；用户可通过 `--on-conflict reload|merge` 显式选择处理方式 |
| P0-A05 | Pass on macOS | 创建 10,000、移动 10,000、删除 5,000 个素材；68,362 个归一化事件后完整扫描收敛到 5,000 个素材、0 问题，监听进程峰值 RSS 9,338,880 字节 |
| P0-A06 | Pass | Vault 内生成标准 `![[internal.png]]`；停用插件并重新渲染后仍为完整的 `app:` 图片，`naturalWidth=1` |
| P0-A07 | Pass | 外部稳定 ID 被转换为完整的 `blob:` 图片，`naturalWidth=1`；Vault 内 PNG 总数仍为 1，未复制外部素材 |
| P0-A08 | Pass | 伪造 URI、查询参数、路径逃逸、文件/目录符号链接逃逸均被拒绝 |

## Sidecar 故障注入

启用 `metadata/fault-injection` 特性，通过独立进程在三个位置执行 `abort`：

```text
before-temp       原文件摘要保持不变
after-temp-sync   原文件摘要保持不变
after-persist     新文件完整、可解析、扫描问题数为 0
```

结论：当前 macOS/APFS 实现满足“旧版本完整或新版本完整”，没有出现截断正式 Sidecar。崩溃可能遗留临时文件，清理策略属于阶段 2。

## 文件监听事件风暴

过程：

1. 对空目录启动递归监听，主线程持续消费事件；
2. 生成 10,000 个素材和 2,000 个 sidecar；
3. 逐个移动 10,000 个素材及对应 sidecar；
4. 删除 5,000 个素材及对应 sidecar；
5. 收到 68,362 个归一化事件：13,189 create、25,193 modify、24,000 move、5,980 delete、0 rescan-required；
6. 事件窗口结束后执行完整扫描。

最终完整扫描得到 5,000 个素材、0 个问题，与操作清单一致。监听进程在 21.71 秒测试中的峰值 RSS 为 9,338,880 字节；事件重复没有造成失控内存占用，完整扫描仍是最终一致性来源。

## 跨平台检查

- macOS ARM64：编译、测试、监听和性能实测通过；
- Linux x86_64 GNU：workspace 全目标 `cargo check` 通过；
- Windows x86_64 MSVC：workspace 全目标 `cargo check` 通过；
- Windows/Linux 运行时监听和文件系统语义尚未执行。

## Obsidian 实机渲染

在带清理标记的隔离 Vault 中使用 Obsidian 1.12.7 和官方 CLI 执行：

1. 启用并重载 `material-bridge`，打开 `smoke.md` 的阅读视图；
2. 外部 `material://0198a7c2-8341-7a31-b842-f15d39f33c18` 被解析为 `blob:` URL，图片 `complete=true`、`naturalWidth=1`；
3. Vault 内 `![[internal.png]]` 保持 Obsidian `app:` URL，图片 `complete=true`、`naturalWidth=1`；
4. 插件索引只有 1 个授权根外素材，Vault 文件清单只有 `internal.png`，确认没有复制外部素材；
5. 调试器捕获到 0 个运行时错误、0 个 error 日志、0 个 warning 日志；
6. 停用插件后，旧对象 URL 无法继续读取，证明 `URL.revokeObjectURL` 已执行；
7. 重新渲染后外部引用按预期不可用，Vault 内标准引用仍完整渲染。

证据：

- [启用插件时的渲染截图](evidence/phase-0-obsidian-enabled.png)
- [停用插件后的渲染截图](evidence/phase-0-obsidian-disabled.png)

## 技术风险与阶段 1 估算依据

| 风险 | 当前控制 | 阶段 1 处理与估算影响 |
|---|---|---|
| 平台监听事件不一致 | 统一事件类型，允许回退完整扫描 | 建立三平台 CI/实体机矩阵；P1-03 预留 20% 平台适配工作量 |
| Sidecar 并发和崩溃 | 摘要乐观锁、同目录原子替换、故障注入 | P1-04 重点实现冲突 UI 和临时文件恢复，不重写存储协议 |
| 100,000 项 UI 渲染压力 | 核心扫描与查询已达到基线 | P1-06/P1-07 使用虚拟列表、懒缩略图和并发限流 |
| Vault 外文件访问越界 | 授权根、realpath 复核、严格 UUID | 外部引用在阶段 1 只保留原型，不扩大发布承诺 |
| 文件移动后的身份重联 | ADR-004 使用 ID/指纹分级处理 | P1-03 先实现同根移动；模糊重联留到阶段 2 |

阶段 1 的工作量依据是九个工作包：工程/CI 1 份、核心文件系统与元数据 3 份、查询与缩略图 2 份、桌面 UI 1 份、Vault 内联动 1 份、配置恢复 1 份。首轮估算按 1 名全职开发者 8–10 周，或 2 名开发者 5–6 周；平台签名、公证和外部引用正式发布不计入该估算。进入阶段 1 前应把工作包拆成可在 1–3 天内验收的任务，并以本报告基线重新估算。

## 阶段结论

阶段 0 标记为 Accepted。ADR-001 至 ADR-008、P0-A01 至 P0-A08、性能基线、Sidecar 安全性和 Obsidian 外部渲染路线均已通过。Windows/Linux 运行时验证仍是公开发布前的必要工作，但不再阻断阶段 1。下一步可以按照开发计划进入阶段 1 的工程骨架与端到端 MVP 实现。
