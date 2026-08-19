# ADR-035：Obsidian 桌面专用兼容、源码保留降级与可复现发布边界

- 状态：Accepted
- 日期：2026-08-20
- 对应：P4-07、P4-08、P4-A11 至 P4-A13
- 实施门禁：阶段 3 退出后

## 背景

Material Bridge 需要 Node.js 文件系统 API、Unix socket/Windows named pipe 和本机 loopback media server。把这些能力伪装成移动端兼容会在 iOS/Android 加载失败，也会让用户误以为 Vault 外引用可以随 Vault 同步。自定义 `material://` 不是标准 Markdown 阅读器的可移植媒体协议；Publish、导出和第三方阅读器不能被承诺为可渲染。

当前代码位于包含 Rust/Tauri 桌面端的 monorepo 子目录，而 Obsidian 社区发布会从插件仓库根读取 README、LICENSE、manifest 和版本信息，并验证 GitHub release 与源码构建。手工复制一个长期分叉的插件仓库会造成源码、lockfile、manifest 或安全说明漂移。

## 决策

1. 正式插件继续设置 `isDesktopOnly: true`。不得通过动态 import 或吞掉 Node/Electron 错误来宣称移动兼容；桌面限定必须同时出现在 manifest、README、安装页和首次外部引用插入提示中。
2. Vault 内素材始终插入标准 `![[vault-relative-path]]`，插件禁用、未安装、Restricted mode、移动端、Publish 或第三方阅读器中仍由宿主按标准能力处理。Bridge 不接管或改写这种引用。
3. Vault 外素材只插入 `![alias](material://<uuid>)`。插件不可用时只承诺 Markdown 源码和稳定 ID 保留：不承诺媒体、alt 占位、点击或导出表现；不得在移动端/Publish 自动下载、复制、删除或改写引用。
4. 选择器首次包含 Vault 外素材时，确认页必须在 Insert 前显示“仅桌面 Bridge 可渲染；移动端、Publish、导出和第三方阅读器不支持；素材不会复制进 Vault”。该说明不能只藏在设置或文档；确认状态最多按说明版本保存，不保存素材清单。
5. 支持矩阵在每个 release candidate 冻结日解析，不用“当前版”替代证据。矩阵至少包含当日 Obsidian 最新稳定版和紧邻上一稳定版的精确版本号、三种桌面 OS、中文/英文 Vault、desktop online/offline 和插件 disabled 状态。
6. `minAppVersion` 取实际使用 API 所需且已通过兼容矩阵的最低 Obsidian 版本。它不能沿用原型占位，也不能为了扩大安装量低报；每个插件版本在 `versions.json` 记录对应 minimum。Obsidian API devDependency 在 RC 前固定为精确版本，禁止 `latest`/范围漂移。
7. API capability 在加载时显式探测并分层：核心引用解析不可用则插件 fail closed；可选 preview/定位能力不可用时只关闭该能力并给出原因。不得用 `as any`、未公开内部对象或吞异常维持伪兼容。
8. 禁用、Restricted mode、插件 reload、Vault 关闭和卸载路径都执行同一 teardown：取消队列、watcher、socket、lease/server、object URL、worker、handle 和内存索引。teardown 不修改 Markdown、素材、Sidecar、桌面 manifest 或其他插件数据。
9. 插件不包含客户端或服务端遥测，不加载广告，不要求账户/付费，不连接互联网，不自安装/自更新依赖。当前用户 socket/pipe 与 `127.0.0.1` media lease 仅是本机传输，README 仍要明确披露其用途、端口生命周期和无 CORS 边界。
10. README/隐私说明必须披露：读取当前 Vault Markdown；读取用户批准的 Vault 外素材/Sidecar；在显式 Insert 时写当前笔记；保存不含绝对路径/token 的批准设置；诊断字段与清理方法；没有互联网、遥测或素材上传。插件设置必须在批准前再次显示真实外部根。
11. monorepo 是插件源码唯一维护位置。发布工具从 `integrations/obsidian-bridge` 和审核过的根级 LICENSE/发布文档生成一个干净、可复现的 release-source tree；生成物包含源码、精确 lockfile、root README/LICENSE/manifest/versions、构建脚本和 `SOURCE_COMMIT`，不得包含本机路径、测试 Vault、素材、token、日志或未跟踪文件。
12. 社区插件使用独立公开 distribution repository，使插件文件位于仓库根且自动审核可以只构建插件。该仓库只接受带源提交摘要的确定性 export，不手工修改业务源码；远程 owner/repository 由用户建立后配置，当前不得假设或自动创建。
13. 发布 tag 必须与 manifest 的 `x.y.z` 完全一致且不带 `v`。GitHub release 逐个附加 `main.js`、`manifest.json` 和可选 `styles.css`，同时发布 SHA-256 checksums；根 manifest、release manifest、package version、versions entry 和 tag 必须一致。
14. 正式 `main.js` 不做隐藏用途的混淆。构建可 tree-shake，但 release review 必须从 `SOURCE_COMMIT`、lockfile 和固定 Node 24 在 clean tree 复现完全相同字节；依赖许可证、npm advisory、禁止模块和 bundle 内容均进入门禁。
15. 首次发布前必须在社区目录的 branch/release review、插件自检、安全评审、两版兼容矩阵和 P4-A01 至 P4-A13 全部通过后才允许提交。发布与社区目录提交是外部动作，必须由用户明确提供仓库/账户并确认执行，不能由本地开发流程自行完成。

## 影响

- 移动端不会加载一个必然失效的 Node 插件；用户仍能看到并编辑原 Markdown，Vault 内引用保持便携。
- 外部引用的局限在写入前可见，不会在同步到手机或 Publish 后才意外发现。
- 精确双版本矩阵和 `versions.json` 让旧 Obsidian 能选择最后兼容插件版本。
- 独立 distribution repository 满足社区审核的根布局，又避免长期维护第二份人工源码。
- 无遥测、无互联网和外部文件访问披露与项目的本地优先目标一致。

## 不采用

- 把 desktop-only 插件标成移动兼容并在运行时尝试补救；
- 在移动端把外部素材自动复制到 Vault 或远程服务；
- 承诺 Publish/导出/第三方阅读器渲染 `material://`；
- 只测试“最新 Obsidian”而不记录精确版本与上一稳定版；
- 永久保留 `obsidian: latest` 或未经验证的原型 `minAppVersion`；
- 从 monorepo 根直接发布一个需要完整 Rust/Tauri 构建的社区插件；
- 手工维护与 monorepo 漂移的第二份插件源码；
- 发布未锁依赖、混淆 bundle、遥测、自更新器或不披露的 Vault 外文件访问。
