# RC001 / RC003 返回与音量± 路径：证据矩阵与旧结论更正

- 整理日期：2026-11-25
- 目的：解决仓库内关于“RC001 三键走 VK 0xFF / key_gate、不经过报告层”的旧结论与真机证据冲突
- 影响范围：全按键支持、RC001/RC003 三键映射、共享 WUDFHost 键盘串扰
- 本轮不改代码；只整理证据并登记待补的真机取证

## 结论（先看这里）

1. **产品行为上 RC001 与 RC003 一致**：开启「全按键支持」后返回 / 音量± 可映射；
   关闭后都不可用。
2. **实现路径也一致**：两型号的 BLE HID 硬件串同为
   `vid&012717_pid&32b8_rev&00a4`，增强捕获助手按该串匹配，**RC001 同样会被注入并接管**。
   两型号的区分只在应用层的 GATT 2A24（`ble_model`），HID 宿主进程看不到型号。
3. 因此“共享 WUDFHost 里物理键盘同名键被当成遥控器”的风险**同时适用于 RC001 和 RC003**，
   不是 RC003 独有；修复也不应只针对 RC003。
4. `rc003-helper` / `is_rc003` 的命名来自“只为 RC003 设计”的早期假设，**名称已误导**；
   实际匹配范围是 RC001 + RC003。

## 证据矩阵

| # | 结论 | 证据 | 日期 | 证据类型 | 状态 |
| --- | --- | --- | --- | --- | --- |
| 1 | 关闭全按键支持时，RC001 三键不映射；开启后可映射；与 RC003 表现一致 | `Testing/WindowsRC001ThreeKeyMapping.md` E1–E5 | 2026-09-25 | RC001 真机 | passed |
| 2 | RC003 的返回/音量± 在 HID 宿主报告里是 `0x00F1` / `0x0080` / `0x0081`，`kbdhid` 丢弃后常规通道收不到 | `docs/investigations/2026-09-23-rc003-hid-host-readonly-tap-result.md`、`...write-tap-result.md`、`...observe-first-real-run.md` | 2026-09-23 | RC003 真机报告转储 + `[EDGE]` | passed |
| 3 | RC001 也被增强捕获助手匹配：1.9.3 真机日志出现 `ownership_resumed` 与 `synth=0x003e->0x00e6`，文档明确写“助手按共享 VID/PID 匹配到了这台 RC001” | `Bugs/2026-11-15-rc001-voice-once-then-hid-reports-stop.md:70-71` | 2026-11-15 | RC001 真机日志 | passed（助手匹配）；back/volume 逐键 `[EDGE]` 未单独采集 |
| 4 | RC001 三键以 VK 0xFF 到达、`key_gate` 直接归因、不需要助手 | `crates/sayall-windows/src/rc003_bridge.rs:19-22`（2026-09-27 注释）、`docs/investigations/2026-09-06-left-double-response-arm-deadlock.md:150`、`docs/investigations/2026-09-23-rc003-driverless-capture-investigation.md:289` | 2026-09-06 / 09-23 / 09-27 | 代码注释 + 推断 | **已被 #3 反证；过期** |
| 5 | RC001 的按键不经过 HID 宿主报告层 | `Testing/WindowsRC001ThreeKeyMapping.md` E3 | 2026-09-25 | 文档断言 | **与 #3 冲突；判为错误/过期**；同文档 E2/E3 又写“两型号都依赖增强捕获” |
| 6 | 关闭全按键支持时两型号三键都不可用 | #1 + 当前 UI/文案：三键卡片置灰，提示需要开启全按键支持 | 2026-09-25 + 当前 | RC001 真机 + 产品文案 | passed（产品层） |

## 冲突是怎么产生的

1. 早期（2026-09-06）只有 RC003 的报告层证据；RC001 用 VK 0xFF 是**推断**，写进了代码注释和调查文档。
2. 2026-09-25 的 RC001 真机测试已经推翻了“关闭增强捕获也能映射”的产品结论，但文档 E3 仍保留了
   “RC001 不经过 HID 宿主报告层”的路径断言，导致同一份文档内部矛盾。
3. 2026-11-15 的 RC001 真机日志直接显示增强捕获助手匹配了 RC001（共享 VID/PID），
   并建立了所有权、执行了语音键合成。这证明报告层对 RC001 是实际路径。
4. 两型号共用 BLE HID 硬件串，`is_rc003` 无法区分；型号只在 GATT 2A24 层可区分。

## 仍未闭合的证据

- 缺少一次“RC001 + 增强捕获开启”的**逐键 `[EDGE]` 取证**：按返回 / 音量+ / 音量−，
  确认助手日志出现 `usages=0x00F1/0x0080/0x0081`，且主程序收到对应边沿。
- 现有 2026-11-15 日志只覆盖语音键合成，未覆盖三键；因此“RC001 三键确实经报告层”
  目前是“助手已匹配 RC001 + 9/25 产品行为一致”的强推断，不是逐键直证。
- 建议最小实验：RC001 真机 + 1.9.7 本地包，开启全按键支持，按三键各 3 次，
  检查助手 `[EDGE]` 与主程序 `map_edges`；同时用 `Testing/capture-rawkeys.ps1`
  记录 Raw Input，确认是否还存在 VK 0xFF 形态。

## 旧文档更正清单

以下位置的旧结论应加“已被 2026-11-15 RC001 真机反证”标注，避免继续被引用：

- `crates/sayall-windows/src/rc003_bridge.rs:19-22`：RC001 三键“被吞的键盘边沿 / GateEdge”表述。
- `docs/investigations/2026-09-06-left-double-response-arm-deadlock.md:150`：RC001 以 VK 0xFF 直接归因。
- `docs/investigations/2026-09-23-rc003-driverless-capture-investigation.md:289`：RC001 三键路径本就完整。
- `Testing/WindowsRC001ThreeKeyMapping.md` E3：RC001 不经过 HID 宿主报告层。

## 对当前 Bug 的影响

`Bugs/2026-11-18-fullkey-shared-host-keyboard-same-usage.md` 中“报告层无法区分共享宿主内
键盘”的结论对 **RC001 和 RC003 都成立**。条件策略（共享宿主 + 键盘类设备 → 排除高频键）
应按两型号统一实现，不能只按 `model=Rc003` 判断。

## 隐私检查

只引用日志事件名、usage、进程号与时间戳；未包含蓝牙地址、设备名、HID 路径或用户路径。
