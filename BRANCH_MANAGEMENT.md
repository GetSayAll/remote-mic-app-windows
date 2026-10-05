# 分支与提交管理策略（Windows 版）

参照 macOS 原版 `BRANCH_MANAGEMENT.md` 裁剪为 Windows 仓库当前适用的最小集；
发布管线不变量（相当于 macOS `release-main` 部分）待 Windows 发布流程建立时
再按原版补入。

## main 不变量

- 开始任何工作前 `git fetch origin main`；功能分支必须从**最新的
  `origin/main`** 创建，不基于过时的本地 main。
- main 工作区只用于同步已合入的远端主线：**不直接开发、不保存临时改动、
  不直接 push**（包括文档改动）——所有变更一律经 PR 合入。
- **该规则当前由分支保护强制（2026-10-03 调整）**：main 开启 "Require a pull
  request before merging"（0 批准）——直接 push 被拒；`enforce_admins` 保持开启
  （管理员也不能 force push / 删除分支）。2026-09-21–10-03 期间这项约束由
  `gate` 必需检查承担，该必需检查已随“纯文档 PR 恢复 paths-ignore”一并移除，
  详见下方两条。
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
- **纯文档 PR 不跑任何 CI（2026-10-03 用户决定）**：`push` 与 `pull_request`
  统一按 `paths-ignore` 跳过 **.md / docs / Testing / artifacts / Screenshots /
  .gitignore / LICENSE**——纯文档 PR 不产生 workflow run、不占 runner。为此移除
  2026-09-21 引入的 `changes`、`gate` 两个 job 和 `gate` 必需检查（当时保留
  `gate` 是因为被 paths 过滤跳过的 workflow 不会创建 check，必需检查缺失会让
  纯文档 PR 永久卡在 "Expected"）。
- **PR 与安装器生命周期校验分层（2026-09-27）**：代码 PR 的 `verify` 只运行
  前端、Rust、Tauri runtime simulation 等快速正确性检查；NSIS 构建以及安装、
  升级、降级、卸载矩阵移到 main push 或人工 `workflow_dispatch` 的 `installer`
  job。`verify`、`installer` 分别设置 30、45 分钟硬超时，同一 PR 的旧提交会被新
  提交取消。该分层与必需检查无关，2026-10-03 移除 `gate` 后继续有效。
- **CI 出结果前不得合并（2026-09-21 教训）**：#98 在自己的 CI 判定失败前
  5 分 35 秒被合入 main，导致 main 连红三个提交、后续所有代码 PR 都会在同一
  步骤失败。当时由 `gate` 必需检查在机制上堵住；该检查已于 2026-10-03 移除，
  **这条纪律重新由人和代理自己执行：代码 PR 的 `verify` 绿之前不得合并。**
- 每个独立工作项一个 commit，只包含该工作项的内容；交付时报告完整 SHA、
  Push 状态与验证命令（引用 macOS 原版"worktree、提交和清理"不变量）。
- 工作必须中途暂停或移交时：先在功能分支上 commit，并在提交信息中注明
  未完成状态与剩余事项。

## 与现有规范的关系

- 提交前必须满足 `AGENTS.md` 的自验证规范（真机证明生效、逻辑完备、
  最小化修改）——commit 是验证完成的落点，不是绕过验证的通道。
- `AGENTS.md` 运维与自愈节继续适用（部署不强杀应用、破坏性操作先验证
  目标等）。
