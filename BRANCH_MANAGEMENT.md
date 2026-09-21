# 分支与提交管理策略（Windows 版）

参照 macOS 原版 `BRANCH_MANAGEMENT.md` 裁剪为 Windows 仓库当前适用的最小集；
发布管线不变量（相当于 macOS `release-main` 部分）待 Windows 发布流程建立时
再按原版补入。

## main 不变量

- 开始任何工作前 `git fetch origin main`；功能分支必须从**最新的
  `origin/main`** 创建，不基于过时的本地 main。
- main 工作区只用于同步已合入的远端主线：**不直接开发、不保存临时改动、
  不直接 push**（包括文档改动）——所有变更一律经 PR 合入。
- **该规则已由分支保护变成强制（2026-09-21）**：main 要求必需检查 `gate`
  通过，且 `enforce_admins` 开启——管理员同样不能直接 push、也不能在 CI
  未通过时合并。实测直推被拒：`Required status check "gate" is expected.`。
  紧急绕过方式：临时 `gh api --method DELETE
  repos/<org>/<repo>/branches/main/protection`，处理完立即按原配置恢复。
- PR 的目标分支只能是远端 main；合入后再次 fetch，确认本地 main 与
  `origin/main` 精确一致。
- 不使用 force-push、广泛 reset，或把未验收内容以整支旧分支覆盖主线；
  冲突逐文件核对解决。

## 工作项提交纪律

- **一项功能"实现完成 + 自验证完成"后必须立即 commit**：push 或 PR 可以
  延后，但工作不允许停留在未提交状态——未提交的工作既容易丢失，也会与
  后续工作项混在一起无法回溯。
- **提交前必须 `cargo fmt --check` 通过（2026-09-05 教训）**：格式漂移曾致
  CI verify 连续 10 次失败（run #64-#73），阻塞 PR #19 合入近一天才发现。
  发现漂移先 `cargo fmt` 并作为独立 style 提交；CI 全量流水线约 19 分钟，
  格式问题拖到 CI 才暴露反馈太慢。
- **push 前跑 `scripts/ci-preflight.ps1`（2026-09-05 新增）**：本地镜像 CI
  verify 的 7 个快速步骤（前端依赖/测试/构建 + fmt/Rust 测试/check +
  `cargo check -p sayall-windows-app --features runtime-simulation`），
  约 1-2 分钟；通过即等价于 CI 的这些步骤必过。发布前加 `-Full` 再追加
  runtime-simulation 的完整构建。第 7 步是 2026-09-21 补入的：此前默认只跑
  前 6 步，而 `cargo check --workspace` 不带 feature、覆盖不到 feature-gated
  的 simulation 模块，#98 的 E0063 就是因此在本机全绿的情况下进的 main。
- **PR 与 push 的路径过滤不再相同（2026-09-21）**：`push` 到 main 仍按
  `paths-ignore` 跳过纯文档改动；`pull_request` 不再按路径跳过整个 workflow，
  改由 `changes` job 判定、`verify` 按需运行、`gate` 始终给出结论。原因是
  被 `paths-ignore` 跳过的 workflow **不会创建任何 check**，而分支保护要求
  `gate`，纯文档 PR 会永久卡在等待状态；现在这类 PR 由 `gate` 秒过。
- **CI 出结果前不得合并（2026-09-21 教训）**：#98 在自己的 CI 判定失败前
  5 分 35 秒被合入 main，导致 main 连红三个提交、后续所有代码 PR 都会在同一
  步骤失败。`gate` 必需检查已在机制上堵住这条路径——不依赖自律。
- 每个独立工作项一个 commit，只包含该工作项的内容；交付时报告完整 SHA、
  Push 状态与验证命令（引用 macOS 原版"worktree、提交和清理"不变量）。
- 工作必须中途暂停或移交时：先在功能分支上 commit，并在提交信息中注明
  未完成状态与剩余事项。

## 与现有规范的关系

- 提交前必须满足 `AGENTS.md` 的自验证规范（真机证明生效、逻辑完备、
  最小化修改）——commit 是验证完成的落点，不是绕过验证的通道。
- `AGENTS.md` 运维与自愈节继续适用（部署不强杀应用、破坏性操作先验证
  目标等）。
