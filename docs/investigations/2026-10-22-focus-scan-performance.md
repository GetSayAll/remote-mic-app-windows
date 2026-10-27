# 「聚焦输入框」UIA 扫描性能实测与提速（2026-10-22）

- 触发：用户报告「打开 WorkBuddy 后按键聚焦输入框 / 打开并聚焦已记录的输入框，可以聚焦但有延迟」。
- 范围：`focus_windows` 的候选扫描与聚焦写入。`focus.rs` 的打分/门槛语义不变。
- 工具：`crates/sayall-windows/examples/focus_probe.rs`（只读探针，不打印任何名称/内容）；
  诊断日志由 `SAYALL_GATT_LOG=<path>` 打开。

## 现象与量化

改动前，对 WorkBuddy 主窗口（pid 实测）连跑 `focus_probe --pid <pid>` 三次：整进程
**1.60 / 1.83 / 2.17 s**，`candidate_count=1`（唯一候选就是对话输入框）。
识别是对的，时间全部花在扫描上。代码注释里记录的 2.5–3.3 s 与现场一致。

## 根因（三处叠加）

1. **整树抓取 + 逐元素跨进程问属性**：`collect_candidates` 用
   `FindAll(TreeScope_Descendants, TrueCondition)` 把整棵树取回来（Electron 约 464 个元素），
   再对每个元素调 11 个属性（矩形 / AutomationId / 类名 / 名称 / 可用 / 可聚焦 / 密码 /
   ValuePattern.IsReadOnly / TextPattern / 焦点）。每次属性都是一次 IPC。
2. **同样的开销付两遍**：`focus_target` 选中索引后又 `FindAll` 一次、重新逐元素取属性
   走到命中项才 `SetFocus`。
3. **打开路径还要等前台**：`button_mapping::open_and_focus` 先
   `wait_for_target_foreground(2500 ms)` 再提交聚焦任务。

## 改动

| 改动 | 性质 |
| --- | --- |
| `IUIAutomationCacheRequest` + `FindAllBuildCache` 批量取属性 | 无损；provider 忽略缓存（关键属性成批为空）时整批回退非缓存 |
| 复用一个扫描阶段拿到的 `IUIAutomationElement` 引用直接 `SetFocus` | 无损；删掉第二次全树扫描 |
| 扫描条件改为 `IsTextPatternAvailable` ∪ ControlType ∈ {Edit, Document, Group} | 需保持候选集不变，见下 |

**条件必须无损**：`passes_hard_gate` 接受三个分支——`is_text_control()`（Edit/Document）、
`has_text_pattern`（**任意类型**）、`has_editor_semantics`（class/name 命中编辑器词）。
第一版只按 ControlType 过滤，实测立即把 Chrome 的候选从 2 个减到 1 个（那个
`ControlType=Other` 的输入框被漏掉）；改为加上 `IsTextPatternAvailable` 后逐目标与改动前完全一致。
纯语义命中的元素由「0 候选时回退全树」兜底。

## 实测（同一 pid，改动前 vs 改动后）

| 指标 | 改动前 | 改动后 |
| --- | --- | --- |
| 整进程耗时（`focus_probe --pid`，3 次） | 1957 / 2434 / 2588 ms | 864 / 907 / 911 ms |
| 单次扫描内部耗时（`focus_scan elapsed_ms`） | 1395 ms（464 元素） | 104–266 ms（5–422 元素） |
| 生产组合一次尝试（`--frontmost` 的 `elapsed_ms`） | 2500–3300 ms | 217–605 ms |

候选集 A/B（改动前二进制与改动后二进制，同一时刻同一目标）：

| 目标 | 改动前 | 改动后 |
| --- | --- | --- |
| WorkBuddy | 2（Other + Edit） | 2（Other + Edit，hash 相同） |
| Chrome | 1（Other） | 1（同 hash） |
| DimAgent（TipTap `Group`） | 1 | 1 |
| 微信 4.x | 0 | 0 |

记事本在两次测量中都是 0 候选（窗口未激活时其 provider 不建树），A/B 一致，与本次改动无关。

## 边界 / deferred

- **真机按键链路未验**（`deferred`）：本次只用探针与生产组合 `FocusRunner` 验证；
  遥控器按键触发的端到端时延需要在安装版上实测。
- 若某 provider 的 `IsTextPatternAvailable` 报 false 但 `GetCurrentPattern(TextPattern)` 成功，
  该元素只能在 0 候选回退时被找到；实测未出现（Chrome / DimAgent / WorkBuddy 均正常）。
- `target_window` 取 Z 序第一个可见窗口；WorkBuddy 出现子 WebView 时可能取到小窗口
  （实测树规模 5 / 88 / 464 三种），属既有行为，本次未改。
