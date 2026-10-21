# 2026-10-21 RC003 共享宿主旧 tap：助手自动做设备节点恢复

- 状态：已实现；自动化 passed；**真机自动路径 deferred**（手动 `pnputil` 恢复已 passed）
- 影响范围：RC003 + 「全按键支持」；共享 `WUDFHost.exe` 里残留上一代 tap
- 功能点：助手旧世代 tap 自愈、桥接恢复状态、主程序捕获失效重启的暂缓
- 关联记录：`Bugs/2026-10-21-rc003-stale-tap-shared-host.md`（由 PR #283 引入；
  记录现象、日志与手动恢复证据）

## 现象与根因

宿主 `WUDFHost.exe` 同时承载 RC003 与蓝牙鼠标（`members=3 rc003_members=1`）。
助手升级/重启后，旧 tap 握着我们接不上的旧令牌；既有自愈只在**独占 RC003**
（`members==1 && rc003_members==1`）时结束宿主，共享宿主走
`inject_new_generation`。真机上该路径无效：`gadget_modules_in_host` 始终为 2、
`[REAP] kept_in_use=1`，每约 38 秒重复一次，报告层始终不可用。

手动恢复（2026-10-21 已验证 passed）：

```
pnputil /remove-device "BTHLE\DEV_<address>\<instance>"
pnputil /scan-devices
```

只移除 RC003 设备节点，不结束共享宿主；系统重建宿主后新助手
`[INJECT]` → `[HELLO] auth=true` → `[SYNTH-ACK]` → `ownership_resumed`，
蓝牙鼠标节点保持 `OK`。旧宿主里的旧 tap 随之消失。

## 修复

### 助手（`hardware/RC003/helper/src/main.rs`）

1. 共享宿主分支在 `gadget_maps > 0` 且拿得到 RC003 完整实例 ID 时，执行
   `recover_shared_host_device_node`：只 `recover_device_nodes`（`pnputil
   remove-device` + `scan-devices`），**不结束宿主**。
2. `wait_for_new_rc003_host` 有界等待同一个 RC003 的新宿主 PID（`≤12 s`）；
   拿到后对**新宿主**做 `Canonical` 干净首注并重开握手窗口。
3. 若系统没有重建宿主、或干净首注失败，回退到既有的
   `inject_new_generation`（不改原有保底行为）。
4. 每次助手进程最多尝试一次；`device-recovery.stamp` 记录上次尝试时刻，
   **跨进程 10 分钟冷却**，避免跟随主程序 38 秒重启节奏反复 `remove/scan`。
5. 恢复期间通过桥接发送 `R 1`，结束发送 `R 0`；断线重连后补发绝对状态。

### 主程序（`crates/sayall-windows/src/rc003_bridge.rs`、`src-tauri/src/lib.rs`）

1. 解析桥接 `R <1|0>`，快照新增 `recovering` / `recovery_age_ms` /
   `recovery_ended_age_ms`；新助手握手或断线时清空。
2. 捕获失效的自动重启在助手恢复期间暂缓：
   - 恢复进行中最多等 `180 s`（`remove-device` 实测可超过 60 s）；
   - 恢复结束后再留 `20 s` 尾巴给注入/握手；
   - 超过上限仍不恢复则照旧重启助手（保留应用侧兜底，防助手挂死）；
   - 只写一条 `capture_recover phase=waiting reason=helper_device_recovery`，
     不按 2 秒节奏刷屏。

### 日志判据

- 助手：`[ATTACH] action=recover_device_node_clean_inject` →
  `[HOST] action=recover_device_node` → `[CLEANUP] device_recover=ok` →
  `[HOST] action=new_host_ready` → `[DLL]` → `[HELLO] auth=true`；
- 失败：`[WARN] 共享宿主设备节点恢复未产生新宿主`，随后
  `[ATTACH] action=inject_new_generation device_recovery=already_attempted`；
- 主程序：`rc003_bridge event=helper_recovering active=true|false`、
  `capture_recover phase=waiting reason=helper_device_recovery`。

## 验证

- `cargo fmt --all -- --check` passed；
- `cargo test -p sayall-windows --lib`：421 passed / 13 ignored；
- `cargo test -p sayall-windows-app --lib`：147 passed / 1 ignored；
- 助手 `cargo test`：75 passed；
- 新增单测覆盖：恢复闸门（无 gadget / 无实例 / 同轮已尝试 / 冷却）、冷却边界与
  时钟回拨、桥接 `R` 行解析、主程序恢复窗口与尾巴边界。

**真机 deferred**：代码路径尚未在真实共享宿主旧 tap 上跑过。已有真机证据仅为
手动 `pnputil` 恢复 passed（见关联记录）。真机验收需安装含本修复的本地包后，
制造共享宿主旧 tap，确认自动链路完整、鼠标保持可用、且恢复失败时只做一次
设备操作并按冷却跳过。

## 边界

- 不结束共享 `WUDFHost.exe`；`remove/scan` 只针对 RC003 实例。
- 设备节点重建会短暂中断 RC003 的 HID/ATVV 链路；RC001 不涉及此助手。
- 恢复失败后主程序仍会按既有节奏重启助手，但助手在冷却期内跳过设备操作，
  只走同宿主再注一代，不反复扰动 HID 链路。
- 隐私：本文与代码日志均不含蓝牙地址、HID 实例路径、用户名或语音内容。
