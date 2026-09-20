# 旧版发布说明归档

这里存放**每个已发布版本发布时的说明快照**（`vX.Y.Z.md`），用途：

- 回溯「某个版本到底改了什么」，不依赖 GitHub Releases 页面；
- 自建更新清单（`settings.updateManifestUrl`）或其它分发渠道需要按版本取说明时，可以直接引用；
- 版本号与正文以 `CHANGELOG.md` 为准 —— 这里的文件**由脚本生成，不要手改**。

## 生成 / 更新

```bash
npm run notes:archive     # 从 CHANGELOG.md 重新生成所有已发布版本的快照
npm run notes 0.1.1       # 只看某个版本的发布正文
npm run notes:check       # 发版门禁：三处版本号一致 + 当前版本在 CHANGELOG 里有说明
```

`docs/releases/README.md`（本文件）是唯一手写内容，其余 `v*.md` 都是生成物。
新增版本时：先改 `CHANGELOG.md`，再执行 `npm run notes:archive` 并提交生成结果；漏掉时 CI 会给出提示。
