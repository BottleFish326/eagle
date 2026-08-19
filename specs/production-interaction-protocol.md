# 生产级网格、预览与可访问性交互协议

本文定义 P3-07 的后端有序结果、多密度虚拟化、焦点/选择、预览、命令面板、Tag 工作流、任务中心和 P3-A10/A11 验收。实施等待阶段 2 退出。

## 1. 有序查询结果

正式请求：

```text
QueryViewInput {
  expression,
  scope,
  sort: { field, direction }
}

QueryViewResult {
  catalogRevision,
  normalizedQuery,
  normalizedSort,
  orderedKeys,
  totalAssets,
  matchedAssets
}
```

支持字段：`file-name`、`modified-at`、`created-at`、`file-size`、`rating`、`asset-kind`。比较规则：

- 文件名按 NFC Unicode 码点序，不使用当前系统 locale；
- 时间、大小和评分按整数；
- 素材类型按 `image, video, audio, pdf, other` 固定序；
- null 在两种方向都位于非 null 之后；
- 主字段相同后始终以 runtime key 码点升序决胜。

结果是当前 revision 的运行期序列，不落盘。排序改变生成新 revision-bound view；P3-04 快照只从一份明确 view 物化。

## 2. 多密度虚拟网格

| 密度 | 目标最小卡片宽度 | 用途 |
|---|---:|---|
| `compact` | 120 CSS px | 大规模浏览 |
| `comfortable` | 172 CSS px | 默认 |
| `large` | 260 CSS px | 比较细节 |

数值是布局起点，不是固定列数。测量模型输入容器内容宽度、视口高度/偏移、实际标题 chrome 高度、间距、字体缩放和 density；输出列数、固定 item/row 高、总高度、窗口边界和 overscan。

- 只挂载视口上下至少 3 行，并以滚动速度自适应到最多 8 行；
- 任意时刻卡片 DOM 上限为 `visibleRows × columns + overscan` 的计算结果，L 验收硬上限 200；
- 标题单行截断，完整名称通过可访问名称/tooltip 提供；
- 200% 字体缩放下若标题 chrome 需要增高，测量模型统一增高整行，不允许卡片单独撑开；
- 列数改变时以 focus key 所在行重新定位，不能按旧 index 像素盲跳；
- `aria-setsize`/`aria-posinset` 使用完整匹配集合。

缩略图请求边长按卡片预览框与 DPR 向上取到 `64/128/256/512/1024/2048`；不得超过 2,048 或为不可见全量预热。

## 3. 焦点、选择和层级

键盘：

| 输入 | 行为 |
|---|---|
| Arrow keys | 按当前列数移动，边界不循环 |
| Home / End | 当前完整结果首项/末项 |
| Ctrl/Cmd+A | 为当前 view 建立精确全选快照 |
| Shift+Arrow / Shift+click | 从 revision-bound anchor 建立范围 |
| Space | 打开/关闭快速预览，不切换按钮选择 |
| Enter | 打开检查器并保持网格返回点 |
| `/` | 聚焦查询；文本输入中不拦截 |
| Ctrl/Cmd+K | 打开命令面板 |
| Escape | 关闭最上层浮层；无浮层时清除选择需二次按键提示 |

网格用 roving tabindex：只有 focus key 对应的已挂载按钮为 0，其余 -1。虚拟滚动先调整窗口，下一 animation frame 聚焦；若 key 已消失则聚焦带 `tabindex=0` 的列表容器并通过 polite live region 报告结果计数。

对话框采用焦点约束、可见标题和关闭按钮；嵌套层级固定为 command palette > conflict/batch dialog > fullscreen/quick preview > inspector。Escape 只关闭最上层，不同时清空下层状态。

## 4. 快速与全屏预览

```text
PreviewSession {
  id,
  viewRevision,
  currentKey,
  mode: quick | fullscreen,
  capability,
  objectUrlOrStream?,
  status
}
```

- 快速预览优先静态高分辨率派生图；视频/音频/PDF 只在 P3-01 capability available 时显示受控动态控件；
- 左/右键按 frozen view 顺序切换，当前 key 丢失时显示失效状态，不悄悄换到别项；
- 切换前取消旧请求并释放 object URL/stream/worker request；
- 全屏仍显示文件名、类型和关闭入口，隐藏内容不等于移除可访问名称；
- 自动播放默认关闭，动画/运动遵循 `prefers-reduced-motion`；
- 不支持、codec 缺失、离线、超限和损坏分别显示稳定原因。

## 5. 命令面板

命令记录：

```text
CommandDescriptor {
  id,
  title,
  keywords,
  shortcut?,
  enabled,
  disabledReason?,
  category
}
```

ID 由编译期注册表提供，执行使用 enum 分派。首批覆盖聚焦查询、切换密度、改变排序、打开预览、复制引用、管理根、打开任务中心和诊断。搜索使用本地标题/关键字，不接受路径、JavaScript、shell 或任意 Tauri command 名称。

面板使用 `dialog + combobox + listbox` 语义；上下键移动 active descendant，Enter 执行，Escape 关闭，禁用项可被发现但不能执行并显示原因。

## 6. Tag 补全、规范化和重命名

新 Tag 规范：

1. Unicode NFC；
2. 去掉首尾 Unicode whitespace；
3. 1–128 个 Unicode scalar；
4. 拒绝 C0/C1 control、NUL、`|` 和 `*`；
5. 保留大小写、内部空白和 `/`；
6. 相同规范值去重。

既有 Sidecar 中不符合该规范的 Tag 仍进入 catalog，显示 `legacy-tag`，不被后台改写。UI 通过素材键/版本提供精确移除和显式重命名入口。

补全结果按：精确前缀优先 → 当前运行期最近明确使用 → catalog count 降序 → 码点升序。最近使用列表只含 Tag 文本、最多 64 项，保存在 `application.yml` 前必须单独提示其隐私含义；默认仅在内存，不随素材结果落盘。

重命名预检：

```text
TagRenamePreflight {
  operationId,
  oldTag,
  newTag,
  scope,
  assetCount,
  alreadyHasTargetCount,
  sidecarConflictCount,
  affectedSavedFilters[],
  catalogRevision
}
```

用户确认素材 scope 和每个过滤器的更新/保留选择后，Sidecar 按事务逐项把 old 移除、new 加入；已有 new 时集合自然合并。过滤器只重写 AST 中精确 Tag 节点。取消、恢复和外部变化遵循 ADR-020/027/029。

## 7. 统一任务中心

任务类型：scan、watch-reconcile、thumbnail/decode、hash/duplicate、batch、cache-maintenance、diagnostics、plugin-index。每项显示稳定 ID、文本阶段、进度/不确定状态、成功/失败/冲突计数、开始时间和可用动作。

- 高频进度原地更新；live region 最多每 5 秒或阶段变化通知；
- 错误摘要可展开，路径继续脱敏；
- 可取消只在安全检查点显示，不能提供无效取消按钮；
- 根离线是持续状态，不作为不断失败的新任务；
- 完成任务在有界运行期列表中保留，持久支持日志沿用 P2-07 限额。

## 8. P3-A10 可访问性验收

只使用键盘完成：聚焦搜索 → 输入复合查询 → 打开字段过滤 → 浏览网格 → 范围/全选 → 快速预览 → 检查器 → 复制当前 Vault 引用 → 打开任务中心 → 返回原焦点。

证据：

- 自动语义/对比度检查和无严重规则失败；
- 100%、200% 字体缩放与高对比/深浅主题；
- `prefers-reduced-motion`；
- macOS VoiceOver、Windows Narrator、Linux AT-SPI 阅读器至少完成核心路径；
- 焦点全程可见、无键盘陷阱、状态不只依赖颜色；
- 虚拟窗口外 Home/End/箭头定位准确；
- 每个平台记录版本、操作脚本、观察与截图/录屏索引。

## 9. P3-A11 L 数据集验收

使用 100,000 素材、20,000 Sidecar 和扩展格式固定比例，Release 构建：

- 60 秒预热后连续 30 分钟滚动、密度切换、六种排序、复合过滤和预览切换；
- 每秒滚动，每 8 秒切换查询，每 20 秒切换排序/密度，每 30 秒开关预览；
- 查询 p95 ≤ 100 ms，首次可交互 ≤ 3 秒，文件变化到 UI p95 ≤ 2 秒；
- 卡片 DOM ≤ 200，对象 URL/preview session 数有界；
- Long Task 总时长占比 ≤ 1%，事件循环延迟 p95 ≤ 16 ms；
- 预热后 JS heap 增长 ≤ 128 MiB；原生 RSS ≤ 1 GiB、增长 ≤ 256 MiB、线性斜率 ≤ 8 MiB/分钟；
- 句柄增长 ≤ 64，worker/调度等待不超过既有硬上限，未处理错误与结果不一致均为 0；
- 结果计数与独立 oracle 一致，随机导航 key 与有序结果一致；
- 测试前后源素材和 Sidecar 摘要不因纯浏览/预览改变。

不得通过缩短运行、截断结果、降低素材数或关闭正式预览能力获得通过。平台缺少可选 codec 时使用已声明 capability profile，并与通用卡片路径分别报告。
