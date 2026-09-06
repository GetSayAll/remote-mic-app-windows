# 调查：按键映射"左键双响应"根因——key_gate 武装管线在 RC003 上结构性死锁（调查归档，2026-09-06）

## 现象

用户映射"左键→退格"后，每次按左键：文本光标先左移一格（原生 VK_LEFT 泄漏进 OS），再收到一次退格（映射引擎注入）＝双响应。确定键（=Enter）同样泄漏，但原生与注入效果相同，肉眼不可见。

## 证据链（全部本机真机实测）

1. **用户复现日志（2026-09-06 01:02，SAYALL_GATT_LOG）**：6 次左键按压全部
   `map_edges Left=true → map_fire action=shortcut chord=Backspace → map_inject ok →
   map_edges Left=false` 成对，映射引擎零缺陷；按压间隔 2.5s/26s/1.1s/1.6s/1.3s
   （全部 > 250ms 武装宽限）。双响应的另一半（原生左移）发生在 LL 钩子层，日志不可见。
2. **RC003 Windows 输入形态（2026-09-05 调查，本档案复用）**：RC003 在 Windows 上
   仅暴露一个 TYPE=1 键盘设备（HID 0x1812 接口），**没有任何 TYPE=2（HID）设备**；
   全部工作按键以逻辑键盘 VK 到达（VK_LEFT/RETURN/HOME/APPS/SLEEP/OEM_3…）。
   即 key_gate 设计所依赖的"独立 HID 报文管线（RIM_TYPEHID，不受键盘 LL 钩子影响）"
   在 RC003 上不存在；唯一武装来源是 RIM_TYPEKEYBOARD 路径。
3. **hw_swallow_probe 探针（2026-09-06 新增，examples/hw_swallow_probe.rs）**：
   LL 钩子无条件吞掉遥控器 VK_LEFT（4 次按压，DOWN/UP 共 8 个硬件事件，
   scan=0x4B、dwExtraInfo=0、非注入），**Raw Input（RIDEV_INPUTSINK）观察到 0 个**。
   证明 2026-09-05 结论"吞掉的事件不再投递 Raw Input"对**硬件事件**同样成立
   （此前仅用注入事件验证过）。推论：钩子 60ms 有界等待期间，本事件的 WM_INPUT
   不可能到达监听器（RIT 在钩子链返回后才投递），武装信号被钩子自身堵死。
4. **独立预信号管线两条候选路线均死**：
   - CreateFileW+ReadFile 直读键盘集合：err=5（ERROR_ACCESS_DENIED，Windows 对
     键盘/鼠标 HID 集合独占打开，安全策略，管理员权限亦无效）；
   - WinRT GATT 订阅 HID 服务（0x1812）Report 特征值：GetCharacteristicsAsync
     返回空（OS HID 栈占用，Testing/probe-rc003-hid-gatt.ps1）。
5. **dwExtraInfo=0**：LL 钩子事件无设备指纹可用。

## 结论

- **根因**：key_gate 的"武装归因"模型假设存在先于键盘孪生事件到达的独立 HID 报文
  管线（macOS 版有：IOHIDManager 直订全部 collection）。RC003/Windows 上该管线
  结构性不存在，武装唯一来源（Raw Input 键盘事件）又被钩子等待堵死 →
  **孤立按压（超过武装宽限间隔）首沿必泄漏**，随后 Raw Input 才看到事件并武装 →
  引擎经泄漏路径继续点火 → 双响应。250ms 宽限使"每次孤立按压"都命中此路径。
- ADR 0002（双轨注入架构，已接受）预判了该约束："公开 API 无法按设备全局屏蔽
  RC001/RC003 原始按键"，完整修复属可选 Helper 轨（提权/驱动，后续路线）。

## 修复（默认轨，用户选定策略：延长武装窗口）

- `key_gate.rs` ARM_GRACE_MS 250ms → 4000ms（吞键自我续期窗口）；
  `raw_input_windows.rs` GATE_ARM_GRACE_MS 250ms → 4000ms（泄漏后/观察到遥控器
  活动后的武装宽限）。
- 效果：遥控器任意按键后 4s 内的后续按压全部正确单响应（首沿泄漏一次并武装，
  吞键自我续期维持会话）；间隔 >4s 的孤立按压仍会双响应一次（结构性残留，
  待 Helper 轨彻底解决）。
- 代价（用户确认接受）：遥控器按键后 4s 内物理键盘同 VK 按压会被误吞并触发
  映射动作。
- 验证手段：map_edges 日志新增 `gate(sw=N lk=M)` 门控计数（sw=已吞边沿、
  lk=泄漏按下沿），真机复测时按 lk 增量核对泄漏次数。

## 遗留风险（待后续处理）

- **VK_SLEEP（电源键）**：孤立按压泄漏原生 VK_SLEEP 时，真实 PC 上会触发系统
  睡眠（本测试 VM 已禁用睡眠故未观察到）。电源键被映射时需特别策略
  （如 VK_SLEEP 直接归因吞下——物理键盘极少有睡眠键），列入 Helper 轨前的
  待决事项。
- >4s 间隔的孤立首按双响应残留（结构性，Helper 轨解决）。

## 运行方式

```powershell
cargo run -p sayall-windows --example hw_swallow_probe [seconds]
powershell -ExecutionPolicy Bypass -File Testing\probe-rc003-hid-gatt.ps1 <out> <seconds>
```

## 真机验证（2026-09-06，修复后便携实例实测）

7 次左键按压（间隔 2.4/4.5/1.5/2.9/2.3/10s），gate(sw/lk) 计数：
- 泄漏 3 次（冷启动首按、4.5s 与 10s 间隔的孤立按压）＝全部 >4s 间隔，符合设计边界；
- 吞下 4 次（<4s 间隔全部单响应，sw 逐一递增，自我续期维持会话）。
修复前同等按压 7/7 全部双响应；修复后泄漏仅剩结构性首按（>4s 间隔）。

## 后续发现（未修，待独立验证轮）

- **UP 配对污染跨按住残留**：track_down 的"泄漏污染"条目在 UP 放行后未清除
  （key_gate.rs UP 路径仅在 swallow=true 时 remove），任一次泄漏后，该键后续
  所有"吞下按压"的原生 UP 都会漏进 OS（stray keyup）。对方向/Enter/VK_SLEEP
  类按键无实际影响（应用忽略无配对 keyup；睡眠由 DOWN 触发），但偏离
  "本次按住污染"的设计意图。修复方向：UP 沿无论吞放都清除条目（一行改动 +
  纯函数单测），需随下一轮真机验证一并确认。
- **VK_SLEEP（电源键）**：孤立按压泄漏原生 VK_SLEEP 时真实 PC 会触发系统
  睡眠（本 VM 已禁睡眠未复现）；电源键被映射时建议后续采用直接归因策略。

## 修复记录（2026-09-06 晚，菜单键报障驱动）

用户报障：菜单键配置映射（Ctrl+V）后，每次按压同时执行原生上下文菜单指令
与映射动作。remote-capture 日志（Testing/investigation/remote-capture.log
19:35:39 会话）实证：孤立按压的 VK_APPS（0x5D）DOWN/UP 全部泄漏进 OS
（RAW 事件 corr=LL 配对可见），映射引擎另行注入 Ctrl+V——与左键双响应同根因
（武装死锁结构性残留，>4s 间隔必泄漏）。

- **VK_APPS 纳入直接归因族**（key_gate.rs `direct_attributed`）：菜单键与
  VK 0xFF 厂商键同款无需武装直接吞，孤立首按不再泄漏。理由：物理键盘仅
  全尺寸键盘右 Ctrl 旁的上下文菜单键会产生 VK_APPS，实际极罕见；映射已
  配置即表达替换意图（调查档案"待决事项"中 VK_SLEEP 建议的同款策略）。
  代价：菜单键已映射且门控就绪（遥控器连接中）期间，物理键盘上下文菜单键
  同样触发映射动作；取消映射即恢复透传。
- **UP 配对残留修复**（`take_up_pairing`）：UP 沿无论吞放都消费配对条目，
  上述"后续发现"第一项落地，泄漏污染不再跨按住残留。
- 单元测试：`menu_vk_apps_is_directly_attributed_without_arming`、
  `up_edge_consumes_pairing_entry_even_when_leaked`（key_gate.rs，纯函数
  decide/direct_attributed/take_up_pairing）。
- VK_SLEEP（电源键）直接归因仍保持待决（用户当前电源键未映射，无报障驱动；
  若映射电源键需先行决策）。
- 方向/Enter/Home/TV 等常见物理键 VK 的孤立首按泄漏仍是结构性残留
  （Helper 轨解决），其中原生动作与映射动作肉眼可见叠加的键：左键（原生
  左移+映射退格）、TV 键（原生输入 ` + 映射快捷键）、Home/Ok（原生动作
  常与映射相同故不可见）。