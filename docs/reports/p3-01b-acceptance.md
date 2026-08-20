# P3-01B SVG 验收报告

- 状态：Completed locally
- 日期：2026-08-20
- 范围：P3-01B；不代表 P3-01、P3-A01 或 P3-A02 通过
- 前置切片：P3-01A Completed locally

## 1. 验收结论

SVG 的安全静态子集已具备有界属性和 PNG 预览。扫描器继续把文件系统作为唯一真相源，只把当前文件派生宽高放入运行期 `AssetRecord`；缩略图写入应用派生缓存，不修改 SVG 或 Sidecar。脚本、事件、动画、`foreignObject`、DTD、外部 URL/文件和 data image 均不会执行或读取。

## 2. 固定实现边界

| 项目 | 判定 |
|---|---|
| 依赖 | `resvg = 0.48.1`、`usvg = 0.48.1`、`roxmltree = 0.21.1` 精确锁定；均为 MIT/Apache-2.0 兼容许可 |
| 构建 feature | resvg/usvg 默认 feature 全关；无系统字体、内存映射字体、SVGZ、raster image 或动态资源加载 |
| 输入 | UTF-8，最大 16 MiB；XML 最大 100,000 节点；DTD 和外部 entity resolver 关闭 |
| 内容隔离 | 拒绝脚本、事件属性、动画、`foreignObject`、非本地 fragment 的 href、外部 CSS URL、`@import` 与 `javascript:` |
| 暂不支持 | `<text>` 明确返回 `preview-unavailable`，不静默渲染为缺字图片 |
| 尺寸 | usvg 规范化 viewport；宽高必须有限、非零且单边不超过 65,535 |
| 输出 | `safe-static-svg` provider 生成透明 PNG，不放大源 viewport，请求单边继续受 16–2,048 限制 |
| 缓存 | provider ID/version 进入 cache key、entry descriptor 和 ready 回执 |

## 3. 固定夹具与可重复证据

清单包含四个仓库生成且逐字节记录 SHA-256 的 SVG：正常、脚本、外部引用和截断。正常夹具对应 16 × 16 PNG 参考；测试重新渲染后要求 PNG 字节完全一致。超 16 MiB 输入由测试在隔离内存中构造，不把大填充文件提交到 Git。

```bash
npm run verify:format-fixtures
cargo test -p asset-svg -p asset-filesystem -p asset-preview
cargo clippy -p asset-svg -p asset-filesystem -p asset-preview --all-targets -- -D warnings
npm run ci
```

局部结果：`asset-svg` 5 项、filesystem 45 项、preview 20 项测试通过；格式清单接受 4 个源文件、507 字节源内容和 95 字节参考 PNG。完整 CI 在提交后的干净工作树执行。

## 4. 未关闭范围

- SVG 文本的固定内置字体与确定性字体许可方案；当前明确降级；
- 把固定超大 SVG 纳入 P3-A02 正式资源/取消证据；
- P3-01C 至 F 的图片 codec、视频、音频和 PDF provider；
- 全格式 P3-A01 一致性与 P3-A02 隔离门禁。

下一动作固定为 P3-01C AVIF/HEIC 通用属性与受控 codec 边界。
