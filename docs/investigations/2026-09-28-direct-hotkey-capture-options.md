# 两处快捷键直接录入方案调研

- 日期：2026-09-28
- 状态：方案调研完成，未改生产代码
- 范围：按键页“自定义快捷键”、连接页“按住说话快捷键”
- 用户目标：去掉必须理解和切换“安全录入模式”的负担，像微信输入法设置页一样直接修改快捷键

## 结论

建议把两处入口统一为一个**持续编辑、显式保存的快捷键录入器**：用户仍直接按实体键，
录入器把本次会话见到的键持续加入草稿，松手只更新显示、不自动保存；用户可以同时按完整
组合，也可以逐个轻按组成组合的键，最后点“保存”。界面不再暴露“安全模式”开关和修饰键
鼠标选择器。

这不是把现有连接页方案直接搬到按键页。连接页当前实现已经被用户判定不可用，并有日志证明
它曾把 `左 Ctrl + 左 Win` 截成单键保存；其后增加的“接受输入法重放副本 + 200ms 稳定窗口”
仍未完成真机通过。新的共同录入器应重写“何时定稿”的状态机，不以“所有键已松开”为自动
提交条件，也不根据第三方麦克风状态猜测用户按了什么。

## 为什么微信输入法能直接录，而 SayAll 不能原样照搬

微信输入法录入的是**它自己拥有的热键**。进入编辑态时，它可以让自己的热键识别分支暂停，
同一个底层钩子便能把组合交给设置 UI，而不是触发语音。SayAll 无权通过公开 API 暂停另一个
进程的全局钩子；如果微信输入法先看到并消费完成键，SayAll 的窗口事件、低级钩子和 Raw Input
都无法保证拿到原始完整组合。

这一区别是基于当前行为和 Windows 输入模型的推论，不是对微信输入法私有实现的逆向结论。

## 已确认的现状

1. 两个页面已经共用 `start_shortcut_capture` / `stop_shortcut_capture` 和
   `shortcut-capture-edge`，底层是 `WH_KEYBOARD_LL`，不是普通 Web `keydown`。
2. 按键页默认已有“同时按完整组合”的直接录入；安全模式只是兜底。说明问题不在于缺少
   录入入口，而在于直接录入不能覆盖所有组合。
3. 本机历史日志已经出现：用户按输入法现用和弦时，只收到 `LeftControl` 或
   `LeftWindows` 单键，随后连接页按 `key_count=1` 保存。现有 Bug 记录还证明外部输入法会
   “吞下物理完成键，再重放注入副本”。
4. `Win+L` 是更硬的边界：仓库真机记录显示，即使 SayAll 钩子判定已吞下 Win/L 边沿，
   当前主机仍可能锁屏。Microsoft PowerToys 也明确把 `Win+L`、`Ctrl+Alt+Del` 列为不能靠
   低级钩子重映射的系统保留组合。
5. `RegisterHotKey` 只能登记一个**已经知道的**组合，并且 Windows 键组合可能被系统保留；
   它不适合用来发现用户正在输入的未知组合。
6. `BlockInput` 会让真实键盘输入不再更新同步/异步键状态，因此既不适合作为录入源，也会
   造成高风险的全局输入冻结体验。

## 推荐产品方案：持续编辑型录入器

### 交互

1. 点击当前快捷键胶囊或“修改”。
2. 弹出小型录入层，焦点锁定在录入器；提示“直接按组合键，也可以逐个按键”。
3. 每个新键立即显示为可删除的键帽，例如 `左 Ctrl`、`左 Win`。
4. 松开全部键后**不自动结束、不自动保存**；用户仍可补按遗漏键。
5. 底部只有“取消 / 清空 / 保存”。页面失焦、超时、路由切换统一取消，绝不保存半截草稿。
6. 两处复用同一组件和同一捕获会话；仅由调用方提供校验策略：
   - 按键页：校验该组合是否能由当前动作执行器可靠注入；`Win+L` 可保存，但执行时继续走
     既有 `LockWorkStation` 特例；`Ctrl+Alt+Del` 等不可执行组合直接说明原因并拒绝。
   - 连接页：允许纯修饰键组合（例如 `左 Ctrl + 左 Win`、单独 `右 Alt`），并提示它必须与
     目标语音软件设置一致；不再通过 ConsentStore 猜测具体组合。

### 为什么它能替代当前安全模式

- 对普通组合，用户仍可以一次同时按下，体验与常见快捷键编辑器一致。
- 对已经被输入法占用的组合，当前日志证明单独按 `左 Ctrl`、再单独按 `左 Win` 时两个键都能
  被 SayAll 看见；持续编辑器会把它们合并为同一草稿，而不会像现实现那样在第一个键松开时
  立即错误落盘。
- 对 `Win+L` 这类系统组合，用户可依次轻按 `Win`、`L`，物理输入流从未形成 `Win+L`，
  因而不会锁屏；不需要先开启安全模式、再用鼠标点修饰键。
- 它不依赖钩子安装顺序、不依赖第三方重放注入副本的具体时序，也不需要管理员权限或驱动。

### 状态机边界

- `idle -> starting -> recording -> saving|cancelling -> idle`，每次带 `session_id`；迟到边沿只能
  命中原会话，不能污染下一次录入。
- 启动时有 preheld 键则只提示先松开；preheld 清零前不修改草稿。
- 草稿按首次按下顺序去重；自动重复不重复加键；左右修饰键保持区分。
- 捕获到的 DOWN 必须与 UP 成对吞放。结束会话后仍等待已经吞下的 DOWN 对应 UP，避免粘键。
- 自己注入的事件用固定 `dwExtraInfo` 标记并排除；来自第三方的注入副本只能作为诊断，不能
  单独触发自动保存。
- 超时只取消，不“尽量保存”；这是现实现把半截组合落盘的关键纠正。

## 可选增强实验：修饰键按下后立即中和

如果仍希望“按住整组后自动识别”的成功率更接近微信输入法，可做独立 spike：SayAll 录到首个
修饰键 DOWN 并吞下后，立即用带自有标记的 `SendInput` 注入该键 UP，让更早的第三方钩子清掉
内部按住态，再继续收集后续物理键。`SendInput` 保证同一批输入串行插入，微信输入法也已实证会
响应注入输入，所以该方向有实验价值。

但它不能作为首版承诺：第三方钩子可能忽略注入 UP，系统保留组合仍可能走更低层路径，且需要
证明快速同按时中和 UP 一定先于第二个物理 DOWN。只有下列真机矩阵全部通过后，才能把它作为
“同时按完整组合”的增强，不得替代持续编辑基线：

- 微信输入法当前热键：冷态/热态各 20 次，零误触发语音、零半截草稿；
- `Win+L`：20 次零锁屏；`Win+D`、`Alt+Tab`、`Ctrl+Shift+Esc` 不执行系统动作；
- 左右 Ctrl/Alt/Shift/Win、纯修饰键组合、主键组合；
- 焦点丢失、超时、快速按放、录入中退出、重复开启；
- 日志核对 DOWN/UP 成对、自有注入被排除、第三方注入可区分。

任一系统组合仍会执行，就保留“同时按”作为尽力能力，并让逐键输入成为确定性路径。

## 不推荐路线

- **直接复用连接页现实现**：真机未通过，且包含 200ms 猜时序与 ConsentStore 推断，不能泛化
  到按键页。
- **只用 WebView `keydown` / `preventDefault`**：到页面时已经晚于系统级处理。
- **只换成 Raw Input**：可稳定观测普通设备输入，但不能恢复已经被前序钩子消费的完整组合；
  `RIDEV_NOHOTKEYS` 也明确不阻止系统热键。
- **`RegisterHotKey` 穷举组合**：不是录入 API，存在注册冲突和系统保留组合。
- **反复重装钩子争抢顺序**：本仓库的实际链序实验与第三方重建钩子的竞态已经证明不可靠。
- **常驻提权助手或键盘过滤驱动**：能下沉到更低层，但成本、风险和产品边界远超快捷键设置
  所需；基础路径不得因此依赖管理员权限或驱动。
- **自动操作微信输入法设置**：只能覆盖单一第三方版本，且会形成脆弱的外部 UI 耦合。

## 实施拆分（后续，不在本次执行）

1. 先抽取无 UI 的 `ShortcutDraft` 状态机并 TDD：持续累计、显式保存、preheld、重复边沿、
   session/generation、取消与迟到回调。
2. 再做共享 `ShortcutRecorder` 组件，让两个页面只提供当前值和校验策略。
3. 复用现有 `key_gate` 的成对吞键与 IPC，但删除两个页面各自的自动定稿逻辑；连接页去掉
   ConsentStore 推断，按键页去掉安全模式开关和修饰键选择器。
4. 自动化通过后先做 Windows 实机键盘矩阵，再分别验证连接页的微信输入法热键与按键页的
   系统组合；RC001/RC003 只影响动作来源，不替代键盘录入本身的 Windows 验收。

## 参考

- Microsoft `LowLevelKeyboardProc`：钩子可返回非零阻止事件继续传到后续钩子/目标窗口，且
  回调发生在异步键状态更新之前：
  <https://learn.microsoft.com/windows/win32/winmsg/lowlevelkeyboardproc>
- Microsoft Raw Input：注册、`WM_INPUT` 与 `RIDEV_NOHOTKEYS`/`RIDEV_NOLEGACY` 边界：
  <https://learn.microsoft.com/windows/win32/inputdev/about-raw-input>
  <https://learn.microsoft.com/windows/win32/api/winuser/ns-winuser-rawinputdevice>
- Microsoft `RegisterHotKey`：Windows 键组合保留边界：
  <https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-registerhotkey>
- Microsoft `SendInput`：串行插入、UIPI 和当前键状态边界：
  <https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-sendinput>
- Microsoft `BlockInput`：物理输入不更新同步/异步键状态：
  <https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-blockinput>
- Microsoft PowerToys Keyboard Manager：其“Type shortcut”同样使用低级钩子，并明确拒绝
  `Win+L`、`Ctrl+Alt+Del` 等低级钩子无法重映射的组合：
  <https://github.com/microsoft/PowerToys/blob/main/doc/devdocs/modules/keyboardmanager/keyboardmanagercommon.md>
  <https://learn.microsoft.com/windows/powertoys/keyboard-manager>

