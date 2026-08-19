# P4-07/P4-08 兼容、降级与发布实施准备报告

> 状态：Architecture ready；阶段 3 退出前不计作阶段 4 开始
>
> 日期：2026-08-20

## 1. 准备结论

Material Bridge 的 desktop-only 声明、两版 Obsidian 冻结矩阵、移动端/Publish/导出源码保留降级、disable/uninstall teardown、隐私披露、插件自检和可复现社区发布路径已经定案。monorepo 仍是唯一源码；社区发布仓库由 clean commit 确定性导出，当前没有 Git remote 时不会猜测或执行外部发布。

## 2. 官方要求核对

2026-08-20 已核对 Obsidian 官方开发文档与 sample plugin：

- 使用 Node.js/Electron API 的插件必须 `isDesktopOnly: true`；
- manifest 要有合法唯一 ID/name、SemVer、实际 `minAppVersion` 和简短 description；
- 社区目录要求仓库根 README、LICENSE、manifest；release tag 与 manifest version 精确相同；
- release 需要独立附加 `main.js`、`manifest.json`、可选 `styles.css`；
- `versions.json` 用于旧 Obsidian 选择最后兼容插件版本；
- Vault 外文件访问和本机传输必须清晰披露；客户端遥测、自安装/自更新和隐藏用途代码禁止；
- 自动审核检查 manifest、release、source 和 build reproduction，官方 ESLint 可本地预检。

权威链接已固定在 `specs/obsidian-compatibility-and-release-protocol.md`，每个 RC 仍需按冻结日复核，避免把本次观察永久写死。

## 3. 已完成资产

| 资产 | 结论 |
|---|---|
| ADR-035 | 固定桌面专用、降级、兼容和 distribution repository 边界 |
| `specs/obsidian-compatibility-and-release-protocol.md` | 固定能力矩阵、移动端、teardown、隐私、自检、export/build/release 门禁 |
| 现有 `manifest.json` | ID/name/desktop-only 形态可用；version/minAppVersion 仍是原型值，等待 RC 实测 |
| 现有 package-lock | 已锁到 Obsidian API typings 1.13.1；package.json 的 `latest` 必须在实现时改为精确值 |
| 现有 monorepo CI | 已有 Node 24、TypeScript/test/build 基础；尚无插件独立 export/release workflow |

## 4. 已关闭的歧义

| 问题 | 决策 |
|---|---|
| 是否支持 Obsidian mobile | 插件 desktop-only；移动端只承诺标准 Vault 引用，外部引用只保留源码 |
| 自定义引用在 Publish/导出 | 不渲染、不上传外部素材，具体占位不承诺 |
| 何时告诉用户限制 | 外部引用插入确认前显示，文档/设置同时披露 |
| “当前版/上一版”如何验收 | RC 冻结日解析精确版本并锁定，不随测试期间上游发布漂移 |
| minAppVersion | 取实际 API 最低版本并实测，不沿用 1.7.0 原型占位 |
| monorepo 如何满足插件根布局 | 确定性 export 到独立公开 distribution repository，禁止手工双维护 |
| 是否允许遥测/互联网 | 当前正式版本全部禁止；本机 IPC/loopback 明确披露 |
| release 可追溯性 | SOURCE_COMMIT/tree manifest、精确 lock、可复现 main.js、checksums/provenance |
| 外部发布权限 | 用户提供并确认远程仓库/账户后才能 push/tag/release/submit |

## 5. 实施切片

### P4-07A：Capability compatibility layer

- 正式 plugin lifecycle/resource registry；
- Core/Insert/Reverse/Navigation/Media/IPC 具名探测；
- fail-closed core 与可选能力降级；
- 禁止内部 API/`as any` 伪兼容。

### P4-07B：无插件与移动端体验

- 外部引用插入 disclosure version；
- disabled/Restricted/mobile/Publish/export 行为文档与 fixture；
- iOS/Android 至少一种设备环境 P4-A13；
- 0 网络、0 自动复制、0 Markdown 改写证明。

### P4-07C：双版本矩阵与 teardown

- RC 精确 stable/previous stable 解析清单；
- 三 OS、中英文 Vault、online/offline pairwise；
- unload resource registry/deadline/幂等测试；
- P4-A11/A12 文件摘要和资源归零证据。

### P4-08A：用户文档、隐私与自检

- 正式 README/PRIVACY/SECURITY/CHANGELOG/LICENSE/notices；
- Vault 外读取、IPC/loopback、写入/undo、无遥测/网络披露；
- 256 KiB 脱敏 self-check/support export；
- 无隐私 sample fixture 与摘要。

### P4-08B：Deterministic export

- tracked clean allowlist、symlink/secret/path 拒绝；
- SOURCE_COMMIT/tree hash/export manifest；
- 双 export reproducibility test；
- distribution repo 漂移检查。

### P4-08C：Build/release gate

- 精确 Obsidian API dependency、versions/manifest/package 原子 bump；
- official ESLint、license/advisory/secret/bundle scan；
- clean rebuild byte equality、release checksums/provenance；
- isolated Vault release-install smoke 和 community branch review。

## 6. 通过条件

- `isDesktopOnly=true` 与 Node/IPC 实现及全部用户文档一致；
- P4-A11 teardown 后所有受管资源归零，Markdown/素材/Sidecar/manifest 摘要不变；
- P4-A12 在报告中记录的两版 Obsidian 与三平台矩阵核心链路一致；
- P4-A13 移动端标准引用按宿主工作，外部引用不读/写/上传且源码保持；
- README/PRIVACY 对 Vault 外文件、本机 IPC/loopback 和目标笔记写入说明准确；
- 无 client/server telemetry、互联网、账户、广告、自更新、数据库或素材上传；
- distribution source 可从单一 monorepo commit 重建，`main.js` byte-for-byte 相同；
- manifest/package/versions/tag/release assets 一致，官方 review 无 error/high-risk；
- 全部 P4 验收、插件安全评审和 release gate 通过后才允许用户执行发布。

## 7. 当前阻塞与未完成

- 阶段 3 尚未退出，因此 P4-07/P4-08 实现未开始；
- `manifest.json` 的 0.1.0/1.7.0 仍是原型值，不能作为发布声明；
- package.json 仍写 `obsidian: latest`，虽然 lockfile 当前解析 1.13.1；
- 仓库尚无根 LICENSE，插件尚无 versions/PRIVACY/SECURITY/CHANGELOG/styles；
- production bundle 当前 minify，尚未切换审核友好的 release profile；
- 未实现 export/version/release scripts 或独立插件 workflow；
- 未配置 Git remote/distribution repository，也未获得任何 push/tag/release/community submit 授权；
- 未执行 P4-A11/A12/A13 或社区目录 review。

以上缺口必须按切片关闭；本报告不表示插件已可发布。
