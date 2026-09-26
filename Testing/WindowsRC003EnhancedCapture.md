# RC003 按设备源头捕获验收（E1–E6）

本手册覆盖「返回 / 音量+ / 音量−」三键在 RC003 上的**按设备源头捕获**（提权助手 + HID 宿主内 tap）。
它给出每项的命令、判据与日志字段。

**步骤是验收方法，不代表各项均已通过。** 每项的当前状态见文末状态表——那里写的 `passed`
表示真机实际执行并观察通过，`failed` 表示实际执行不满足预期，`deferred` 表示依赖当前不可得的条件。
编译通过、自检通过、模拟宿主通过，**都不算**这三项。

助手本体位于 `hardware/RC003/helper/`，证据在 `hardware/RC003/evidence/`，
路线背景见 `docs/investigations/2026-09-22-per-device-key-interception-route-selection.md` §6。

---

## 0. 先读这一节：RC003 上有一个可观测性缺口

**RC003 的三键在 Windows 侧本来就零事件。** 根因已定论：设备确实上报了这三个 usage
（`0x00F1` 返回 / `0x0080` 音量+ / `0x0081` 音量−，都在键盘页 `report_id=0x01` 内），
但 Windows 的 HID→VK 映射表缺这三个 usage，`kbdhid` 在映射阶段就把它们丢掉了。

由此推出一个很容易踩空的事实：

> 「清掉它们」与「不清」在外部**完全看不出差别**——音量不会变、也不会有任何字符出现。

所以传统判据「按了之后没有原生动作」在 RC003 上**恒为真**，无论清空是否真的生效。
它没有分辨力，用它来判「拦截生效」等于没判。

**解法：哨兵键（canary）。** 额外清掉一个 Windows **本来能处理**的键——主页键 `0x4A`：

```cmd
REM 提权运行；默认 120 秒
run-helper-canary.cmd
```

于是「清空有没有生效」变成肉眼可见的三步对照：

| 时刻 | 现象 | 说明 |
| --- | --- | --- |
| 运行前（记事本里按主页） | 光标跳到行首 | 基线：该键确实能到达 Windows |
| 运行中（`clear_usages` 含 `0x004a` 之后） | **什么都不发生** | 清空真的作用到了报告上 |
| 运行结束 / 租约到期 / Ctrl+C 之后 | 光标又能跳了 | fail-open：不残留清空、不需重启 |

这三步同时构成 E2 与 E4 的**直接**证据，而不是推断。安全边界：只清该 usage 的 2 个字节，
`report_id` / `modifiers` / `reserved` 一律不碰；仍受 `--duration` 上限与 agent 侧 2 秒租约双重兜底；
默认关闭，只有 `run-helper-canary.cmd` 会打开它。

> 哨兵键只影响**清空**，不影响**上报**。日志里 `[EDGE]` 只会是三键——每个 agent 心跳里的
> `report_usages` 恒为三键，`clear_usages` 才是实际清空范围。

### §0.1 2026-09-26：哨兵键曾是死代码（已修，E2 那一轮白跑了）

第一次真机跑 `run-helper-canary.cmd`，按主页键**光标照常跳到行首**——判据第 4 步不成立。
按上面的说法这该读作"清空没生效"，但日志一切正常：

```
[CANARY]  enabled=true … clear_usages=0x00f1,0x0080,0x0081,0x004a canary=0x004a
[CONFIG]  … targets_sent=true … clear_usages=…,0x004a canary=0x004a
[AGENT]   msg=targets:applied clear=0x00f1,0x0080,0x0081,0x004a canary=1
```

**根因在 agent 的门禁，不在拦截链路上**（`agent/rc003_agent.js` 的 `onEnter`）：

```js
var found = targetSetIn(bytes);      // 上报集合 = 三键
…
if (found.length === 0) return;      // ← 只含哨兵键的报告在这里就出去了
…                                    // 下面那段用 clearUsages 的清空循环永远执行不到
```

`targetSetIn()` 用的是**上报**集合（恒为三键），而清空用的是**清空**集合（三键 + 哨兵键）。
只含主页键的报告在门禁处就被 `return`，于是哨兵键从第一天起就没被清过——
"按主页无反应"这条判据被静默地变成了**恒不成立**。同时心跳里 `target_hits × 2 == edges_sent`
仍然自洽，所以从统计上也看不出少算了什么。

**修复**：门禁改为"上报 ∪ 清空"（`clearSetIn()` + `shouldTouchReport()`），哨兵命中单独计入
`canary_hits`，不污染 `target_hits`。

**回归锁**（两层，都不是"看源码有没有这行"就算了）：

| 层 | 入口 | 覆盖 |
| --- | --- | --- |
| 行为级（含阳性对照） | `node hardware/RC003/helper/agent/agent_logic_test.mjs [agent.js]` | 加载**真脚本**，用假指针喂 9 字节报告，断言缓冲区真的被改写；把门禁退回旧写法必须 FAIL（实测：19/19 → 14/19，5 条红，其中字节级那条 `buf3=0x4a` 正是真机症状） |
| 构建级 | `sayall-helper.exe --selftest` 第 23b / 23c 项 | 内嵌脚本必须含 `clearSetIn` / `shouldTouchReport` / `canary_hits`，且 `AGENT_BUILD` 与 JS 内一致 |

**同一天学到的第二件事：改了 agent 不会自动生效。** Gadget 一旦 `LoadLibrary` 进宿主，
脚本就再也不会被重新读取（同路径再 LoadLibrary 只加引用计数）。助手会走 `Attach`
接管旧 tap——握手正常、命令照发、日志漂亮，只是行为还是旧的。所以新增了 `AGENT_BUILD`
代次：接管到旧实例时日志会打 `[AGENT-STALE] running=… embedded=…`。
**看到这一条，就必须先让宿主换新（重启机器最稳），再重跑验收，否则又是白跑一轮。**

**修复已在真机上验证（2026-09-26 21:48，见 §3.1）**：`canary_hits` 会随主页键增长、
`clears_ok` 从 0 涨到 14，同一轮里"未武装 → 武装"的对照闭合，E2 判据成立。

> 宿主换新不一定非要重启：管理员 `taskkill /F /PID <宿主 pid>` 让设备重新枚举即可
> （pid 用 `sayall-helper.exe --dry-run` 查）。2026-09-26 就是这样把 pid=4032 换成 22620 的。
> 代价：RC003 会短暂断连并由主程序重连，且重新枚举后**可能又变回共享宿主**（正常，靠内容门禁）。

---

## 1. 入口与前置

| 入口 | 需要管理员 | 作用 |
| --- | --- | --- |
| `run-helper-selftest.cmd` | 否（可双击） | 内置 55 项自检。**第一步先跑这个** |
| `run-helper-dryrun.cmd` | 否（可双击） | 定位宿主 + 独占性核对 + 校验 Gadget 哈希；不注入、不监听 |
| `run-helper-observe.cmd [秒]` | **是** | 注入并上报按键边沿，**从不清报告**。零回归风险 |
| `run-helper.cmd [秒]` | **是** | 正式拦截：只清三键 |
| `run-helper-canary.cmd [秒]` | **是** | 拦截 + 哨兵键（主页），用于 E2 / E4 的可见验收 |

- 日志（追加语义，每轮带 `---------- 新一轮运行 <时间> ----------` 头）：
  `hardware/RC003/evidence/helper-run.log`
- 运行时目录：`%ProgramData%\SayAll\rc003-helper`（`frida-gadget.dll` / `rc003_agent.js` /
  `frida-gadget.config` / `session.token`）
- 退出码：`0` 成功 / `2` 参数 / `3` 未提权 / `4` 找不到 Gadget / `5` 宿主非独占 /
  `6` Gadget 校验失败 / `7` 运行时目录 / `8` 端口占用 / `9` 注入失败 / `10` 令牌文件 /
  `11` `--attach-only` 但没 tap / `12` `[NO-HELLO]` / `13` `[STALE-TAP]` / `14` 缺 `SeDebugPrivilege`

**验收用的二进制 ≠ 产品实际跑的那个（2026-09-26 实测）**：应用侧由计划任务拉起的是
**安装目录**里的 helper（`%LOCALAPPDATA%\无线麦 SayAll\sayall-helper.exe`），
不是仓库里 `hardware/RC003/helper/target\release\` 那份。判据是 `[ENV] agent_sha256=`——
它是**内嵌 agent 脚本**的 SHA-256：

| 来源 | 体积 / 时间 | `agent_sha256` | 含哨兵键修复 |
| --- | --- | --- | --- |
| 仓库构建（`src-tauri/*.exe` 与 spike 构建同源） | 915968 B / 09-26 21:00 | `1cbd55d8…` | 是（`canary-gate`） |
| 安装版（应用实际拉起的那份） | 908288 B / 09-25 01:02 | `27381d72…` | 否 |

影响（**对当前产品路径无害，但要知道**）：产品路径 `canary=none`，清空集合 == 上报集合，
所以旧脚本的门禁缺陷在产品路径上不表现；但① 运行时目录里的 `rc003_agent.js` 会被
安装版覆写成旧脚本，**下一次全新注入（宿主重启 / 重启机器后）加载的就是旧脚本**；
② 此后任何 agent 改动都必须先让安装包带上新 helper 才会生效。

机检示例（本机 Git Bash；`grep -F` 是必要的，`grep -c '\[..\]'` 在本机会把 `\[` 当字符类而给出假计数）：

```bash
LOG=hardware/RC003/evidence/helper-run.log
grep -cF '[EDGE]' "$LOG"                      # 边沿条数
grep -nF -e '[CANARY]' -e '[CONFIG]' -e '[SUMMARY]' "$LOG" | tail -20
```

---

## 2. E1 — TV 的 Shell 协议动作来自哪个 report

- **目的**：确认「TV / 主页 / 返回 / 音量±」是否**全部**由 `report_id=0x01` 引发。
  若 TV 来自别的 report（参考实现记录该设备另有 vendor report `0x06/0x07/0x08`），
  只清 `0x01` 就漏了它——表现是「三键被清掉、TV 动作照旧」。
- **方法**：用**只读** tap 记录该设备**全部 report ID 与原始字节**（只记录、不改写，
  `onLeave` 不做任何回写），逐键采集 TV / 主页 / 返回 / 音量±。
- **判据**：得到「TV 按压 ↔ 完整 report 清单」的对应关系；确认是否只有 `0x01`。
- **日志/证据字段**：需要的是 report 级原始字节落盘（当前 agent 不打这个，只打 `[EDGE]` 的 usage）。
- **当前状态**：**未完成**。现成材料：`0x01` 的布局已确认（`report_id` + `modifiers` +
  `reserved` + 3 × `uint16` usage 槽），`0x01` 覆盖返回 / 音量± / 确定 / 主页；
  vendor report 是否存在仍待实测。相关文档：
  `docs/investigations/2026-09-23-rc003-hid-host-readonly-tap-result.md`。
- **失败归因**：若 TV 不在 `0x01`，需把清空范围扩展到对应 report——**不要**先改代码，
  先拿到 E1 的 report 清单。

## 3. E2 — 清空是否真的消除全部原生动作

**在 RC003 上必须用哨兵键，否则本项无法判定（见 §0）。**

1. **基线（阳性对照，不可省）**：打开记事本，按遥控器**主页**键 → 光标应跳到行首。
   若无反应，说明该键不可达，本项**不能做**（先排查设备）。
2. 提权运行 `run-helper-canary.cmd 120`。
3. 等日志出现 `[CANARY] … clear_usages=0x00f1,0x0080,0x0081,0x004a`，
   并且 `[CONFIG] … targets_sent=true`。
4. 在记事本里按**主页** → **不应有任何反应**（这是「清空生效」的证据）。
5. 按**返回 / 音量+ / 音量−** → 仍应出现三组 `[EDGE]`（证明捕获没坏，
   只是它们在 Windows 侧本来就没有原生动作）。
6. 让本轮自然结束（等 `[TIMEUP]`），或按 Ctrl+C → 出现
   `[TIMEUP] 已达 --duration …` → `[NOTE] 已向 agent 发送 disarm…` → `[SUMMARY] …`。
7. 再按**主页** → **应恢复**（光标又能跳）。

- **判据**：第 4 步无反应 **且** 第 7 步恢复。两条缺一不可——只有第 4 步的话，
  「设备掉线了」也能造出同样的现象。
- **日志字段**：`[CANARY] enabled/clear_usages`、`[CONFIG] targets_sent/clear_usages`、
  `[AGENT] msg=targets:applied clear=… canary=1`、`[EDGE]`、`[SUMMARY] how=injected|attached_existing_tap`
- **仍 deferred 的部分**：锁屏场景（本机 Edge 已安装 / 未安装两种状态下，
  锁屏不出现协议选择器）。这一条依赖 E1 的结论。
### §3.1 E2 实测记录（2026-09-26 21:48:26–21:53:21，宿主 pid=27436）

- 前置：`[HELLO] build=2026-09-26.canary-gate`（**修好的脚本**在跑）、**无** `[AGENT-STALE]`、
  `how=attached_existing_tap`、`clear_usages=0x00f1,0x0080,0x0081,0x004a`。
- **日志侧判据成立**：`[EDGE]` 13 条（`back` / `volume_up` / `volume_down` 各按下+抬起，
  `[SUMMARY] edges=12 distinct_buttons=|back|volume_down|volume_up`）；
  `canary_hits` **23 → 31**、`clears_ok` **0 → 14**、`clears_fail=0`、`write_fail=0`、
  `kernel_changed=0`、`restores_ok=14`。
- **同一轮里自带 A/B（最硬的一条）**：该 agent 是本轮之前就活着的（上一轮 21:26:54 注入）。
  在 21:26:59 上一轮助手被杀 → 21:48 本轮武装之前，它见过 **23 次**主页键报告，
  `clears_ok` 一直是 **0**（租约过期 → fail-open，主页键保持原生行为）；
  本轮武装后，8 次主页键报告 + 6 次三键报告 → `clears_ok` **涨到 14**。
  **同一个键、同一个宿主、同一份脚本，唯一变量是「是否武装」**——这就是「清空生效」的直接证据。
- 收尾：`[TIMEUP] 已达 --duration 300s` → `[NOTE]` → `[SUMMARY]`，退出码 0。
- **目视确认（操作人）**：第 4 步武装期间按主页**光标不动**；第 7 步结束后按主页**恢复跳行首**。
- **当前状态**：**passed**（2026-09-26 21:53）。日志侧计数 + 目视两步都成立，
  E2 的判据（"第 4 步无反应 **且** 第 7 步恢复"）完整闭合。

## 4. E3 — 物理键盘零影响

1. 提权运行 `run-helper.cmd 120`（或哨兵键版；哨兵键清的是**遥控器**的主页键，不影响物理键盘）。
2. 遥控器在线期间，用**物理键盘**高频输入：`` ` `` `~` `/` `Home` `←→↑↓` `Enter` `音量±`。
3. 同时按遥控器三键（制造主机侧负载）。

- **判据**：物理键盘全部键照常，无吞键、无延迟异常；遥控器三键的 `[EDGE]` 不缺不重。
- **日志字段**：`[EDGE]` 的条数与 `[SUMMARY] edges=` 一致；无 `[WARN]`。

### §4.1 E3 实测记录（2026-09-26 22:42–22:52）

**先记一个当时真实存在的顾虑**：RC003 的宿主是**共享**的，同宿主里有一台**蓝牙键盘**
（`VID&0232C2_PID&6621`，`kbdhid`，COL01 / COL04 两个键盘 TLC，另有鼠标、用户控制、系统控制）。
而我们的门禁是**按 usage**（`0xF1/0x80/0x81`）而**不是按设备** ⇒ 同一宿主里任何设备
只要用**键盘页** `0x0080/0x0081` 发音量键，就会被清掉、并被上报成 RC003 的边沿。
判据不能靠"我觉得不会"，所以做了隔离复测：

- **复测方法**：`--observe`（只上报、**绝不清空**）轮里**只按物理键盘的音量键**。
- **结果**：`ioctl_calls` 145252 → 150948（探针确实在收报告），但 **`[EDGE]` 一条都没有**、
  `target_hits` 停在 15 未动 ⇒ 那台键盘的音量键**不产生**我们 hook 的那类报告
  （是消费页 `0x0C` 的 TLC，不是键盘页 `0x80/0x81` 的 9 字节报告）⇒ **无串设备**，物理键盘零影响成立。
- 误报澄清：早先报的"物理键盘音量+/− 异常"其实是**遥控器**的音量键
  （原生没动作 → 清掉后由应用接边沿跑映射）；`` ` `` / `~` 打不出来是 TV 键那条线的事
  （usage `0x35` 不在清空集合，构造上不可能被清）。
- **残留风险（未消除，只是今天没触发）**：usage 门禁 ≠ 设备门禁。换一台用键盘页
  `0x80/0x81` 的设备接进同一宿主，仍会串；而 agent **不记录报告来自哪个设备/句柄**，
  日志里无法归因。要彻底闭环需给 hook 加设备维度。

### §4.2 边沿不缺不重 —— 决定性一轮（2026-09-26 23:14:35，产品路径 / `canary=none` / 每键只按一次）

上一轮（22:48–22:52，run 模式）是 `back` 4 / `volume_up` 3 / `volume_down` 3、
`[SUMMARY] edges=20`、`clears_ok` +10（与按下 1:1）、`clears_fail=0`、
`app_bridge=connects=2 failed=0 edges=20`；按下沿相邻间隔 ≥645 ms ⇒ 不是同一按的重复上报。
但**操作人无法回忆返回键究竟按了 4 次还是 3 次** ⇒ 多出的那 1 条无法归因，"边沿不缺不重"未闭合。

- **时间序列（先把「多半是多按了一次」钉成可讨论的形态）**：

  | 边沿 | 距首条 |
  | --- | --- |
  | `back` | +0.00 s（**孤立的 1 次**） |
  | `back` ×3 | +15.93 / +16.57 / +17.67 s（间隔 0.64 / 1.10 s） |
  | `volume_up` ×3 | +22.93 / +24.48 / +25.64 s（间隔 1.55 / 1.16 s） |
  | `volume_down` ×3 | +27.34 / +29.04 / +30.36 s（间隔 1.70 / 1.32 s） |

  形态 = **1 次孤立的 back + 一组「back×3 → volume_up×3 → volume_down×3」**，
  组内间隔三键同构；而 `volume_up` / `volume_down` 恰好都是预期的 3 次 ⇒ 计数确实是 1:1。

- **决定性一轮（每键只按一次）**：操作人在应用里关/开「增强捕获」→ 计划任务
  `SayAll RC003 Helper` 拉起安装版 `sayall-helper.exe --follow-app`（pid=25032，
  宿主仍是 27436，`how=attached_existing_tap`）。**免 UAC、无 `--duration` 时限**，
  日志 `C:\ProgramData\SayAll\rc003-helper\helper-task.log`。

  ```
  [EDGE] buttons=back        raw=t=…5681221 usages=0x00F1 n=1
  [EDGE] buttons=(释放)      raw=t=…5681326                n=0   (+105 ms)
  [EDGE] buttons=volume_up   raw=t=…5684970 usages=0x0080 n=1   (+3.75 s)
  [EDGE] buttons=(释放)      raw=t=…5685107                n=0   (+137 ms)
  [EDGE] buttons=volume_down raw=t=…5687446 usages=0x0081 n=1   (+2.48 s)
  [EDGE] buttons=(释放)      raw=t=…5687506                n=0   (+60 ms)
  ```

  **恰好 3 条按下 + 3 条释放，三键各 1 条** —— 无多余边沿、无丢失。

- **结论**：报告层的按下/释放计数是 **1:1，不重不漏** ⇒
  上一轮多出的那 1 条 `back` 是**操作人真的多按了一次**（开头那次孤立的试探按），不是重复上报。
  **E3 两项（物理键盘零影响、边沿不缺不重）均 passed。**
- 注：这一轮 `[ENV] agent_sha256=27381d72…`（09-25 安装版），其 `[HELLO]` 行**不带** `build=`
  —— 该字段是 09-26 才加到助手里的，**不等于**宿主里跑的是旧 agent（宿主 27436 里的 tap
  是 21:26 由新构建注入的）。统计三键边沿时 `canary=none`，新旧 agent 行为一致。

## 5. E4 — 失效与恢复

四个子场景。**全部用哨兵键观察恢复**，否则同样不可观测。

| 子场景 | 操作 | 判据 |
| --- | --- | --- |
| a 助手被强杀 | 任务管理器结束 `rc003-helper.exe` | ≤2 s 后**主页键恢复**（agent 侧租约过期）；日志停在上一条 `[HB]`，**没有** `[SUMMARY]` |
| b `--duration` 到期 | 起一轮短时长，什么都不做 | `[TIMEUP]` → `[NOTE]` → `[SUMMARY]`，退出码 0；主页键恢复 |
| c Ctrl+C | 运行中按 Ctrl+C | 同上（控制台处理器置位 → 主循环收尾 → 发 disarm） |
| d BLE 断连 / 睡眠唤醒 / UAC 拒绝 / 卸载 | — | **deferred**（依赖当前不可得的条件） |

- **判据**：拦截能力 `fail-open`（失效后放行原生边沿）、不残留清空、不执行迟到映射、不需重启。
- **日志字段**：`[TIMEUP]`、`[NOTE]`、`[SUMMARY]`、`[STOP]`、`[NO-HELLO]`、`[STALE-TAP]`；
  agent 侧计数在下一轮 `[HB]` 的 `stat` 里可见（`lease_expired` / `read_errors` / `rx_timeouts`）。
- **b 已执行并通过（2026-09-26）**：两轮都自然走到时长上限并干净收尾——
  `--duration 60`（21:46:25–21:47:41）与 `--duration 300`（21:48:26–21:53:21），
  均为 `[TIMEUP]` → `[NOTE]` → `[SUMMARY]`、退出码 0、`how=attached_existing_tap`。
  主页键恢复见 E2 §3.1 的 A/B（`clears_ok` 停止增长、fail-open 生效）。
- **a 已执行并通过（2026-09-26 22:08:07）**：提权 `taskkill /F` 强杀助手 → 助手 `exit=1`、
  日志**无 `[TIMEUP]`、无 `[SUMMARY]`**（与 b 的干净收尾正好形成对比）、进程消失；
  末条心跳 `canary_hits=39 / clears_ok=14 / clears_fail=0`；操作人随后确认
  **主页键恢复跳行首** ⇒ fail-open 成立、无残留清空。
  - 旁证（更早一次意外强杀）：21:26:54 那轮助手在 `up=5s` 被杀后，到 21:48 重新武装前，
    agent 见过 23 次主页键报告而 `clears_ok` 始终为 0。
  - **「≤2 s」标为机制上界（agent 租约 2000 ms + 500 ms 续约），未做人肉秒表**，并如实记录原因：
    agent 侧无法自证秒数（助手死后就不再有心跳）；我的提权 kill 必须过一次 UAC，
    无法把"杀的时刻"对齐到某一秒。要补就由操作人在杀的同时用秒表读。
- **c 已执行并通过（2026-09-26 22:52:48）**：运行中在控制台按 Ctrl+C →
  **无 `[TIMEUP]`**（区别于 b 的到期路径）、`[SUMMARY] uptime_s=253 … edges=20` →
  `[NOTE] 已向 agent 发送 disarm…`、退出码 0、`clears_fail=0`。
- **当前状态**：**a / b / c 通过**；d **deferred**。

## 6. E5 — 全键纯注入可用性

- **目的**：逐键在记事本 / 浏览器 / 微信输入法 / 管理员窗口（UIPI）下验证**映射动作**是否可用。
- **当前状态**：**deferred**。`helper` 这个 spike 只做「捕获 + 上报 + 选择性清空」，
  **不含映射动作**（不做 `SendInput` 注入）。本项属于产品侧的映射引擎，
  必须等三键真正接到映射链路上之后再验收。参考注意点：
  本仓库已证实微信输入法 / 豆包会过滤**注入的和弦**（见 `Bugs/2026-09-04-*`），
  所以「遥控器上的物理按键 → 我们注入的键」这条路要单独验。

## 7. E6 — 共存与安全软件

- **目的**：反作弊 / 杀软共存，以及注入被拦时的表现。
- **判据**：至少记录一次实测；无法验证的具体游戏保持 `deferred` 并写进用户文档。
- **当前状态**：**deferred**。

---

## 7.5 E7 — 桥接连通性（主程序 ↔ 助手，**不需要设备**）

- **目的**：验证捕获链第 ② 段（传输）真的通了。这一段不通时，三键会被清空但
  **映射永远不触发**——也就是"按下去什么都没发生"。
- **为什么单独列一条**：它**不需要遥控器按键**，只需要一次提权。
  是当前性价比最高的验收点（第 ① 段的捕获已 passed，第 ③ 段引擎零改动）。

**步骤**

0. **免提权前置检查（可选但建议先做）**：`run-helper-dryrun.cmd`，看 `[DRY-RUN-BRIDGE]`。
   - `probe=ok port=… token_len=…` → 助手能看见主程序写的描述文件（路径与权限都对）；
   - `probe=absent(主程序未运行？)` → 主程序没开，或描述文件不在 `%LOCALAPPDATA%\SayAll\`；
   - `probe=invalid(版本不符或字段缺失)` → 两侧协议版本不一致。
   这一步把"两边都在跑却谁也没看见谁"这类最费时间的问题**前移到提权之前**。
1. **启动主程序——而且必须是"包含桥接的那一版"**。
   - 常规做法：`pnpm tauri dev`（会自动重编译）。
   - **本机注意（2026-09-23 实测）**：这台机器 **`pnpm` 不在 PATH 上**
     （整个用户目录搜不到 pnpm，只有 node/npm），`pnpm tauri dev` 直接跑不了。
     替代入口是项目根的 **`dev-app.cmd`**（双击即用，本机专用、不入库）：
     它用 node 的绝对路径起 vite dev server（`127.0.0.1:2430`），
     再启动已构建的 `target/debug/sayall-windows-app.exe`，并在 8 秒后
     **报告描述文件有没有写出来** —— 于是"这一版有没有桥接"不用提权就能当场确认。
     （若该文件不存在，让助手重新生成，或改用 `pnpm tauri dev`。）
   - **单实例守卫会静默吃掉第二次启动**：主程序用命名互斥体
     （`SayAll.Windows.SingleInstance`）做单实例保护，检测到已有实例时**直接 return**，
     只在 stderr 打一句"SayAll 已在运行"——界面上**什么都看不到**。
     所以"重启"= **先关掉旧窗口，再启动**；否则你会看到一个没有任何错误的假失败。
   - 启动成功的标志：`%LOCALAPPDATA%\SayAll\rc003-bridge.ini` 出现
     （**不需要**读它的内容）。
2. 以管理员运行助手：`run-helper.cmd 30`（30 秒足够；`--await-hello` 到时会以退出码 12 结束，
   这是**预期**，因为设备可能没连或没按键）。
3. 看日志里是否出现 `[APP-BRIDGE] event=connected`。
4. 收尾后看 `[SUMMARY]` 的 `app_bridge=` 字段。

**判据**

| 观察 | 结论 |
| --- | --- |
| `[APP-BRIDGE] event=enabled` + `event=connected` | 描述文件被发现、令牌被接受、握手完成 → 第 ② 段**通了** |
| `[APP-BRIDGE] event=unavailable reason=descriptor_missing(<完整路径>)` | 主程序没在跑（**正常现象**），或描述文件不在该路径。**若主程序确实在跑**，先确认跑的是**接线之后的构建**（见下） |
| `reason=rejected(DENY ...)` | 令牌不匹配：主程序已重启而助手握着旧令牌，等助手下次重连即可；持续出现则说明描述文件被别的进程写入 |
| `reason=descriptor_invalid_or_version_mismatch` | 两侧协议版本不一致（助手与主程序需同版本） |
| `[SUMMARY] app_bridge=connects=0 failed>0` | 第 ② 段**没通**，据 `app_bridge_last_error` 定位 |
| `[SUMMARY] app_bridge=connects=1 edges=0` | 桥连上了，但**连接期间没有边沿**——没按三键时这是正常的；按了还是 0 则要回头查第 ① 段 |

**⚠️ 先证明"跑的是哪一版二进制"（2026-09-23 首轮踩过）**：E7 第一次执行时，
主程序进程在跑、但桥接毫无动静——因为跑的是**接线之前构建的** exe。
三条判据任选其一即可确认，**在怀疑协议之前先做这一步**：

1. `%LOCALAPPDATA%\SayAll\Logs\sayall-diagnostic.log` 里应有 `rc003_bridge phase=listening`
   （桥接启动时必打）；**没有这条记录 = 那段代码从未执行**；
2. `%LOCALAPPDATA%\SayAll\rc003-bridge.ini` 应存在（主程序启动即写出）；
3. `target/debug/sayall-windows-app.exe` 的修改时间应晚于接线提交的时间。

"应用侧没看到效果"有两个完全不同的原因——**代码没生效**与**代码有 bug**。
不要跳过这一步直接查协议。

- **失败时的先后顺序**：先确认 `[ENV] elevated=true`（否则根本没注入）→ 再看
  `[APP-BRIDGE]`（第 ② 段）→ 最后看 `edges`（第 ① 段到第 ② 段的交界）。
  **`captures 有值而 edges=0` 是"键能按、映射不动"的确切特征**，
  最容易被误读成"注入没成功"。
- **当前状态**：**未执行**（实现与离线验证已完成，缺一次提权真机跑）。

---

## 8. 状态汇总

| 项 | 状态 | 依据 / 缺口 |
| --- | --- | --- |
| E1 report 归属 | **未完成** | 需要只读 tap 记录全部 report；`0x01` 覆盖三键+确定+主页已确认，TV 未确认 |
| E2 清空消除原生动作（含哨兵键） | **passed**（2026-09-26 21:48，见 §3.1） | 修好的 agent（`build=2026-09-26.canary-gate`，无 `[AGENT-STALE]`）；同轮 A/B：未武装 23 次主页键 `clears_ok=0` → 武装后 `clears_ok=14`、`canary_hits=31`、`clears_fail=0`；目视第 4/7 步已由操作人确认 |
| E2 锁屏场景 | **deferred** | 依赖 E1 |
| E3 物理键盘零影响 | **passed**（2026-09-26，见 §4.1 / §4.2） | observe 轮隔离复测：只按物理键盘音量键 ⇒ `ioctl_calls` +5696 但 `[EDGE]` 零条、`target_hits` 不动 ⇒ 无串设备。决定性一轮（23:14 产品路径、每键只按一次）**恰好 3 按下 + 3 释放** ⇒ 边沿 1:1 不重不漏；22:48 那轮多出的 1 条 `back` 是操作人多按的一次（时间序列 = 1 次孤立 back + back×3→volup×3→voldown×3） |
| E4 a 强杀助手 | **passed**（2026-09-26 22:08） | 提权 `taskkill /F` → `exit=1`、无 `[TIMEUP]`/`[SUMMARY]`、进程消失、`clears_fail=0`；操作人确认主页键恢复。`≤2 s` 为机制上界（租约 2000 ms），未做人肉秒表 |
| E4 b `--duration` 到期 | **passed**（2026-09-26，60 s 与 300 s 两轮） | 均 `[TIMEUP]` → `[NOTE]` → `[SUMMARY]`、退出码 0 |
| E4 c Ctrl+C | **passed**（2026-09-26 22:52） | **无 `[TIMEUP]`**、`[SUMMARY]` → `[NOTE] 已向 agent 发送 disarm`、退出码 0、`clears_fail=0` |
| E4 d 断连/睡眠/UAC/卸载 | **deferred** | — |
| E5 全键纯注入 | **deferred** | helper 不含映射动作 |
| E6 安全软件共存 | **deferred** | — |
| E7 桥接连通性（主程序 ↔ 助手） | **connected 已达成**（19:55 主程序侧 `helper_authenticated`） | **不需要设备，只需一次提权**；判据与"先证明跑的是哪一版二进制"见 §7.5 |
| E7-b 按三键 → `[EDGE]` → 主程序侧投递 | **passed**（2026-09-23 19:11） | 助手侧 **18 条 `[EDGE]`**（三键各 3 次按下+释放，各键齐全）；主程序侧 `event=first_edge pressed=0x00F1`、收尾 `event=closed edges=18 released=0 dropped=0`；全程 `lease_ok=true` |
| **三键端到端映射生效**（按下 → 执行已配置动作） | **passed**（2026-09-23 19:18） | 在「按键」页给**返回键**配置"键盘 b" → 按遥控器返回键 → **记事本打出 b**。本轮 16 条 `[EDGE]` 全为 `back`（8 次按下+释放），主程序侧 `first_edge pressed=0x00F1` / `closed edges=16`。证据 `evidence/rc003-bridge-e2e-2026-09-23.log` |
| 桥接状态在按键页可见 | **passed**（2026-09-23 20:35） | 主程序 `source_revision=80cc6c3` 之后，按键页顶部显示「三键桥接：助手已连接（端口 65169），已投递 24 条边沿」。**UI 的 24 与助手侧 `[EDGE]` 计数 24 逐条对上** ⇒ 显示的是真实投递数。仅在连接 RC003 时渲染（RC001 走 `key_gate`，不显示以免误导） |
| 三键端到端映射生效（按返回 → 执行已配置动作） | **未执行** | 依赖 E7；实现已完成（引擎零改动，复用 RC001 的 `GateEdge` 通道） |
| 三键边沿送达（`observe`） | **passed**（2026-09-23） | `evidence/2026-09-23-observe-run.log`：30 条 `[EDGE]`，`target_hits=15` 与按下 1:1 |
| 反复运行 / 接管路径 | **passed**（2026-09-23） | `evidence/2026-09-23-acceptance-run.log`：注入轮 `how=injected`、接管轮 `how=attached_existing_tap` |

---

## 9. 日志字段速查

| 行 | 关键字段 |
| --- | --- |
| `[ENV]` | `elevated` `pid` `port` `runtime_dir` `target_pid` `gadget_sha256` `agent_sha256` |
| `[REG]` / `[REG-WARN]` | `diag_keys` `read_failures` |
| `[HOST]` / `[EXCLUSIVE]` | 宿主 pid / 进程名 / 是否独占 RC003 |
| `[CANARY]` | `enabled` `report_usages` `clear_usages` `canary` |
| `[HELLO]` | `auth` `pid` `agent` **`build`（脚本代次）** `mode` `restore` `lease_ms` |
| `[AGENT-STALE]` | `running` `embedded` `host_pid` —— **出现即表示本轮接管的是旧脚本，新逻辑不会生效**（见 §0.1） |
| `[TOKEN]` | `source=file\|generated\|explicit` |
| `[PREP]` | `dll` `action=reuse\|overwrite\|skip_copy_attaching_existing_tap` `reason` |
| `[TAP]` / `[ATTACH]` | 宿主内已有 tap 的路径 / 接管决定（`skip_injection`） |
| `[DLL]` | `dll` `reused_existing` `sha256_verified` `copied` `plan` |
| `[LISTEN]` | `addr` |
| `[INJECT]` | `pid` `dll` `wait` `hmodule` `loaded` |
| `[VERIFY-MODULE]` | `gadget_modules_in_host`（**核实用它，不要信 `LoadLibraryW` 返回值**） |
| `[HELLO]` | `auth` `pid` `agent` `mode` `lease_ms` |
| `[CONFIG]` | `arm_sent` `mode_sent` `restore_sent` `targets_sent` `flush` `mode` `restore` `report_usages` `clear_usages` `canary` `ack=n/a` |
| `[EDGE]` | `buttons` `raw=t=… usages=0x00F1,… buttons=… n=…` |
| `[HB]` | agent 心跳：`mode` `report_usages` `clear_usages` `lease_ok` `stat{…}` |
| `[AGENT]` | agent 侧 `logLine` 通道；`targets:applied clear=… canary=…` 在这里 |
| `[APP-BRIDGE]` | 第 ② 段：`event=enabled\|disabled\|connected\|unavailable\|descriptor_found_elsewhere\|closed\|replaced_by_new_connection` `descriptor` `reason` `tries` |
| `[DRY-RUN-BRIDGE]` | `--dry-run` 下的**只读**探测（不连接）：`descriptor` `probe=ok\|absent\|invalid`；`probe=absent` 表示主程序没在运行（正常现象） |
| `[TIMEUP]` / `[NOTE]` / `[SUMMARY]` | 收尾三行。`[SUMMARY]`：`uptime_s` `agent_lines` `renews_sent` `edges` `distinct_buttons` `authenticated_ever` `how` `app_bridge=connects=N failed=M edges=K` `app_bridge_last_error` |
| `[NO-HELLO]` / `[STALE-TAP]` / `[STOP]` | 失败归因（配合退出码 12 / 13 / 各自的 STOP 码） |

注：**没有 `[DISARM]` 这个标签**——收尾时打的是 `[NOTE] 已向 agent 发送 disarm…`。

---

## 10. 常见失败与处置

| 现象 | 含义 | 处置 |
| --- | --- | --- |
| `[STOP] 绑定 127.0.0.1:47831 失败` + 退出码 8 | 上一轮还在运行（单实例锁） | 先结束上一个助手窗口；或等 `--duration` 到期 |
| `[STOP] 复制 Gadget 失败 … os error 32` | 宿主仍映射着运行时目录那份 DLL | 更新版已不会这样：会走 `[ATTACH]` 接管或 `[STALE-TAP]` 提示 |
| `[STALE-TAP]` + 退出码 13 | 宿主里的 tap 是**旧世代**（令牌已换，接不上） | 按提示清一次：断开重配对 RC003（最干净）/ 设备管理器禁用再启用 / 重启 |
| `[NO-HELLO]` + 退出码 12 | 注入了但 30 s 内没有**已鉴权**会话 | 看 `[VERIFY-MODULE] gadget_modules_in_host`；确认没有第二个助手在抢端口 |
| 按了键但一条 `[EDGE]` 都没有 | 可能是设备没上报，也可能是探针/链路坏了 | **先做阳性对照**：用哨兵键或 `observe` 模式确认同一设备上已知可用的键（确定/主页）能出来 |
| `[EDGE]` 有、但主程序里三键映射不触发 | 清空与转发是两件事：第 ② 段（桥接）没通 | 看 `[SUMMARY] app_bridge=`：`connects=0` → 启动主程序并确认 `%LOCALAPPDATA%\SayAll\rc003-bridge.ini` 存在；据 `app_bridge_last_error` 定位（`descriptor_missing` / `rejected(DENY…)` / `version_mismatch`）。判据见 §7.5 |
| 哨兵键不恢复 | fail-open 失效——**这是真问题** | 立刻按 Ctrl+C / 关窗；仍不恢复就断开重配对 RC003；把日志留档 |
