# 旧世代 tap 残留导致「全按键支持」不自愈：只接管不换代，30 秒后报 agent_never_acked

- 发现日期：2026-10-08（操作人现场：语音条拉不起来、按键不映射）
- 状态：已修复；受控复现 **passed**（PR [#234](https://github.com/GetSayAll/remote-mic-app-windows/pull/234)，merge `a6a1474`）
- 影响范围：0.8.1–0.8.3；Windows 10/11；RC003 增强捕获（「全按键支持」）；应用升级安装、宿主被结束/重建后
- 功能点：助手启动时的宿主识别与注入决策（`hardware/RC003/helper/src/main.rs`）、增强捕获布防（主程序侧）
- 现象：界面「全按键支持」显示已开启，但按键完全不映射；同时语音键无反应（微信/豆包语音条均拉不起来）；应用侧日志 `rc003 feature=enhanced-capture action=capture_inactive terminal_result=failed reason=agent_never_acked retryable=true elapsed_ms=30001 hint=reconnect_remote`
- 复现条件：宿主进程（WUDFHost）里残留**上一代注入**的 tap——旧世代 agent 的令牌与助手手上的不一致；助手启动走 `DllPlan::Attach`（只接管、不注入），旧 agent 反复重连但被 `[REJECT] reason=token_mismatch` 拒绝
- 正常预期：不需要用户或人工干预；助手应自行换代，恢复到"助手 ↔ 新 agent 已鉴权握手"的状态
- 证据（2026-10-08 本机）：
  - 故障现场（revision `8b4c2c98`）：`[ATTACH] action=skip_injection …已有我们那一代 tap` → 8 秒后 `[ATTACH] verify=failed reason=no_authenticated_agent action=inject_new_generation` → 之后应用侧一直 `agent_never_acked`；`[REJECT] reason=token_mismatch` 连续 3 次（`agent_pid` 指向旧宿主）。
  - 人工恢复验证有效的机制：结束该独占宿主 → 系统重新枚举出新宿主 → 助手对新宿主干净首注（`[TAP] resident=0` → `[INJECT] … hmodule_return=…` → `[HELLO] auth=true`）；宿主消亡后节点进 Error 时用 `pnputil /remove-device` + `/scan-devices` 恢复。
- 根因：`attach_verification_expired` 命中后的换代动作是**在同一个宿主进程内再注入第二份 Gadget**——该路径在使用说明里即标注「实验性：依赖 Frida 允许同一进程内两个 Gadget 实例，**未验证**」。真机上它不足以摆脱旧世代 tap，导致自愈失败并把错误状态一直挂在界面上。
- 修复（PR #234，commit `233e7b0`）：
  1. 新增纯函数 `should_restart_host_for_clean_inject(escalated, members, rc003_members, gadget_maps)`：判据 `!escalated && members==1 && rc003_members==1 && gadget_maps>0`（与 `[EXCLUSIVE] verdict=exclusive_rc003_host` 同口径）。
  2. 新增 `restart_rc003_host(old_pid, instance_id, logger)`：`terminate_process` → `recover_device_nodes`（实例 ID 只传递、不进日志）→ 有界轮询（≤12 s / 400 ms）找同一 RC003 的新宿主 pid。
  3. 分流：满足判据时结束旧宿主 → 对新宿主 `prepare_runtime(Canonical)` + `inject_gadget` 做**一次干净首注**并重开握手窗口；**共享宿主 / 无我方注入时绝不结束宿主**，保留原换代尝试并写 `skip_host_restart=shared_host|no_resident_tap|module_scan_failed`。新路径失败不回退到"同宿主再注一代"。
- 验证：
  - 自动化：助手单测 **45 passed / 0 failed**（新增 4 例：`restarts_only_for_exclusive_rc003_host_holding_our_gadget`、`never_restarts_a_shared_host`、`never_restarts_without_our_gadget_in_the_host`、`never_restarts_twice_in_one_run`）；`--selftest` 退出码 0、共享日志 0 FAIL（含新增自检项）。
  - **受控复现（真机 passed）**：轮换运行时目录 `session.token` 制造旧令牌僵尸 → 助手启动 → `[REJECT] token_mismatch` ×3 → `[ATTACH] action=restart_host_clean_inject` → `[HOST] terminate_stale_tap_host pid=9304` → `[HOST] new_host_ready pid=31496` → `[INJECT] pid=31496` → `[HELLO] auth=true pid=31496`，**从僵尸识别到握手成功约 3 秒、零人工操作**；设备零 Error 节点。
  - 本地测试包（`233e7b0`）：升级安装后助手哈希与包内一致、应用内嵌 rev 一致、`[HELLO] auth=true`、`ble_connect completed`、增强捕获已布防。
- 未覆盖边界：本地包为 x64 单载荷（本机缺 aarch64 工具链，stage 已提示）；`restart_rc003_host` 按 `is_rc003 && pid != old_pid` 找新宿主，多台同名实例并存时不区分；发布包需在 CI 侧重出。
- 隐私检查：全文不含设备实例 ID、接口路径、令牌值或语音内容；实例 ID 与宿主 pid 的使用符合 `LOGGING.md`。
