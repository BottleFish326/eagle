# 高级查询与智能属性协议

本文定义 P3-02 对查询语言版本 1 的兼容扩展。实现门禁为阶段 2 退出；当前协议用于锁定字段、单位、缺失值、规范化和 P3-A03 验收边界。

## 1. 字段与语法

高级谓词保持单个空白分隔 Token 的形式：

```text
field:[operator]value
```

比较符为 `=`, `<`, `<=`, `>`, `>=`，省略时为 `=`。字段名和值枚举使用小写 ASCII；字符串值沿用版本 1 的双引号和反斜杠转义。

| 字段 | 值 | 示例 | 缺失值 |
|---|---|---|---|
| `rating` | 整数 `0..5` | `rating:>=4` | 不可缺失，0 表示未评分 |
| `size` | 非负整数和可选二进制单位 | `size:>=10MiB` | `size:unknown` |
| `width`, `height` | 正整数像素 | `width:>=1920` | `width:unknown` |
| `aspect` | 正整数分数 `n/d` | `aspect:>=16/9` | `aspect:unknown` |
| `created`, `modified` | RFC 3339 时间 | `modified:>=2026-08-19T00:00:00+08:00` | `created:unknown` |
| `duration` | 非负整数和时长单位 | `duration:>=30s` | `duration:unknown` |
| `pages` | 正整数 | `pages:>=2` | `pages:unknown` |
| `orientation` | `landscape`, `portrait`, `square`, `unknown` | `orientation:landscape` | 只由 `unknown` 匹配 |
| `root` | 根 UUID，可用 `\|` 表示 OR | `root:0198...\|0199...` | `root:unknown` |
| `path` | 非空根内相对路径子串 | `path:"Brand Assets/icons"` | 不允许 `unknown` |
| `color-space` | 提供器规范化名称，可用 `\|` 表示 OR | `color-space:srgb\|display-p3` | `color-space:unknown` |
| `has-note` | `true` 或 `false` | `has-note:true` | 不可缺失 |
| `has-alpha` | `true`, `false`, `unknown` | `has-alpha:true` | 只由 `unknown` 匹配 |

版本 1 的 `type`, `ext`, `favorite` 和 Tag 条件继续有效。例如：

```text
ui/* type:image rating:>=4 width:>=1920 aspect:>=16/9 modified:>=2026-01-01T00:00:00Z -state/draft
```

## 2. 单位与边界

### 2.1 文件大小

- 无后缀或 `B`：字节；
- `KiB`, `MiB`, `GiB`, `TiB`：分别乘以 1024 的 1–4 次方；
- 单位大小写必须精确，数值只接受十进制整数，不接受小数、负数、千位分隔符或科学计数法；
- 解析时必须检查 `u64` 溢出。

### 2.2 时长

- 无后缀或 `ms`：毫秒；
- `s`, `min`, `h`：分别乘以 1,000、60,000、3,600,000；
- 只接受十进制整数并检查 `u64` 溢出。

### 2.3 宽高比和方向

`aspect:n/d` 的分子、分母范围均为 `1..1,000,000`，分母不得为零。解析后按最大公约数约分；比较 `a/b` 和 `c/d` 时使用带溢出检查的扩大整数交叉乘法，不转为浮点数。

有效宽度大于高度为 `landscape`，小于为 `portrait`，相等为 `square`。EXIF 方向 5–8 或等效视频旋转元数据先交换宽高。任一尺寸缺失时，方向和宽高比均为 `unknown`。

### 2.4 时间

时间必须符合 RFC 3339 且包含 `Z` 或 `+/-HH:MM` 偏移。解析后比较 Unix 毫秒；高于毫秒的精度按向零截断并由规范化序列化器输出毫秒精度。无偏移日期、相对日期（如 `today`）和受系统时区影响的值都属于 `invalid-date`。

## 3. 组合与缺失值

- 不同字段使用 AND；
- 同一类别字段的多个值使用 OR，例如 `orientation:landscape|square`；
- 同一数值字段的多个条件组成闭/开区间，例如 `width:>=1024 width:<4096`；
- 多个 `path:` 条件全部必须匹配；
- 普通比较永不匹配 `null`/未知属性，只有显式 `unknown` 匹配；
- `unknown` 不能与同字段范围条件或已知枚举同时出现；
- `has-note:true` 表示备注去除 Unicode 空白后非空；
- `has-alpha:false` 只匹配明确提取出无 Alpha，不匹配未安装 codec 或提取失败。

范围归一化后下界高于上界、相等边界被开区间排除、布尔值互相冲突或 `unknown` 与已知条件并存，都返回结构化冲突错误。

## 4. 路径和根目录

查询只使用 `relativePath` 与 `rootId`：

- `relativePath` 先把平台分隔符映射为 `/`，再做 Unicode NFC；
- `path:` 是区分大小写的连续 Unicode 子串匹配，不解析 glob、正则、`..`、盘符或绝对路径；
- `root:` 值必须是规范小写 UUID；它是过滤条件，不授予根目录访问权；
- 保存过滤器的外层 scope 先限制可见根，表达式内 `root:` 再取交集；离线、停用或已移除根不会被隐式改写。

## 5. AST 与执行

解析结果保留现有 Tag/类别字段，并增加定型谓词。概念结构：

```text
TypedPredicate =
  IntegerRange(field, lower?, upper?)
  InstantRange(field, lower?, upper?)
  RatioRange(field, lower?, upper?)
  EnumSet(field, values)
  Boolean(field, value)
  Unknown(field)
  PathContains(value)
```

执行器先以集合索引求出低基数候选，再对候选执行范围和路径谓词。无条件查询仍返回全部运行时记录。排序是查询后的独立稳定步骤：主键相同或缺失时以素材运行时 `key` 升序决胜，确保桌面 UI、保存过滤器和验收工具结果一致。

## 6. 错误协议

沿用版本 1 的零基 UTF-8 字节 `offset`。新增稳定错误种类：

```text
invalid-operator
invalid-integer
invalid-unit
numeric-overflow
invalid-ratio
invalid-date
invalid-enum
invalid-root-id
invalid-path
unsupported-unknown
conflicting-range
conflicting-value
```

解析失败必须保留上一次有效结果并显示错误位置；不得把错误查询作为空查询、零结果或字符串 Tag 执行。

## 7. 独立验收

P3-A03 的固定语料遵循 [`query-conformance-manifest.schema.json`](../schemas/query-conformance-manifest.schema.json) 和[查询一致性语料规范](query-conformance-manifest.md)。最少覆盖：

- 每个字段的等于、边界和缺失值；
- 开区间、闭区间、相等边界和冲突区间；
- 大小/时长单位及溢出；
- 宽高比精确交叉乘法和 EXIF 旋转；
- 同一绝对时刻的不同 RFC 3339 偏移；
- Unicode/大小写路径、未知根和跨根 scope；
- codec 缺失与明确 `false` 的区别；
- 合法组合与每种新增解析错误。

独立验证器以 oracle 谓词线性扫描记录，不导入产品 parser/index。每个用例依次证明：oracle 结果等于提交的固定 `expectedKeys`，产品结果也等于同一集合，且输入记录和原始素材摘要未被修改。
