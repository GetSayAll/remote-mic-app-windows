# 组合动作：编辑一次后列表变成两条（同 id 多版本被当动作铺开；附 formatVersion=0）

- 状态：已修复（自动化 passed；真机复验 deferred——需重装含本修复的包后目视）
- 影响范围：组合动作页（列表、候选、按键页「组合动作」分组）；以及导出→导入往返
- 报告：2026-10-15 用户现场（组合动作编辑后"变成了两个"）

## 现象

在组合动作页编辑一条已保存的动作（改名/改步骤）并保存后，列表里出现**两条**，名字分别是编辑前的与编辑后的。

## 现场证据（`%APPDATA%\app.getsayall.remote-mic.windows\combined-actions.json`）

```json
{
  "formatVersion": 0,
  "definitions": [
    { "macroId": "macro.9fdd…", "version": "1.0.0", "name": "打开网页", … },
    { "macroId": "macro.9fdd…", "version": "1.0.1", "name": "打开网页和记事本", … }
  ]
}
```

## 根因（两处，各自独立）

1. **显示层把"版本"当"动作"铺开**：库文件按不可变版本累积（编辑 = 追加
   `1.0.1`，绑定引用按最新版解析——这是对齐 Mac 的有意设计），但页面 /
   按键页芯片直接渲染 `definitions` 全量，于是同一条动作的每个版本都占一行。
   数据没坏、绑定也没坏，坏的是列表口径。
2. **空库结构版本落盘为 0**：`MacroLibraryDocument` 用 `derive(Default)`
   得到 `format_version: 0`，首次保存即以 0 落盘；而导入要求
   `format_version == 1` ⇒ **导出后再导入必被拒**（"不支持的组合动作文件版本：0"）。
   同一文件里两处症状同源可复现。

## 修复

- `crates/sayall-core/src/macros.rs`：
  - 手写 `Default`（当前结构版本）+ `normalized()` 盖章；
  - 新增 `latest_definitions()`：同 `macro_id` 只留最新语义版本，顺序按首次出现。
- `src-tauri/src/settings.rs`：加载/保存/导出/导入统一 `normalized()`；
  导入的版本检查放宽为"仅拒绝更高版本"（0 是历史落盘形态，归一化放行）。
- 前端：`latestMacroDefinitions()`（bridge.ts，同款语义版本的纯函数），
  组合动作页经 `applyLibrary()` 统一收口、按键页芯片列表同源去重。

## 验证

- **阳性对照**：临时把组合动作页的去重改回原样，两条新回归用例立即失败
  （`expected a length of 1 but got 2`，正是现场症状）；恢复后通过。
- Rust：`cargo test --workspace`（core 54 含 3 新例、app 117 含 1 新例）、
  `cargo fmt --all -- --check`。
- 前端：`vue-tsc --noEmit`、vitest 390 passed | 11 skipped（新增
  bridge 3 例 + 组合动作页 2 例 + 按键页 1 例）。

## 边界与遗留

- 历史文件里的 0 版本在加载时归一化，无需手工改文件；旧版本号条目保留在
  库中（不可变版本语义），界面只显示最新，删除会清掉该动作的全部版本。
- 真机复验 deferred：重装含本修复的包后，编辑一条动作确认列表仍只有一条，
  并顺手验一次"导出 → 导入"往返。
