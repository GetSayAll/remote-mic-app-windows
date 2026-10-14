# RC003 助手 agent 自愈刷新加固：独占闸门 + 同一代次组合只刷新一次

- 发现日期：2026-10-14（复核用户报障"蓝牙设备约每小时中断一次"时确认两条可放大风险）
- 状态：**已实现（自动化 passed）**；真机复测 `deferred`
- 影响范围：RC003（2 Pro）「全按键支持」的助手自动换代路径；0.8.6 及更早版本
  均含原实现；RC001 与未开「全按键支持」的用户不受影响
- 功能点：`hardware/RC003/helper/src/main.rs` 的 stale agent 自动刷新
  （`spawn_agent_refresh_if_needed` → `refresh_agent_in_host`）
- 现象/风险（复核结论）：
  1. 机制动作是**结束承载遥控器的 WUDFHost 进程**再让系统重建——每次执行都会让
     遥控器（及同一宿主承载的其它 BLE 设备）可见地中断一次；
  2. 原判定是"冷却 3600 秒后可再试"：**如果刷新一次不能收敛**（宿主里仍是旧代），
     就会退化为"约每小时杀一次宿主"——与报障"约 1 小时一次中断"同形（列为候选
     之一；用户侧日志 `deferred`，本机未命该状态）；
  3. 原实现**没有独占安全闸**（与干净首注路径 `restart_rc003_host` 的
     `members==1 && rc003_members==1` 闸门不一致）：共享宿主（同载用户键鼠等）
     会被一并中断。
- 证据：
  - 本机日志 1 次真实执行（本地 2026-10-09 01:02）：`[AGENT-STALE]`
    `running=2026-10-03.*` / `embedded=2026-10-05.*` → `[AGENT-REFRESH]`
    `requested`→`completed terminal_result=passed`（1.03 秒、单次收敛，此后未复现）；
  - 代码判据：`should_auto_refresh_agent`（原签名为"冷却内不刷、冷却外再刷"）、
    `refresh_agent_in_host`（直接 `TerminateProcess`，无成员检查）。
- 修复（2026-10-14，最小改动）：
  1. **独占闸门**：新增纯函数 `should_refresh_stale_agent_host(members, rc003_members)`
     （`1 && 1` 才刷新）；共享宿主跳过并落
     `[AGENT-REFRESH] terminal_result=skipped reason=shared_host`，且**不写尝试记录**
     （宿主恢复独占后仍允许首次刷新）；
  2. **每组合一次**：`agent-refresh.stamp` 由"单个时间戳"升级为
     `last_refresh=<unix秒>` + `tried=<旧代>-><内嵌代>` 多行；同一组合只自动刷新
     一次，之后落折叠日志 `reason=already_attempted_once`（前 3 条 + 每 30 条一条）；
     旧格式文件兼容读取（只恢复冷却时刻，仍允许该组合一次刷新）；
  3. 同步更新 `[AGENT-STALE]` note 与
     `Testing/WindowsRC003EnhancedCapture.md` 的日志字典。
- 验证：
  - 先红后绿：新增单测 `same_build_pair_is_never_refreshed_twice`、
    `shared_host_is_never_auto_refreshed`、`refresh_stamp_roundtrips_and_accepts_legacy_timestamp`
    先因缺符号/签名不符编译失败（预期原因），实现后通过；
  - `cargo test --manifest-path hardware/RC003/helper/Cargo.toml` **53 passed / 0 failed**；
  - `cargo run --manifest-path hardware/RC003/helper/Cargo.toml -- --selftest`
    退出码 0（结论：全部通过）；
  - `cargo fmt --manifest-path hardware/RC003/helper/Cargo.toml -- --check` passed；
  - 真机复测 `deferred`：制造"宿主里旧代 agent"现场（或在下一版升级后自然出现）
    验证 ①同一组合至多刷新一次、②共享宿主跳过、③报障用户侧"约 1 小时一次中断"
    是否消失（需下一版安装包）。
- 隐私检查：本文不含个人路径、设备身份、蓝牙地址、语音内容或凭据。
