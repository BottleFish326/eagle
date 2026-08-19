# P3-07 生产级交互实施准备报告

> 状态：Design ready；阶段 2 退出前不计作阶段 3 开始
>
> 日期：2026-08-20

## 1. 准备结论

P3-07 已固定后端有序查询、稳定缺失值排序、三种网格密度、key-based 焦点、快速/全屏预览、静态命令面板、Tag 规范化/重命名事务、统一任务中心和 P3-A10/A11 证据矩阵。实现不会在 React 复制查询/排序语义，也不会为恢复视图保存素材快照。

## 2. 已完成资产

| 资产 | 结论 |
|---|---|
| `specs/adr/032-production-interaction-state-and-accessibility.md` | 固定正式 UI 状态、排序、焦点、Tag 和可访问性边界 |
| `specs/production-interaction-protocol.md` | 固定网格、预览、命令、任务中心及 A10/A11 验收 |
| ADR-017 / 阶段 1 | 已有按行窗口化、对象 URL 回收、完整集合 ARIA 位置和 M 稳定性基础 |
| ADR-028/029/031 | 已固定高级查询、精确选择与授权拖放，P3-07 直接组合这些协议 |

## 3. 实施切片

### P3-07A：后端 view 与排序

- catalog 单调 revision；
- query + scope + sort 单次响应；
- null/相等字段稳定决胜；
- 独立 oracle 与更新/删除索引测试。

### P3-07B：多密度测量窗口

- 把现有固定 4:3 行高扩展为密度/字体/DPI 测量模型；
- 速度自适应 overscan 和 DOM 硬上限；
- key-based roving focus、列变更和过滤恢复；
- 只请求实际档位缩略图。

### P3-07C：预览 session

- Space 快速预览、显式全屏、结果前后导航；
- capability 降级、自动播放关闭和 reduced motion；
- object URL、stream、worker 的取消与释放测试。

### P3-07D：命令面板与任务中心

- 编译期命令注册、禁用原因和 dialog/combobox；
- 统一任务状态、节流 live region、取消能力；
- 无脚本/shell/任意 IPC 路径。

### P3-07E：Tag 工作流

- 新输入 NFC/trim/字符验证；
- legacy Tag 保留；
- catalog 补全；
- rename preflight、Sidecar 事务和保存过滤器逐项选择。

### P3-07F：A10/A11 正式验收

- 三平台键盘/屏幕阅读器关键路径；
- 100%/200%、高对比、reduced motion；
- L 数据集 Release 30 分钟交互曲线；
- 独立结果/导航 oracle 和素材保护摘要。

## 4. 通过条件

P3-07 只有同时满足以下条件才完成：

- P3-A10 全键盘流程和三平台辅助技术人工证据通过；
- 焦点、选择、预览和拖放都绑定正确 asset key/revision；
- Tag 规范化不静默改写既有 Sidecar，重命名可取消/恢复且过滤器选择准确；
- P3-A11 在完整 L 数据集达到统一查询/首屏/变化延迟基线；
- 30 分钟内 DOM、对象 URL、heap/RSS、句柄、worker 和队列保持有界；
- 纯浏览/排序/预览前后源素材与 Sidecar 摘要不变；
- 未引入 IndexedDB、LocalStorage、数据库或结果快照。

## 5. 尚未开始

- 未扩展 catalog sort/revision API；
- 未实现多密度、preview session、命令面板、任务中心或 Tag rename UI；
- 未执行 P3-A10/A11；
- 未把阶段 3 状态改为 In progress。

以上实现等待阶段 2 退出评审完成后开始。
