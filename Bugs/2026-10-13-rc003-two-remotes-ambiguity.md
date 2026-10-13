# 两支同型号遥控器同时在线：两条链路都拒绝盲选，整机按键全断且无解释

- 发现日期：2026-10-13
- 状态：已修复（自动化 passed；真机「两支在线」复验 `deferred`）
- 影响范围：全部版本；Windows 10/11；RC003（VID 0x2717 / PID 0x32B8）；RC001 路径不涉及
- 功能点：Raw Input 绑定（普通按键）+ 增强捕获助手（「全按键支持」）——两条链路各自的候选选择
- 用户报障原话：「你改了什么东西？现在全按键映射不行了」「按键的监听启动也失败」

## 现场与时间线（诊断日志，UTC，本机 +08）

1. `07:28:27Z` 起 `raw_input_listener action=start … terminal_result=failed
   error_code=listener_start_failed`，每 10 秒一次，当天 49 次（历史日志中此前从未出现）。
   紧随其后的是 `raw_input binding_audit action=timer_disarm_failed error=句柄无效
   (0x80070006)`——**这是收尾时对没装上的审计定时器 KillTimer 的连带报错**，不是原因。
2. `07:32:32Z` 起助手逐个退出：`[REG] diag_keys=10 instances_with_hostpid=10 no_host=0
   read_failures=0 rc003_instances=2` → `[STOP] note=定位到多个候选宿主 {2108, 29068}`，
   当天 13 次（同样只在今天出现）。
3. 设备清单（`Get-PnpDevice -Class HIDClass`）里确实有**两台 RC003**（不同蓝牙地址，
   尾号 `…D9DB` 与 `…B76B`）同时在线，各带一个活的 `WUDFHost`（2108、29068）；
   另有两台 RC001 的节点（不影响：两条链路的匹配规则都只认 `pid&32b8`）。
4. 时间线上的关键一环：`07:28` 起失败的是**另一个分支的构建**（`source_revision=c7af1e1`，
   `fix/trikeys-observable-without-mapping`），`07:31:56` 才轮到本次的包（`0fc6c0b`）——
   两边失败形态逐字一致，说明与具体构建无关。**旧实例不会暴露它**：启动时早就绑好了
   唯一目标，之后不再重新枚举；一旦重启（或助手重新解析宿主），歧义必然暴露。

## 根因

- Raw Input：`select_single_device_path` 在多候选时返回 `DevicePathError::Ambiguous`
  （`raw_input_windows.rs` 注释原文：「多设备歧义仍按失败处理，由上层重启」）。
- 助手：`resolve_target` 在多个 RC003 宿主之间**拒绝盲选**（`Err(12)`，提示用
  `--target-pid` 指定）——这是刻意的安全设计：选错遥控器比不用更糟。
- 两条链路都**没有**利用一个本来就知道的事实：**应用自己正连着哪一支**（BLE 会话持有的
  `BluetoothLEDevice`）。于是「用户有两支遥控器」被实现成了「整机不可用」，
  而且界面上一句话都没有。

## 修复（`fix/rc003-pick-connected-remote`）

把「应用正在用的那支遥控器」做成**单一来源**（`crates/sayall-windows/src/remote_target.rs`）：

- BLE 会话在真正就绪（`PipelineOutput::Ready`）时发布**对端蓝牙地址**，
  进程启动时先清零（上一轮残留不会带歪过滤条件）；
- 进程内：Raw Input 枚举后按地址收窄候选（`filter_paths_for_target`）。地址已知但一个
  候选都不匹配 ⇒ 按「设备不在位」处理（转等待、等接口恢复），**绝不退回未过滤集合**
  ——那会在两支同型号遥控器之间绑错设备（用户按 A 支、程序听 B 支）；
- 进程外：地址写进 `%LOCALAPPDATA%\SayAll\rc003-target.ini`（`version=1` +
  12 位十六进制）。助手在**解析宿主之前**读它，多候选时只留属于这支的实例 id
  （`instance_matches_address`，大小写不敏感）；选不出唯一一支时保持旧行为（拒绝盲选）。
- 日志：新增 `remote_target action=published|cleared source=ble_session`（只记"有没有值"），
  助手侧新增 `[REG] note=按主程序目标地址消歧 candidates=N target_matched=N`（只记数量）。

## 验证

- 自动化 passed：`crates/sayall-windows` 新增 `remote_target` 两组单测（地址匹配大小写不敏感、
  另一支同型号不命中；文件正文往返与坏值拒绝）；`cargo fmt --all -- --check`、
  `cargo test --workspace`（sayall-windows 334 passed、app 108 passed）、
  `cargo check --workspace`、`cargo check -p sayall-windows-app --features runtime-simulation`；
  助手 `cargo test` 50 passed、`--selftest` 退出码 0（新增自检项「目标地址匹配」）。
- **真机「两支同型号遥控器同时在线 → 自动选中应用那一支」：`deferred`**——
  需要两支 RC003 同时在线的现场（本次自然发生，本机未构造受控复现）。

## 边界与隐私

- 只解决「**同型号**候选之间的消歧」：地址未知（BLE 未就绪、或上一轮未发布）时行为与过去
  完全一致（唯一候选才绑定、多候选仍拒绝）。
- 地址只用于匹配：**不进日志**、不进用户可见文案、不进诊断摘要、不进仓库；
  目标文件落在用户自己的 `%LOCALAPPDATA%\SayAll\`（与桥接描述文件同目录、同可见范围）。
- 本次现场的另一半（同一个根因）已同轮修好：Raw Input 与助手现在指向**同一支**遥控器。
