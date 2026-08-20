# Obsidian 兼容、降级与插件发布协议

本文细化 ADR-035，供 P4-07/P4-08 实现和 P4-A11/A12/A13 验收。本文记录的是发布门禁；版本号必须在实际 release candidate 冻结日由官方稳定渠道解析并写入证据，不能把本文日期或 npm type definition 版本当成运行时兼容证明。

## 1. 支持声明

正式 manifest 必须满足：

```json
{
  "id": "material-bridge",
  "name": "Material Bridge",
  "version": "x.y.z",
  "minAppVersion": "verified.minimum",
  "description": "Search, insert, and render approved filesystem material assets.",
  "author": "BottleFish",
  "isDesktopOnly": true
}
```

`id` 发布后不可变；name/description 需在社区目录查重并通过官方字符、长度和措辞规则。description 不超过 250 个字符并以标点结束。没有实际 funding URL 时不得加入占位字段。

### 1.1 RC 版本冻结

在 release candidate 分支冻结时记录：

```text
CompatibilityTarget {
  resolvedAt,
  sourceUrls,
  stable: { version, installerBuild? },
  previousStable: { version, installerBuild? },
  obsidianApiTypesVersion,
  minAppVersion,
  pluginVersion,
  sourceCommit
}
```

- stable/previousStable 来自 Obsidian 官方稳定发布渠道；Insider/beta 不替代；
- `obsidian` devDependency 固定为精确版本，package-lock 同步且 clean install 不变；
- `minAppVersion` 由最早实际 API/行为需求决定，并至少在该版本运行一次 core smoke；
- `versions.json[pluginVersion]` 必须等于 manifest `minAppVersion`；
- 版本冻结后上游发布新稳定版不让正在验收的矩阵漂移；下一个 RC 再更新。

## 2. 能力与兼容分层

| 层级 | 必需能力 | 缺失行为 |
|---|---|---|
| Core | Plugin lifecycle、Markdown postprocessor、Vault desktop adapter、安全文件读取 | fail closed，插件自检失败，不扫描外部根 |
| Insert | MarkdownView、Editor transaction/selection/value、command/modal | 禁用搜索插入，现有引用仍按 Core 渲染 |
| Reverse index | Vault Markdown enumeration/read/events、稳定 AST parser | backlink 标 incomplete/unavailable，不返回空的假完整结果 |
| Navigation | workspace openLinkText、active Markdown editor、desktop deep link | 隐藏/禁用定位动作并显示原因 |
| Media | Blob/object URL 或本机 Range lease server | 对应格式显示 unsupported，占位不改 Markdown |
| Desktop IPC | Unix socket/Windows pipe 和 endpoint discovery | 显示 offline，使用本地 Phase A/B，不丢旧引用 |

所有探测在 plugin load/self-check 中完成，结果是具名 capability，不直接读取未公开 Obsidian 内部字段。核心失败后 teardown 已创建资源，Notice/设置页提供稳定错误，不在循环中刷屏。

## 3. 平台矩阵

每个 RC 至少执行：

| 轴 | 值 |
|---|---|
| Obsidian | 冻结日 stable、previous stable |
| OS | 当前支持的 Windows、macOS、Linux 最低/主版本组合 |
| Vault | 英文 ASCII、本地化中文 + 空格，另含同名/长路径夹具 |
| Desktop app | running/connected、stopped/offline、protocol mismatch |
| Plugin | enabled、disabled/Restricted mode、reload、uninstall |
| Root | local、removable offline/reconnect、revoked、moved、duplicate ID |
| Reference | Vault internal、external stable ID、missing/unsafe/unsupported |

两版 Obsidian 都执行 P4-A01/A02/A03/A06/A07/A09/A10/A11 的核心链路；其余组合按 pairwise 覆盖，安全边界和文件摘要测试不得只跑单版。测试记录 app version/build、Electron/Node runtime、OS/filesystem、plugin/source commit、manifest、步骤、截图/JSON、日志摘要和结果。

## 4. 无插件与移动端降级

### 4.1 行为表

| 环境 | Vault 内 `![[...]]` | Vault 外 `material://` | Bridge 写入 |
|---|---|---|---|
| 桌面插件 enabled | Obsidian 原生 | 插件按授权解析 | 仅显式 editor Insert |
| 桌面插件 disabled / Restricted mode | Obsidian 原生 | 只保留 Markdown；渲染表现不承诺 | 0 |
| iOS / Android | Obsidian 原生能力 | 只保留 Markdown；无 Bridge 渲染/恢复 | 0 |
| Obsidian Publish | Publish 原生能力 | 不支持，不上传外部素材 | 0 |
| 普通 Markdown 导出/阅读器 | 阅读器自身能力 | 不支持，源码是否展示由阅读器决定 | 0 |

P4-A13 使用含两类引用的 fixture Vault，在 iOS/Android 至少一种真机或官方支持的设备环境打开。验证插件未安装/未加载、Vault 内引用符合 Obsidian 当前原生表现、外部引用没有触发本机/网络读取、笔记/附件/Sidecar 摘要完全不变。alt text 是否显示不作为通过条件。

### 4.2 插入前告知

当冻结批次含任一外部引用时，确认页在按钮之前显示：

```text
External material references require Material Bridge on Obsidian desktop.
They do not render on mobile, Publish, exports, or other Markdown readers.
The original asset is not copied into this Vault.
```

中文界面提供等价文案。用户必须在当前说明 version 下明确勾选；状态只保存 `externalReferenceDisclosureVersion`，不保存所选素材、笔记或时间。文案版本或支持范围变化后重新确认。

## 5. Teardown 与零改写证明

`onunload`、disable、Restricted mode、生效中的更新、Vault close 与 load failure 共用幂等 `RuntimeScope.close()`：

1. 禁止新请求/preview/insert；
2. cancel selector/query/backlink/index scan；
3. 关闭 watcher、IPC、loopback listener 和活动 stream；
4. revoke media lease/object URL；
5. terminate worker/timer/event registration；
6. 清除 asset/note handle、index、approval runtime intersection 和 DOM child；
7. 记录无路径的资源计数摘要。

每个 await 有 deadline；超时资源仍从 registry 标记 failure，不能让 unload 永久挂起。测试在 enable 前、active load 中和 unload 后记录 listener/timer/socket/server/stream/objectURL/worker/queue/handle 计数以及 Markdown/素材/Sidecar/manifest 摘要。P4-A11 要求运行资源归零，文件摘要不变；插件自己的 `data.json` 可由 Obsidian 卸载流程保留或删除，但不得作为素材恢复必要条件。

## 6. 用户数据与隐私披露

README 与独立隐私文档使用表格列明：

| 数据/能力 | 为什么 | 存储/传输 | 用户控制 |
|---|---|---|---|
| Vault Markdown | 查找/插入外部引用与 backlink | 只在当前进程解析；不发送正文 | disable/uninstall |
| 批准的 Vault 外素材与 Sidecar | 建立 ID/搜索索引和渲染 | 本机读取；索引仅内存 | 桌面 manifest + 插件逐根批准 |
| Vault/根 fingerprints 与 IDs | 匹配本机授权 | plugin data；无绝对路径/token | 设置页 revoke/清除数据 |
| 当前用户 IPC | 在线查询/定位 | Unix socket/Windows pipe；不监听 TCP | 桌面/插件集成开关 |
| loopback media lease | 大媒体 Range | `127.0.0.1` 短期 opaque URL；无 CORS | 关闭 view/revoke/unload |
| 目标笔记写入 | 用户确认插入 Markdown | 一次 editor transaction | Undo |
| 诊断 | 显示错误与资源统计 | 无正文/Tag/name/绝对路径；有界 | 查看后显式复制/导出 |

明确写出：无互联网请求、无账户、无广告、无客户端/服务端遥测、无素材上传、无自更新、无数据库。未来任一事实改变必须先更新 ADR、披露和用户授权，再实施。

## 7. 插件自检与支持导出

设置页提供只读 self-check：

- plugin/manifest/protocol/schema/build；
- Obsidian/runtime/OS 能力；
- manifest discovery/permission/revision，不含路径；
- approved/online/offline root 数、索引 progress/problem counts；
- active object URL/lease/socket/watcher/worker/queue/handle 数；
- last bounded error kinds；
- no-network/telemetry build flags。

复制支持信息前显示预览；输出最多 256 KiB，使用 ADR-024 的路径/内容脱敏，不含 Vault 名、笔记路径、文件名、Tag、查询、UUID、absolute path、token、nonce、lease 或 noteHandle。插件不自行上传。

## 8. 唯一源码与 distribution export

### 8.1 Canonical tree

`integrations/obsidian-bridge` 是唯一业务源码。版本化 release metadata 也在 monorepo 审核；export 工具只读取 Git tracked、clean 的明确 allowlist：

```text
src/**
package.json
package-lock.json
tsconfig.json
esbuild.config.mjs
manifest.json
versions.json
README.md
PRIVACY.md
LICENSE
styles.css                 # 存在时
```

外部共享 LICENSE 可从仓库根的相同摘要文件复制。缺少任何必需文件、dirty tracked file、untracked allowlist collision、symlink、超限文件、绝对路径文本或 secret scanner 告警均拒绝 export。

### 8.2 Export result

工具写入由调用者提供的全新空临时目录，不删除现有目录，附加：

```text
SOURCE_COMMIT
SOURCE_TREE_SHA256
EXPORT-MANIFEST.json       # path, bytes, sha256, mode
```

同一 clean commit/Node/tool version 连续 export 两次，排除生成时间后 tree hash 必须相同。distribution repository root 完全等于 export tree；业务源码更改只能先回 monorepo，再重新 export。

当前 monorepo 已配置源码 SSH `origin`，但没有独立插件 distribution remote。export/release 工具不得把 monorepo origin 当作发行仓库，不得猜测 GitHub owner/repository、创建远程、推送、打 tag 或发布 release。用户显式配置 distribution repository 并授权对应动作后，CI 才允许校验目标 repo 与 `SOURCE_COMMIT`；对 monorepo 的源码 push 授权不自动扩展为插件发布授权。

## 9. 构建与供应链门禁

在干净 Linux runner 和至少一个本地复核环境：

1. 使用固定 Node 24 patch/container digest；
2. `npm ci --ignore-scripts`，除非逐项审核的依赖确实需要脚本；
3. `npm run check`、unit/integration/security/fixture tests；
4. 官方 Obsidian ESLint 插件和社区 review lint；
5. dependency license allowlist、`npm audit`/advisory triage、secret scan；
6. production build，不带 sourcemap、绝对路径、测试数据、动态远程代码、遥测、自更新器；
7. clean rebuild byte-for-byte 比较 `main.js`；
8. bundle import/string scan 和大小预算；
9. 生成 SHA-256 checksums 与 build provenance；
10. 在隔离 test Vault 从三个 release 文件安装并执行 smoke。

Tree-shaking 允许；minification 在公开审核通过前关闭，以便审计且避免“隐藏用途”争议。若以后打开，仍必须保留可复现构建和源码对应证明。

## 10. 版本与发布原子性

版本更新工具在单个提交中同步：

```text
package.json version
manifest.json version/minAppVersion
versions.json[version]
CHANGELOG entry
```

只允许 SemVer `x.y.z`，不覆盖已有 `versions.json` key，不删除旧映射。release job 从 annotated tag 运行并验证：

- tag 精确等于 manifest version，不带 `v`；
- tag commit 等于 `SOURCE_COMMIT`；
- root/release manifest 摘要相同；
- `main.js` 来自该 clean source build；
- `styles.css` 若 manifest UI 需要则存在，否则明确不发布；
- GitHub release assets 是独立 `main.js`、`manifest.json`、可选 `styles.css` 以及 checksums，不只依赖 source archive；
- release publish 前所有必需 jobs 通过，失败时没有部分 public release。

发布后以匿名干净 Vault 从 GitHub release 安装并验证摘要，再提交/刷新社区目录 review。任何 tag/release/community 提交都需要用户明确授权。

## 11. 发布文档清单

插件 distribution root 必须包含：

- README：功能、桌面限定、安装、授权根、两类引用、在线/离线、移动/Publish/导出限制、外部文件/本机 IPC 披露、禁用/卸载；
- PRIVACY：第 6 节的精确数据表、无网络/遥测声明、诊断预览；
- LICENSE 与第三方 notices；
- SECURITY：私密漏洞报告入口、支持版本、响应范围；
- CHANGELOG：用户可见变化、Schema/protocol/min version；
- manifest/versions；
- 无隐私 sample Vault/素材 fixture 的生成方法和摘要，不提交真实用户文件。

## 12. P4-A12/A13 正式证据

P4-A12 报告必须列出精确两版 Obsidian，而非 “latest/previous”，并逐项给出 query、insert、render、open-asset/backlink、offline、revoke、unload 的结果及中英文 Vault 截图/JSON。P4-A13 报告记录移动设备/版本、plugin unavailable、两类 Markdown 原文、Vault 内原生表现、外部引用无读取/写入以及所有文件摘要。

报告还要附社区 review 结果、release/source checksums、依赖/许可证审计和未解决缺陷。任何 P0/P1、安全高风险、源码不可复现、隐私披露不实、两版核心链路差异或移动端写入都会阻止发布。

## 13. 官方依据（冻结日复核）

- [Obsidian Developer policies](https://docs.obsidian.md/community-directory/developer-policies)
- [Submission requirements for plugins](https://docs.obsidian.md/community-directory/submission-requirements-for-plugins)
- [Submit your plugin](https://docs.obsidian.md/Plugins/Releasing/Submit%20your%20plugin)
- [Manifest reference](https://docs.obsidian.md/Reference/Manifest)
- [Mobile development](https://docs.obsidian.md/Plugins/Getting%20started/Mobile%20development)
- [Community directory FAQ](https://docs.obsidian.md/community-directory/faq)
- [Official sample plugin](https://github.com/obsidianmd/obsidian-sample-plugin)

官方流程会变化；RC 必须重新核对以上页面，并在证据中记录访问日期与实际规则。
