# 遥控器操作 Windows 应用切换器（Alt+Tab）

## 适用版本或分支

- 适用分支：包含 `app_switcher` 动作的分支（2026-11-05 起）。
- 产品行为对齐 macOS `Command+Tab` 临时会话；平台动作换成 Windows 原生
  `Alt+Tab`。
- 本文覆盖启动、左右选择、OK 确认、返回取消、重复触发、15 秒超时、断连、
  语音键取消、应用退出、鼠标切换与映射变化。RC001 与 RC003 必须分别执行。

## 行为摘要

1. 触发绑定为「切换应用」的按键后：保持 Alt 并发送一次 Tab，系统切换器出现。
2. 切换器显示期间：
   - 左 / 右：上一项 / 下一项（发送 Shift+Tab / Tab）；
   - 再次触发原按键或按 TV：下一项；
   - OK：释放 Alt，确认当前项；
   - 返回：先发送 Esc 取消，再释放 Alt；
   - 上 / 下：消费本次按键，不发送任何键（对齐 Mac）；
   - 其他按键：继续执行各自映射。
3. 最后一次操作后 15 秒无操作：取消并释放 Alt。
4. 遥控器断连、监听停止、映射变化、向导暂挂、语音键开始、应用退出：
   取消会话并释放 Alt。
5. 用户用鼠标点选窗口后切换器已关闭：静默释放 Alt，不再发送 Esc，也不改变
   用户已经点中的窗口。
6. 如果开始后 1.5 秒内没有观察到原生切换器（注入失败、第三方 Alt+Tab
   替换器），立即释放 Alt 并记录 `reason=start_not_observed`，不把 Alt
   长时间留在系统里。

Windows 10 的切换器窗口类是 `MultitaskingViewFrame`；Windows 11 是
`XamlExplorerHostIslandWindow`。类名只用于布尔判定，不写入日志。

会话期间门控会直接遮蔽与遥控器同名的物理键（OK / 返回 / 方向 / TV 等），
即使该键没有映射，以免原生 Enter / 方向 / 返回漏进切换器或目标应用；会话
结束或遥控器离线后立即恢复。因此验收时不要同时用物理键盘的 Enter / 方向键
判断“原映射是否失效”，应使用遥控器实体键。

## 测试前准备

1. 安装测试包并启动无线麦 SayAll Windows 版，确认遥控器已经连接。
2. 打开至少三个可切换的普通窗口（例如记事本、文件资源管理器、浏览器）。
3. 进入「按键」页，启用自定义按键功能，把 TV 键单击绑定为「切换应用」
   （或绑定到 OK/首页等任意普通按键）。
4. 把左方向键绑定为「退格」，用于验证会话期间原映射不会执行。
5. 记录应用版本、构建修订号、遥控器型号（RC001 / RC003）、Windows 版本
   和当前前台应用。
6. 诊断日志位置：`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`。

## 用例 1：启动会话

1. 让一个非无线麦的窗口处于前台。
2. 按一次绑定为「切换应用」的按键并松开。

预期结果：系统 Alt+Tab 切换器出现；当前高亮项保持；不会立即完成切换，
也不会向输入框写入字符。日志出现：

```text
map_app_switcher operation=<n> phase=start ... result=submitted
map_app_switcher operation=<n> phase=open switcher_seen=true
```

失败判定：切换器不出现、出现后立即消失、Alt 没有保持、输入框出现字符。

## 用例 2：左右选择

1. 在用例 1 的切换器仍显示时，按一次左方向键并松开。
2. 再按一次右方向键并松开。
3. 重复左右各 3 次。

预期结果：每次按键只移动一个高亮项；左键不执行原来绑定的「退格」；
日志出现 `phase=press ... action=Previous` 和 `app_switcher_inject
command=ReverseTab result=submitted`。

失败判定：一次跳过多个项、左键执行退格、切换器消失、输入框出现字符。

## 用例 3：OK 确认

1. 启动切换器并移动到目标应用。
2. 按一次 OK 并松开，等待目标成为前台。

预期结果：切换器关闭，目标应用成为前台；无线麦保持可用。日志：

```text
map_app_switcher operation=<n> phase=press ... action=Confirm
app_switcher_inject command=AltUp result=submitted
map_app_switcher operation=<n> phase=end reason=confirmed ... alt_release=submitted alt_down_after=false
```

失败判定：没有切换、切换到错误窗口、Alt 卡住（后续键入变成快捷键）。

## 用例 4：返回取消

1. 启动切换器并移动到另一个应用。
2. 按一次返回键并松开。

预期结果：切换器关闭，前台仍是启动前的应用；返回键不执行原映射。日志出现
`escape=true`、`reason=cancelled`、`alt_down_after=false`。

失败判定：切换到了新高亮应用、切换器仍在、Alt 卡住。

## 用例 5：重复触发与 TV 前进

1. 把「切换应用」绑定到 TV（或一个非 OK/返回/方向键）。
2. 启动切换器后，每按一次 TV 并松开，观察高亮项每次前进一项。
3. 把「切换应用」改绑到 Home 长按；按住 Home 触发后，再按一次 Home。

预期结果：原触发键在会话内只前进一项；不会重复按 Alt；不会出现一次按键
跳过两项。日志中的 `tab_count` 随操作递增。

失败判定：原触发键执行原动作、一次前进多项、Alt 被重复按下。

## 用例 6：15 秒无操作超时

1. 启动切换器后不再操作遥控器，等待至少 16 秒。
2. 观察切换器与前台窗口，再用键盘输入普通文字。

预期结果：最后一次操作约 15 秒后切换器关闭，前台回到启动前的应用；
Alt 已释放；日志出现 `reason=timeout escape=true`。每次导航都会重新开始计时。

失败判定：切换器长期不退出、Alt 卡住、超时明显早于 15 秒。

## 用例 7：断连、睡眠与监听停止

1. 启动切换器后关闭遥控器或断开蓝牙。
2. 重新连接遥控器，用实体键盘输入普通文字和快捷键。
3. 另一次测试中，启动切换器后让电脑睡眠再唤醒。

预期结果：会话取消并释放 Alt；重新连接后普通映射、键盘输入和系统快捷键
恢复正常。日志出现 `reason=device_removed` / `reason=listener_stopped`
（睡眠唤醒按实际路径记录）。

失败判定：Alt 卡住、后续文字变成快捷键、必须重启应用才恢复。

## 用例 8：语音键取消

1. 启动切换器后按一下遥控器语音键。
2. 说话并松开，确认文字进入启动前的目标输入框。

预期结果：切换器取消（前台回到启动前的应用），Alt 释放，语音输入正常；
日志出现 `reason=voice_started`，随后是正常的语音会话日志。

失败判定：语音条不出、语音热键被 Alt 干扰、切换器仍显示、Alt 卡住。

## 用例 9：应用退出

1. 启动切换器后，从托盘退出无线麦 SayAll。
2. 用实体键盘输入普通文字和快捷键。

预期结果：应用退出前同步释放 Alt；日志出现 `reason=shutdown` 与
`alt_down_after=false`（退出收尾的 ack 路径）；键盘不残留 Alt。

失败判定：退出后 Alt 仍按下、普通文字变成菜单快捷键。

## 用例 10：鼠标切换

1. 启动切换器后，用鼠标点击另一个窗口。
2. 等待 1～2 秒，再用键盘输入普通文字。

预期结果：切换器关闭，用户点中的窗口成为前台；应用静默释放 Alt，不发送
Esc；日志出现 `reason=switcher_closed escape=false alt_down_after=false`。

失败判定：Alt 长时间卡住、点击后又被 Esc 取消、光标窗口被切回原应用。

## 用例 11：映射变化与向导暂挂

1. 启动切换器后，在另一端修改任一按键映射并保存。
2. 另一次测试中，启动切换器后进入向导第②步（按键映射暂挂）。

预期结果：会话取消并释放 Alt；日志出现 `reason=mappings_changed` 或
`reason=suspended`；修改保存或暂挂流程本身正常完成。

失败判定：会话跨配置存活、Alt 卡住、配置保存失败。

## 用例 12：第三方 Alt+Tab 替换器（边界）

1. 安装并启用一个会替换系统 Alt+Tab 的第三方工具。
2. 触发「切换应用」。

预期结果：本功能只支持 Windows 原生切换器；1.5 秒内观察不到原生切换器时
释放 Alt 并记录 `reason=start_not_observed`。用户仍可通过「录入自定义
快捷键」绑定一次性 Alt+Tab，或关闭第三方替换器。

失败判定：Alt 长时间卡住、第三方切换器被反复注入。

## 已知边界

- 只支持 Windows 原生切换器；第三方 Alt+Tab 替换器启动后会按
  `start_not_observed` 释放 Alt（见用例 12）。
- Windows 11 的 `XamlExplorerHostIslandWindow` 也被开始菜单等 explorer
  XAML 浮层复用。会话期间打开其他浮层时，外部关闭判定可能推迟到 15 秒
  超时；Alt 仍会在超时释放，不会无限保持。
- 会话期间门控会接管同名物理键（Enter / 方向 / 返回等），最长 15 秒；
  结束后恢复。验收“原映射不执行”时使用遥控器实体键。

## Windows 版本矩阵

| 版本 | 切换器窗口类 | 结果 | 备注 |
| --- | --- | --- | --- |
| Windows 11 26200 | `XamlExplorerHostIslandWindow` | passed（本机探针） | 见下方证据 |
| Windows 10 1809+ | `MultitaskingViewFrame` | deferred | 需在 Windows 10 真机执行用例 1–6 |

## 本机证据（2026-11-05）

- PowerShell 注入探针（Windows 11 26200，真实桌面）：
  - Alt 按下 + Tab：前台变为
    `XamlExplorerHostIslandWindow`（explorer.exe），切换器出现；
  - 再按 Tab / Right：前台保持切换器；
  - 按 Esc 后释放 Alt：前台回到启动前的窗口；
  - 两次 Tab 后释放 Alt：前台切换到另一个窗口（确认生效）；
  - Alt+Tab 后保持 16 秒：切换器仍在，说明 15 秒应用侧超时是必要的；
  - 释放后 `GetAsyncKeyState(VK_LMENU)` 为 0。
- `cargo run -p sayall-windows --example app_switcher_probe`：
  `switcher_present=true`、`left_alt_down_after=false`，退出码 0。
- 自动化：`cargo test --workspace` 全绿（sayall-windows 518 passed /
  16 ignored，含 16 条会话状态机与 1 条引擎集成；sayall-windows-app 203
  passed / 1 ignored）；前端 595 passed / 10 skipped；`vue-tsc --noEmit`。

## 失败判定的共同项

- 任何路径结束后 `GetAsyncKeyState(VK_LMENU)` 仍为按下（Alt 卡住）。
- 会话期间控制键执行了原映射（例如左键退格、OK 回车）。
- 一次按键跳过多个切换项，或切换器没有出现却持续保持 Alt。
- 应用退出、断连、语音开始后仍保持 Alt。
- 日志缺少 `map_app_switcher phase=end`，或 `alt_down_after=true` 但没有
  后续恢复说明。

## 隐私与日志边界

- 日志只记录操作编号、原因 token、布尔值、耗时和计数。
- 不记录窗口标题、窗口句柄、类名、进程路径、设备地址或语音内容。
- 真机验收时如需截屏，请避开包含个人信息或聊天内容的窗口。
