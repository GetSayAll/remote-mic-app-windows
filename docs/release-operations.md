# 本地构建与发布操作要点

> 本文记录**操作层的坑与命令**（可复用、可共享），流程与授权约定见仓库根 `RELEASING.md`，
> Release Notes 文案规范见 `docs/release-notes-spec.md`。
> 建立：2026-09-17（从会话长期笔记中归档，原为个人记忆，属团队可复用知识）。

## 本地构建

```bash
pnpm exec tauri signer generate -w <tmpkey> --password="x" --ci
export TAURI_SIGNING_PRIVATE_KEY="$(cat <tmpkey>)"   # 必须是密钥内容，路径形式不认
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="x"
pnpm tauri build                                     # 全量约 30 分钟
```

- 产物在 `target/release/bundle/nsis/`。
- **一次性密钥仅供本地验证，禁止发布**。
- 抬高版本号即可覆盖安装。
- **构建后用 `artifacts/version_snapshot/` 还原 `Cargo.toml` / `tauri.conf.json` / `Cargo.lock`**
  ——这三个文件常被并发会话占用，**不能用 `git checkout` 还原**。
- 启动 GUI 用后台方式运行；node detached spawn 会被回收。
- **从 main 构建测试包**：`git worktree add C:/wt-release --detach <main-sha>` → 改版本 → 构建 → 收尾。

## draft / pre-release

- **建 draft**：
  ```bash
  gh release create v<ver> --draft --target <main-sha> --notes-file <md> <assets>
  ```
  - GitHub **会自动去掉资产名的中文前缀**。
  - draft URL 形如 `untagged-<hash>`。
  - **本地 `git tag -l` 看不到 draft 的 tag**（正常，不是缺失）。
- **draft → 预览版**：
  ```bash
  gh release edit v<ver> --title "..." --notes-file <md> --prerelease --draft=false
  ```
  **不需重新构建**。若手上已有 draft，先查再建，避免重复。
- ⚠️ **`gh release edit --notes-file` 必须传 Windows 形式绝对路径**；
  Git Bash 的 `/tmp/xxx` 会报 "The system cannot find the file specified"。

## 发布后必须核对四件事

1. `isDraft,isPrerelease` → 分别为 `false` / `true`；
2. tag sha == `git ls-remote origin refs/heads/main`；
3. 资产 `digest` 与发布前**逐字一致**（**不能只凭 size 判断**）；
4. `gh release list` 确认标记为 Pre-release。

- **改已发布 Release 正文后仍要重新核对这四项**；改正文不等于改版本，`publishedAt` 应保持不变。

## Release Notes 聚合口径（易错）

- ⚠️ **聚合口径是"相对上一个已发布版本"，不是相邻版本号**。
  本仓库 **0.2.7–0.2.11 从未发布**，所以 **0.2.12 相对的是 0.2.6**。
- ⚠️ 判断某功能是否属于本区间，**必须 `git ls-tree <起止tag> <path>` 逐个确认文件存在性**，
  不能只看 `git log tagA..tagB` 的提交条数（0.2.6→0.2.12 有 61 个提交，远超直觉）。

## 纯文档 PR 不触发 Windows CI（不是失败）

`.github/workflows/windows-ci.yml` 的 `paths-ignore` 含
`docs/**` / `Testing/**` / `artifacts/**` / `**.md` / `.gitignore` / `LICENSE`
→ `gh pr checks` 报 "no checks reported" 是**正常行为**。

**判据**改用：`gh pr view --json mergeable,mergeStateStatus`（要 `MERGEABLE` / `CLEAN`）。

## 授权边界（重申）

- 用户选 draft **不等于发布授权**；"发布预览版"是**单独授权**，且**不等于**发布正式版。
- 交付路径按用户原话执行：要本地包就给本地路径，不自行扩展到上传、发布等额外渠道。
