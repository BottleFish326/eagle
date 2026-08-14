# Material Bridge prototype

阶段 0 Obsidian 桌面插件原型，用于验证 Vault 外素材的稳定 ID 解析和安全边界。

## 已验证能力

- 解析 `material://<uuid>`；
- 递归读取授权根内的相邻 `*.asset.yml`；
- 不跟随符号链接目录；
- 每次读取前执行 realpath 和授权根边界校验；
- 仅允许白名单图片 MIME；
- 将小于 25 MiB 的外部图片转换为对象 URL；
- Markdown 视图释放时撤销对象 URL；
- 素材管理器未运行时独立建立最小索引。

## 本地验证

```bash
npm install
npm run check
npm test
npm run build
```

生成的 `main.js` 是构建产物，默认不提交 Git。要在测试 Vault 中试装，将下列文件复制到：

```text
<vault>/.obsidian/plugins/material-bridge/
```

所需文件：

```text
main.js
manifest.json
```

插件只应用于专用测试 Vault。当前版本是安全与协议原型，不是可发布版本；搜索选择器、反向引用和大媒体流式读取属于阶段 4。
