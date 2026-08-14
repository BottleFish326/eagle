# 素材查询语言规范

> 版本：1
>
> 状态：Accepted
>
> 日期：2026-08-14

## 1. 目的

本规范定义桌面搜索栏与筛选面板共享的文本查询协议。查询只针对可重建的内存索引执行，不创建数据库、保存素材副本或修改任何磁盘文件。

## 2. 基本语法

查询由空白分隔的条件组成。空查询匹配当前内存目录中的全部素材。

| 语法 | 含义 | 示例 |
|---|---|---|
| `tag` | 必须包含该 Tag | `ui/icon` |
| `tag-a tag-b` | 多个普通 Tag 默认 AND | `ui/icon color/blue` |
| `-tag` | 排除包含该 Tag 的素材 | `-state/draft` |
| `any:(a\|b)` | 同组 Tag 为 OR | `any:(color/blue\|color/red)` |
| `namespace/*` | 匹配该命名空间下的所有后代 Tag | `ui/*` |
| `type:value` | 素材类型过滤 | `type:image` |
| `ext:value` | 扩展名过滤 | `ext:png` |
| `favorite:value` | 收藏状态过滤 | `favorite:true` |
| `tag:value` | 显式 Tag；用于包含冒号的 Tag | `tag:source:camera` |

多个 `any:(...)` 组之间使用 AND；每个组内部使用 OR。例如：

```text
any:(color/blue|color/red) any:(usage/hero|usage/card)
```

表示颜色必须属于蓝色或红色，同时用途必须属于 hero 或 card。

## 3. 字段过滤

### 3.1 类型

允许值为 `image`、`video`、`audio`、`pdf` 和 `other`。值不区分大小写。用 `|` 可以选择多种类型：

```text
type:image|video
```

重复出现的类型条件继续合并为 OR 集合。

### 3.2 扩展名

扩展名由 1 至 32 个 ASCII 字母或数字组成，可以带一个开头的点。解析后统一转为小写：

```text
ext:.PNG|jpeg
```

重复出现的扩展名条件继续合并为 OR 集合。

### 3.3 收藏

收藏只接受 `true` 或 `false`，值不区分大小写。同一表达式同时要求两种状态属于解析错误。

## 4. Tag 与转义

- Tag 精确匹配且区分大小写；
- Tag 最长 128 个 Unicode 字符；
- `*` 只允许作为非空命名空间后的 `/*`；`ui/*` 匹配 `ui/icon` 和 `ui/icon/outline`，不匹配独立 Tag `ui`；
- `|` 是 OR 组保留字符；
- 含空白的 Tag 使用双引号，例如 `"visual style/minimal"` 或 `tag:"visual style/minimal"`；
- 反斜杠可把空白、双引号或反斜杠纳入当前 Token；`|` 始终是保留字符，不能作为 Tag 字面量；
- 未使用 `tag:` 时，带冒号的未知前缀被视为过滤器拼写错误，而不是普通 Tag。

## 5. 组合示例

```text
ui/* any:(color/blue|color/red) -state/draft type:image ext:png|jpg favorite:true
```

执行顺序不影响语义。该表达式要求：

1. 至少有一个 `ui/` 后代 Tag；
2. 颜色为蓝色或红色；
3. 不包含 `state/draft`；
4. 类型为图片；
5. 扩展名为 PNG 或 JPG；
6. 已收藏。

## 6. 错误协议

解析错误必须终止本次查询，不能转换为合法的零结果。错误对象包含：

```json
{
  "kind": "unknown-filter",
  "offset": 0,
  "token": "kind:image",
  "message": "unknown filter; use tag: to search for a tag containing a colon"
}
```

`offset` 是表达式中的零基字节位置。错误类型覆盖未闭合引号、尾部转义、空 Tag、超长 Tag、非法通配符、非法 OR 组、未知过滤器、非法类型、非法扩展名、非法收藏值和冲突收藏值。

## 7. 返回协议

成功结果包含：

- 原始 `expression`；
- 解析并规范化后的结构化 `query`；
- 按素材运行时键排序的 `keys`；
- 查询时内存目录的 `totalAssets`。

返回键而非重复传输完整素材记录。桌面 UI 使用扫描批次已持有的 `AssetRecord` 映射结果；索引被删除或应用重启后，通过重新扫描文件和 Sidecar 恢复。
