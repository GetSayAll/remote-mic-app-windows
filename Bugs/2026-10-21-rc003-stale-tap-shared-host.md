# 2026-10-21 RC003 助手卡在共享宿主旧 tap：每 38 秒白注入一次，报告层始终不可用

- 发现日期：2026-10-21
- 状态：调查中，未修复；需要真机实验决定「只重启 RC003 设备节点」是否安全
- 影响范围：RC003 + 「全按键支持」；Windows 10/11；助手升级/重启且宿主
  `WUDFHost.exe` 未重启时
- 功能点：RC003 增强捕获助手启动决策、旧世代 tap 自愈、报告层合成
- 现象：界面「全按键支持」仍显示已开启，但报告层没有生效；日志每约 38 秒出现
  一次 `capture_inactive reason=agent_never_acked`，助手进程不断更换 pid。
  语音键回落到 `SendInput`，豆包按设计忽略注入键，表现为语音不可用。
- 正常预期：旧 tap 接不上时自动换代，不需要用户重配遥控器、禁用设备或重启系统。
- 复现条件：宿主里残留上一代注入的 tap，且宿主同时承载其它 HID 设备
  （本机 `members=3 rc003_members=1`，共享宿主）；助手重启后无法接管旧 tap。

## 证据（`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`，UTC）

旧世代最后一次可鉴权：

- `11:54:46.979Z` / `11:54:46.984Z` `[HELLO] auth=true pid=37356`
  `agent=rc003_agent/1 build=2026-10-05.gate-delay-units`
- 之后旧 agent 不再重连；`11:55:58Z` 起每轮：

```
[ATTACH] verify=failed reason=no_authenticated_agent
         action=inject_new_generation members=3 rc003_members=1
         gadget_maps=2 skip_host_restart=shared_host
[VERIFY-MODULE] module_present=true gadget_modules_in_host=2
[REAP] removed=0 kept_in_use=1
[SUMMARY] uptime_s=38 agent_lines=0 authenticated_ever=false
         how=injected_new_generation
```

- 应用侧对应行：
  `capture_inactive ... reason=agent_never_acked retryable=true
  hint=auto_restart_helper bridge_phase=connected helper_pid=<每轮不同>`
- 到 `12:37Z` 已累计 50+ 轮；`gadget_modules_in_host` 始终为 2，说明
  `inject_new_generation` 没有产生可鉴权的新实例，只是在同一宿主里重复
  `LoadLibrary` 已有模块。
- 同期 `voice_synth_resend ... attempt=1|2|3` 持续出现，应用始终拿不到
  `voice_synth_ack`；`voice_attempt` 回落到 `path=send_input
  synth_confirmed=false`。
- 应用升级/重启后仍复现（`12:36:40Z` 新实例 `pid=10888` 起）：
  `WUDFHost.exe` 未重启，旧 tap 仍在；说明重启主程序不能清除该状态。

## 根因

已确认：

1. 旧世代 tap 仍驻留在共享 `WUDFHost.exe`（pid 37356）里；它的 agent 在
   `11:54:46Z` 之后不再重连，新助手也没有令牌能接管它。
2. 既有自愈只允许结束**独占 RC003** 的宿主
   （`should_restart_host_for_clean_inject`：`members==1 && rc003_members==1
   && gadget_maps>0`）。本机 `members=3`，判据不满足，走
   `skip_host_restart=shared_host` 分支；这是正确保护——同宿主还承载鼠标等
   其它 HID 设备，不能直接杀。
3. 「同一宿主内再注一代」是实验性路径。真机上 `gadget_modules_in_host`
   保持 2，`[REAP] kept_in_use=1`，新实例无法完成 agent 握手；助手 30 秒后
   自行退出，应用对账再拉起，形成 38 秒循环。

仍未知：

- 旧 tap 是否仍在改写 F5（配置曾为 `synth=0x003e->0x00e6`）；它是否与本轮
  ATVV STOP 丢失有因果关系，现有日志不能证明。语音卡死本身已由应用侧
  看门狗兜底（见
  [2026-10-21-voice-session-stuck-right-alt-held.md](2026-10-21-voice-session-stuck-right-alt-held.md)）。

## 2026-10-21 21:51 受控验证：只移除 RC003 设备节点可同时清掉旧 tap 和半死 GATT

现场应用卡在 GATT 查询，旧 `WUDFHost.exe pid=37356` 仍带旧 tap。执行：

```
pnputil /remove-device "BTHLE\DEV_C05D39C2D9DB\8&2B46EB95&0&C05D39C2D9DB"
pnputil /scan-devices
```

结果（passed）：

- `remove-device` 约 65 s 后成功；旧宿主 37356 消失，新宿主 37164 拉起；
- 新助手 `[INJECT] pid=37164` → `[VERIFY-MODULE] gadget_modules_in_host=1`
  → `[HELLO] auth=true` → `[SYNTH-ACK] state=0x003e->0x00e6`
  → `[HB] lease_ok=true`；
- 应用侧 `voice_synth_ack confirm=agent`、`voice_synth_gate active=true`、
  `enhanced_capture ownership_resumed usages=80,81,f1`；
- 蓝牙鼠标节点保持 `OK`，未被一起移除；
- 应用连接在宿主重建后 1.3 s 内完成（`service_discovery 76 ms`）。

结论：**只重启 RC003 设备节点、不结束共享宿主**这条方向可行，而且能同时
解决「旧 tap 卡住助手」与「旧 GATT 会话卡住连接」。同日用户确认：重建后
`path=report_layer` 会话正常出字（session 128 / session 1），报告层合成
`0x003e->0x00e6` 生效。下一步是把它做成助手的自动恢复分支，并加真机回归。

## 拟定修复方向（未实施）

优先做**只重启 RC003 设备节点**，不结束共享宿主：

1. 助手识别「共享宿主 + 确有我们旧 tap」后，不杀宿主，改为
   `recover_device_nodes(rc003_instance)`：`pnputil /remove-device` +
   `/scan-devices`，只让 RC003 重新枚举。
2. 等待新的 RC003 宿主/实例出现，做一次干净注入；旧宿主里的旧 tap
   随设备节点释放。
3. 若设备节点恢复后仍拿不到新宿主，再回退到明确的人工指引，并**停止
   每 38 秒重复注入**（避免日志/进程 churn 和潜在 HID 链路扰动）。

验收判据：

- `[HELLO] auth=true` + `voice_synth_ack active=true confirm=agent` +
  `ownership_resumed`；
- 另一台 BLE 鼠标/键盘在操作期间保持可用；
- 不要求用户重新配对或重启系统；
- 失败时只落一条可分流日志，不进入无限重启循环。

## 边界

- 不允许为了自愈结束共享 `WUDFHost.exe`（会同时断开鼠标等设备）。
- 设备节点重启可能短暂影响 RC003 的 HID/ATVV 链路，必须真机验证。
- 本项与「语音会话 STOP 丢失」是两个独立问题；应用侧看门狗只保证不粘键、
  不永久卡 UI，不恢复报告层。
- 隐私检查：未包含蓝牙地址、HID 路径、用户名、语音内容或凭据。
