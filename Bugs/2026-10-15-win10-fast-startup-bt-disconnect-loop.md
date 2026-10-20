# Win10 开启快速启动后蓝牙链路反复断开（hiberboot 旧固件状态）

- 发现日期：2026-10-15（用户现场反馈，随桌面文件 `蓝牙快速启动修复.zip` 提供自写修复脚本）
- 状态：调查中 —— 应用侧**只读取证**已落地；真机复现与恢复手段 `deferred`
- 影响范围：Windows 10 + 「启用快速启动」开启 + 蓝牙适配器为 Realtek 8852 系列（已知案例，
  用户脚本里举的就是 8852CE）；遥控器侧用户那台是小米蓝牙语音遥控器，RC001 / RC003 均可能命中
- 功能点：BLE 连接生命周期（`ble` 工作线程）、诊断日志
- 现象：已配对遥控器每隔几十秒掉线一次、几秒后自己连回来，如此循环；重启电脑能好一阵子，
  关掉快速启动则彻底消失

## 复现条件

1. Windows 10，控制面板 → 电源选项 → 「启用快速启动」保持开启；
2. 正常点「关机」再开机（走 hiberboot，不是「重启」）；
3. 已配对的遥控器在开机后自行连上，随后进入"几十秒断一次 → 几秒后自连"的循环。

## 正常预期

连接稳定；只有用户主动断开、系统睡眠或遥控器关机时才断开。

## 证据

- 用户脚本与说明：`artifacts/user-feedback-fastboot/`（`file4_BT-FastStartup-Fix.ps1`、
  `file1_使用说明.txt`）。脚本值钱的实测结论有两条：
  - 开机后 ~25 秒做无线电重置**无效**（协议栈还在初始化，动作被吞）；
  - 登录桌面几分钟后重置**有效**，且用 `pnputil /restart-device`（原子操作）成功。
- 外部同类案例：Microsoft Learn Q&A「Bluetooth not connecting or pairing after login until
  the module is disabled then enabled in device manager」——Realtek RTL8852BE、登录后蓝牙
  半死不活、设备管理器禁用再启用即恢复；官方答复同样把 Fast Startup 列为首要排查项。
- 本机取证（2026-10-20）：`cargo run -p sayall-windows --example boot_type_probe` 输出
  `boot_type=full fast_startup_enabled=true`，与 PowerShell 真值一致
  （事件 27 `引导类型为 0x0`、`HiberbootEnabled=1`）。

## 根因

已确认的事实链：

1. 快速启动关机 = 把内核与驱动状态写进 `hiberfil.sys`；开机恢复镜像，而不是重新初始化硬件。
2. 固件阶段 BIOS/UEFI 会**重新上电并枚举 USB 总线**，蓝牙控制器实际被硬件级重置过；驱动醒来
   时却以为硬件还是关机那一刻。软件快照 ≠ 实际硬件状态。
3. Realtek 8852 系列在这条路径上恢复不干净（控制器内部带着过期状态），链路每次建立后几十秒
   被踢掉，遥控器自动重连后又踩同一个坑——这就是"固定周期"的来源，不是干扰/电量。
4. 完整启动（重启、Shift+关机、关快速启动）会重新初始化驱动，所以能彻底消失。

仍未知的边界：具体是哪一项控制器状态被污染（连接参数 / 加密上下文 / 电源管理标志）未定位；
我们**尚未在本机复现**该循环，因此不能宣称"应用侧任何方案已验证有效"。

`pnputil /restart-device` 之所以有效，是它让驱动完全卸载重装、控制器彻底重新初始化，等价于
完整启动时的那一次初始化；重置时机必须晚于协议栈恢复完成（用户实测 25 秒太早）。

## 修复（本期只有取证，不做系统级动作）

1. `crates/sayall-windows/src/boot_type.rs`（新增）：启动时读 `Microsoft-Windows-Kernel-Boot`
   事件 27 的 `BootType` 与 `HKLM\...\Session Manager\Power\HiberbootEnabled`，
   落 `app_lifecycle event=boot_context boot_type=… fast_startup_enabled=…`。
   两项都只读、不需要管理员权限；读不到落 `unknown` 并在有错误码时附 `boot_type_query_error`。
2. `crates/sayall-windows/src/disconnect_loop.rs`（新增，纯逻辑）：15 分钟内 ≥3 次 ≤90 秒的
   会话判为循环，睡眠/退出清零，长会话复位并重新武装；命中时落一条
   `ble_disconnect_loop phase=detected short_sessions=… boot_type=… action=log_only`。
3. `ble.rs`：连接成功 / `ConnectionStatusChanged → Disconnected` / `CallbackError`（GATT
   回调链读取失败）/ 睡眠 / 退出五处喂给检测器；同一代会话只会被计一次（`on_disconnected`
   会取走连接起点，重复事件自然被忽略）。
   **不改变任何恢复行为**——2026-10-14「不做无线电恢复」的决策不变。
4. 「复制诊断信息」块（2026-10-20 同轮）：诊断报告 schema **v3 → v4**，新增
   `bootType` / `fastStartupEnabled`（`null` = 读不到，不写成 false），前端复制块加一行
   `Boot: type=… fast_startup=yes|no|unknown`。用户报障**连日志都不用拉**就能分流
   "是不是 hiberboot"。同轮把启动日志里的 `diagnostic_schema=` 从写死的 2 改为
   `diagnostics::DIAGNOSTIC_SCHEMA_VERSION`（此前日志与报告体各写一份，已经漂了：
   报告是 3、日志还是 2）。

实现期踩到的两个坑（都已修，回归钉在 `boot_type` 单测里）：

- `EvtNext` 的 timeout 传 0 时**不报错但返回 0 条**，表现为 `boot_type` 永远 `unknown`；
  必须给非 0（用 200ms，只是安全上界：结果已由 `EvtQuery` 算好，正常路径立即返回）。
- `EvtRender` 输出的 XML 属性是**单引号**（`<Data Name='BootType'>0</Data>`），只匹配双引号
  的解析器在真机上恒为 `unknown`。

复查时补的两处（2026-10-20 晚，避免"日志静默失真"）：

- **`unknown` 不缓存**：只缓存读到的值，读不到下次再查。否则应用恰好在事件日志服务就绪的
  边沿启动时，一次失败会让这个进程余生都写 `unknown`——而这条日志的全部价值就是能区分
  两种开机。成功时同时清掉上一次的错误码（避免"曾经失败"被读成"这次也失败"）。
- **`CallbackError` 也计入掉链**：GATT 回调链读取失败同样意味着这一代会话结束；漏掉它会让
  "只以回调失败形式表现的掉线"永不进统计。`on_disconnected` 会取走连接起点，紧随其后的
  `ConnectionStatusChanged` 不会重复计数。

## 验证

- `cargo test -p sayall-windows --lib`（含新增 10 例）`passed`
- `cargo test -p sayall-windows-app diagnostics::` `passed`（schema v4 + camelCase 键 + token 合法性）
- `cargo run -p sayall-windows --example boot_type_probe` 真机 `passed`，与 PowerShell 真值一致；
  查询耗时 `elapsed_ms=2..6`（启动路径同步调用，不拖启动）
- 前端 `npx vitest run` 396 passed | 11 skipped、`vue-tsc --noEmit` + `vite build` `passed`
- `cargo fmt --all -- --check` `passed`
- `cargo check -p sayall-windows-app --features runtime-simulation` `passed`
- `deferred`：在 Windows 10 + 快速启动 + Realtek 适配器的机器上真机复现循环、确认
  `ble_disconnect_loop` 命中、并取得完整启动（对照）样本；之后才决定是否上"用户确认后一次性
  提权重置无线电"或 Helper 自动恢复（届时须修订 2026-10-14 决策，见
  `Testing/WindowsBleResourceRecovery.md`）。

## 隐私检查

未包含个人路径、设备身份、语音内容或凭据。
