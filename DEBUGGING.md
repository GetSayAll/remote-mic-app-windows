# 无线麦 SayAll Windows Bug 排查手册

这份文档是**怎么查**，不是**做什么**。流程（记录、复现、缩小范围、只修已确认根因）
见 [Bugs/README.md](Bugs/README.md)；本文只写"哪些做法会绕弯子、哪些判据真的管用"。

**任何 Bug 调查开始前先读本文。** 每一条都来自实测，附"当时踩到的样子"，避免读成空话。

首个案例：2026-09-16 的"遥控器连不上 + 蓝牙图标拉不起来 + `0x80070008`"（16 轮爆发、
4581 次失败、跨 4 天）。完整记录见
[Bugs/2026-09-16-ble-stack-resource-exhaustion-recovery-ineffective.md](Bugs/2026-09-16-ble-stack-resource-exhaustion-recovery-ineffective.md)。

---

## 1. 先分清"现象"和"原因"，同源症状不要串成因果链

同一时刻冒出的多个症状（蓝牙图标拉不起来、设置页扫描报 `0x80004004`、SayAll 报
`0x80070008`）**默认假设同源**，然后用时间戳对齐判"哪一侧先坏"，而不是在症状之间
建立"A 导致 B"的链条。

当时踩到的样子：把"图标拉不起来"当成 `0x80070008` 的原因去解释，方向整反了——两者
是同一僵死态的两个出口（一个是 Shell 侧，一个是应用侧）。第一轮结论必须作废重来。

## 2. 错误码的字面含义不是根因

`0x80070008` = `ERROR_NOT_ENOUGH_MEMORY`，名字叫"内存资源不足"。现场实测：物理内存
空闲 3.1 GB、提交量占上限 58%、本进程私有提交约 14 MB。**"资源不足"这个字面含义当场
被计数否证。**

规则：错误码是**这条路径的通用失败码**，只说明"现在做不了"，不说明"为什么做不了"。
必须用**资源计数实测**去否证字面含义，而不是拿它当结论。

当时踩到的样子：先下结论"内核非分页池被泄漏占满"（错），修正为"用户态进程泄漏了 BLE
资源"（又错），两次都是被新加的探针打掉的。见 §4。

## 3. 只看日志尾部会得出相反结论

只看 `tail` 时，每次现场都像"刚恢复、偶发"。全量聚合才看清是**慢性病**：16 轮爆发、
4581 次失败、单轮最长 1 小时 53 分（1571 次）。

规则：**先全量聚合（按小时 / 按 pid / 按爆发轮次切分），再决定读哪一段。**

```bash
# 一行就能避免半天误判
grep -ac "windows_resource_exhausted" "$LOG"          # 总量
grep -a "windows_resource_exhausted" "$LOG" | grep -ao "pid=[0-9]*" | sort | uniq -c | sort -rn
```

## 4. 缺的不是"更多日志"，是"缺哪几类事实"——然后让日志否证自己

本次真正缺的只有四类事实，补的都是**只读采样**，不动任何行为：

| 缺的事实 | 补法 | 结果 |
| --- | --- | --- |
| 进程/系统资源计数 | `resource_probe.rs` 只读采样（句柄 / GDI / USER / 私有提交 / 非分页池 / 提交量 / 物理可用 / 进程线程数） | **第一次启动就否掉了当时的主假设**：资源全不紧张，"泄漏"一族假设三连否 |
| 原始 HRESULT | 把被压成 `snapshot_failed` 的失败改成 `hresult=0x…` + `raw_error=` | 确认三条入口是同一个错误码，排除了"多码混合" |
| 睡眠/唤醒时刻 | `system_suspend` / `system_resume` 打点（此前**零日志**） | 与系统事件对照后确认本次 S3 是"伪睡眠" |
| 失败发生在哪一阶段 | `ble_connect_stage phase=failure_detail` | 定位到设备对象创建，而非发现/订阅 |

规则：**加日志的目的是否证自己，不是印证自己。** 如果新日志只会"确认我已经相信的事"，
那它没价值。写完探针要主动问："它长什么样才会推翻我？"

## 5. 恢复手段要做成实验矩阵，逐项记结果

"某手段执行成功" ≠ "故障被解除"。必须用**同一个判据**复查（本次的判据是：新进程能否
创建 BLE 设备对象 → 看 `radio_recovery_prepare ... passed cache=ready`）。

本次的矩阵（全部实测）：

| 手段 | 结果 |
| --- | --- |
| 应用重启 / 新进程 | 无效（新进程一启动即失败 → 反证不是本进程泄漏） |
| 无线电 Off/On | **无效**（489 次请求、143 次明确"执行成功"） |
| 杀每用户 WinRT broker（RuntimeBroker/SystemSettings/explorer） | 无效 |
| 杀每用户蓝牙服务宿主（`microsoft.bluetooth.userservice.dll`） | 无效 |
| 杀 COM 代理 `dllhost` | 无效 |
| 提权 PnP 重启适配器 | 无效，**且是空操作**（见 §6） |
| 伪 S3 睡眠 | 无效 |
| **完整重启** | **有效（3 次实证）** |

规则：把候选手段列成表，**逐项做、逐项留证据**，不要"再试一个看看"。否决也是资产——
它能排除整层范围（本次直接排除了"所有用户态手段"）。

## 6. 本机环境会骗人：三个必须验的前提

1. **管理员命令"跑过了" ≠ "生效了"。** `pnputil /restart-device` 返回成功退出码，但
   `Kernel-PnP/Configuration` 通道零设备事件（同一通道对其它设备重启都有 400/410/420
   记录）。→ **验收看副作用通道，不看退出码。**
2. **睡眠未必真的断电。** `Kernel-Power 42 → 107` 只隔 2.3 s、固件计时
   `SuspendStart/SuspendEnd` 只差 1 个 tick、`FullResume: 259 ms` → 伪 S3。真实 S3 恢复
   是秒级。→ **判"是否真断电"看固件计时与恢复耗时。**
3. **关机 ≠ 重启。** 本机启用了快速启动，混合关机不会完整复位设备，必须用"重启"。

## 7. 工具反模式清单（都在本仓库真实出现过）

- **测试/运维脚本用强杀收尾。** `Stop-Process -Force` 本身就是楔死诱因。正确收尾：先请求
  优雅退出（`Local\SayAll-GracefulExit`）→ 有界等待 → 仍不退才强杀并打印告警。
  2026-09-16 已修正两个安装脚本，并以
  `scripts/test-windows-installer-graceful-exit.ps1` 把"应用运行中执行安装/卸载"纳入 CI。
- **测试依赖"我是第一个初始化全局状态的人"。** `gatt_sink()` 是 `OnceLock`，首次调用即
  固定；新测试先落一条日志就把同进程的日志落盘测试钉死（2026-09-16 实际发生）。
  新增测试不要碰全局 sink，除非先显式设置 `SAYALL_GATT_LOG`。
- **只判退出码不看副作用**（见 §6.1）。
- **`.ps1` 含中文却不带 UTF-8 BOM。** PowerShell 5.1 会按 ANSI 解码，中文变乱码并产生
  看似语法错误的假报错（本次 12 个"语法错误"全是编码假象）。**含中文的脚本存 UTF-8 BOM**
  （`scripts/ci-preflight.ps1` 是既有先例）；`.nsi` 同理，直接调 `makensis` 时必须带 BOM
  或加 `/INPUTCHARSET UTF8`。
- **NSIS 里用 `${__LINE__}` 拼标签名。** 安装器与卸载器两次汇编会把它展开成
  `752.2.16` 这类复合 token，卸载段标签解析失败；**这种错只有真跑 makensis 才会暴露**，
  静态检查发现不了。
- **不要拿"文档写了规则"当"已经实现"。** 本次根因正是：AGENTS.md 早就写了
  「部署不得强杀正在连接的应用」，而 `installer-hooks.nsh` 里 45 行，一个字都没实现。
  → **规则必须落到代码 + 一条能自动跑的测试上**（契约测试：`src-tauri/src/lib.rs` 的
  `installer_hook_*`）。
- **判断第三方模板/工具的行为，要看"自己这次构建生成的产物"，不要看上游仓库。**
  上游默认分支会变：`CheckIfAppIsRunning` 在 tauri `dev` 分支已从
  `nsis_tauri_utils::KillProcessCurrentUser` 换成 Restart Manager
  （`RSTRTMGR::RmShutdown` + `RmForceShutdown`）。而本仓库实际生成的是
  `target/release/nsis/x64/utils.nsh`（外加 `SimpChinese.nsh` 里的真实弹窗文案）——
  以它为准，升级 CLI 后重新核对。本机 npm 包与 cargo 缓存里的版本才是真正生效的版本。

## 8. 结论要带词汇，被推翻就当场改

本次被自己否掉的结论（保留证据链，别偷偷改口）：

| 早期结论 | 被什么否证 | 修正后 |
| --- | --- | --- |
| `0x80070008` = 内核非分页池被占满 | 3.1 GB 物理内存空闲 + 提交量 58%（池取自物理内存） | 不是"池满"，是**通用失败码** |
| 用户态进程泄漏 BLE 资源 | 探针：本进程 ≤499 句柄 / ~14 MB | 楔死态在资源计数上**无签名** |
| 图标挂掉时应用也应该挂 | 日志：图标死时 SayAll 一切正常（它靠已缓存的 Radio 对象活着） | 死的是"**新建**对象"，不是"已持有" |
| 只有电源循环能清除 | 直接实验：伪 S3 无效 | 精确为**完整重启** |
| 第 10 轮靠 S3 收尾 | 那也是 2 秒伪 S3 | 撤回该推断，结束原因未确认 |
| 关开蓝牙能治 | 143 次 Off/On "执行成功"却无效 | 软件层开关**不释放**底层对象 |

规则：`passed` / `failed` / `deferred` 三选一，**被推翻就写进文档**——下一个人不该再走一遍。

## 9. 交付前的三个自问

1. 这条结论是我**观察到**的，还是我**推**出来的？（推出来的写 `deferred`）
2. 我的证据里还有哪个**环境假设**没验？（提权是否生效 / 是否真断电 / 是否有真实硬件）
3. 同一个根因**换一个入口**能不能复现？（同源症状应能在多个客户端观察到）

---

## 附：本次的高频判据速查

日志位置：`%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log`（`SAYALL_GATT_LOG` 可覆盖）

```bash
# 资源耗尽轮次与规模
grep -ac "windows_resource_exhausted" "$LOG"
# 资源快照（新增）：判"是不是资源被占满"
grep -a "resource_probe" "$LOG"
# 原始错误码与失败阶段（新增）
grep -aE "hresult=0x|failure_detail" "$LOG"
# 睡眠/唤醒（新增）
grep -aE "system_suspend|system_resume" "$LOG"
# 退出收尾（新增）：应看到 signal → ble_session_shutdown → platform_shutdown
grep -aE "app_exit " "$LOG"
```

系统侧（`wevtutil`，输出是 UTF-16，`grep` 需加 `-a`；`Date:` 字段带 `Z` 但**实际是本地时间**）：

```bash
wevtutil qe System "/q:*[System[Provider[@Name='Microsoft-Windows-Kernel-Power'] and (EventID=42 or EventID=107 or EventID=130 or EventID=131)]]" /c:8 /rd:true /f:text
wevtutil qe "Microsoft-Windows-Kernel-PnP/Configuration" /c:20 /rd:true /f:text   # 判设备重启是否真发生
wevtutil qe System "/q:*[System[Provider[@Name='Application Popup']]]" /c:10 /rd:true /f:text  # 系统弹窗文案
```
