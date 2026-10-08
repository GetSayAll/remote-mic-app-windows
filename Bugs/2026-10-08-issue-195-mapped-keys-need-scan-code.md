# Issue #195：遥控器按键映射到快捷键后，替换键收不到（注入事件缺 PS/2 扫描码）

- 发现日期：2026-10-04（用户 issue [#195](https://github.com/GetSayAll/remote-mic-app-windows/issues/195)，报告人 tinyredinc）；本仓库侧复现与定位 2026-10-08
- 状态：已修复（自动化 passed；真机验收 `deferred`，见文末清单）
- 影响范围：报告人现场为 SayAll 0.5.0；main 截至 `cdccd77`（0.8.2）同型未修；Windows 11 Pro 26H2 + 小米蓝牙语音遥控器 2 Pro（RC003）
- 功能点：按键映射 → 快捷键动作注入（`button_mapping` → `SendInputRuntime::tap` → `send_input_windows::build_input`）
- 现象：TV 单击绑定为 Esc 并保存后，日志 `map_fire … chord=Escape` + `map_inject result=ok`，但 keyboardtest.cn 与其他应用都收不到 Esc；同时物理键盘反引号失效
- 复现条件：报告人现场（0.5.0，映射前 TV 能打出 `` ` ``，映射后原始键与替换键都不可见）；本机未复现应用层现场（无同款第三方应用组合），只复现了机制层事实（`deferred` 部分见文末）
- 正常预期：遥控器 TV 单击只向当前前台应用发送 Esc；映射开启期间物理键盘反引号行为有明确、可预期的语义
- 证据：报告人 issue 正文日志（04:48:19Z–04:48:34Z）与两条真机 UAT 评论；本机 LL 钩子探针（2026-10-08）；`hardware/RC003/probes/ll_flag_logger.py`（可复跑同一判据）

## 根因（两件独立的事，不要串成一条因果链）

### ① 原始键与物理反引号：全局钩子按虚拟键吞，不按设备吞（设计内代价，本次未改）

TV 的原生键盘身份是 `VK_OEM_3`（`0xC0`，反引号；`raw_input.rs`）。映射一保存，
`persistent_suppress_mask` 把已映射的 Home/TV 放进常驻抑制掩码（"方案 C 遥控器优先"，
2026-09-07 用户选定，见 `docs/investigations/2026-09-06-left-double-response-arm-deadlock.md`），
LL 钩子在遥控器在线时对该虚拟键直接吞。低级键盘钩子没有设备来源，遥控器 TV 与物理键盘
反引号是同一个键 ⇒ **两者一起消失**。报告人日志里 `gate(sw=8→9→10, lk=0)` 与
`map_fire` 只隔 1ms（无 60ms 武装等待）正是这条常驻抑制路径。

这是既定取舍（换取 Home/TV 孤立冷首按也严格单响应），零代价终局是 Helper 轨（ADR 0002）。
RC003 打开「全按键支持」后由报告层按设备清空报告、钩子让位，物理反引号恢复——
这是报告人 UAT 里唯一全通过的组合。**本次不改这条路径**（product-copy/名单属产品决策，
见 `TODO.md` 的"全按键接管名单"待拍板项）。

### ② 替换键：注入事件没有扫描码，按物理键位认键的消费者收不到（本次修复）

`map_inject result=ok` 只证明 `SendInput` 把按下与抬起**入队**（返回事件数 = 计划数），
不证明任何应用收到了键。机制层事实（本机实测，判据与 `ll_flag_logger.py` 相同）：

| 注入形态 | LL 钩子实测（vk / 扫描码 / flags） |
| --- | --- |
| `wVk=0x1B, wScan=0`（修复前的 Escape 路径） | `vk=0x1B scan=0x00 flags=0x10` |
| `wVk=0, wScan=0x01 + KEYEVENTF_SCANCODE`（修复后） | `vk=0x1B scan=0x01 flags=0x10` |
| `wVk=0x7C, wScan=0`（修复前的 F13） | `vk=0x7C scan=0x00` |
| `wVk=0, wScan=0x64 + KEYEVENTF_SCANCODE` | `vk=0x7C scan=0x64` |
| `wVk=0xC0, wScan=0`（修复前的 `` ` ``） | `vk=0xC0 scan=0x00` |

`SendInput` 的纯虚拟键事件到达系统时 `scanCode = 0`——这与本仓库 2026-09-23 的
Raw Input 观测（`docs/investigations/2026-09-23-rc003-hid-host-write-tap-result.md`
§4.5：`MapCode=0x00`，而报告层改写出来的键拿到正常扫描码）是同一条事实。修复前
`KeyCode::physical_scan_code()` 只覆盖修饰键与 `Oem3`，其余键（Escape、字母、数字、
功能键、方向簇）全部走虚拟键注入 ⇒ 只认**物理键位/扫描码**的消费者（键盘测试网页、
部分 Electron 应用、游戏）看不到这些键，而按虚拟键认键的消费者（多数应用）照常收到。
报告人的 0.5.0/0.5.1 对照正落在这里：同一发 Esc，key-test 认（按虚拟键），
keyboardtest.cn 与 Cherry Studio 快速助手的"返回上一级"不认（按物理键位）。

⚠️ 撞号陷阱：Escape 的 HID usage 是 `0x29`，而 `0x29` 是**反引号**的 PS/2 Set-1
扫描码（Escape 的扫描码是 `0x01`）。把 `hid_usage()` 当扫描码填进 `wScan` 会打出 `` ` ``，
而且那发事件会再撞上 ① 的 TV 常驻抑制，症状与本次完全一样。本次改动禁止这条路径，
并有单测钉住两个值不同。

### 报告人诊断的采信与不采信

- 采信：扫描码缺失、`result=ok` 不足以当成功判据、设备级接管（报告层）才能同时满足
  "遥控器只出 Esc"与"物理 `` ` `` 仍可用"。
- 不采信：「注入前等钩子回调退出」——本仓库的注入在 `sayall-button-mapping` 引擎线程，
  不在钩子回调内（边沿经 channel 投递），该改动对本仓库是无操作。
- 不采信：「关掉 Home/TV 常驻抑制」——会把冷首按的原生键泄漏变成常态（报告人自己的
  0.5.1 关闭全按键支持时就出现 `` ` `` + Esc 双响应），与 2026-09-07 的既定决策冲突。
  本条属产品决策，不在本次修复范围。

## 修复（最小改动）

1. `crates/sayall-windows/src/send_input.rs`：`KeyCode::physical_scan_code()` 补全为
   **PS/2 Set-1 扫描码表 + 扩展（E0）标志**。期望值取自本机
   `MapVirtualKeyW(VK, MAPVK_VK_TO_VSC_EX)` 实测；扩展键（方向/Home/End/Ins/Del/Apps）
   的 E0 标志与既有 `is_extended()` 一致——漏掉会把方向键退化成小键盘数字键。
   媒体键没有标准 Set-1 扫描码，维持虚拟键注入（边界，有单测记录）。
2. `crates/sayall-windows/src/key_gate.rs`：新增注入**回声**读数
   （`InjectedEcho { count, vk, scan, flags }`，由宿主钩子转发的注入事件填字段）。
3. `crates/sayall-windows/src/button_mapping.rs`：`map_inject` 由 `result=ok` 改为
   `result=queued` + `echo_delta=/last_vk=/last_scan=/last_flags=`（最多等 60ms 取回声，
   等到即返回）。`echo_delta=0` 表示本次注入没有被本进程钩子宿主看到——这是可诊断的
   失败形态，而不是"成功但没反应"。
4. `crates/sayall-windows/examples/preset_inject_probe.rs`：真机探针的比对加上扫描码，
   让"注入键带正确扫描码"在真机上可判定（Esc 一行即本次的现场判据）。

未改动：常驻抑制（方案 C）、菜单键直接归因、语音键报告层合成槽、助手协议、按下/抬起
配对规则。

## 验证

- 阳性对照（先红后绿）：`escape_uses_its_own_set1_scan_code_and_never_the_hid_usage`、
  `standard_mapping_keys_carry_set1_scan_codes` 在实现前精确失败（`Escape → None`、
  `Backspace → None`），补齐表后转绿。
- 新增用例：Esc/Oem3 撞号、字母/数字/功能键/导航簇逐族扫描码与扩展标志、
  `set1_scan_codes_are_unique_across_keys`（表内不得撞号）、
  `media_keys_stay_virtual_key_only`（边界）、
  `shortcut_injection_note_reports_queue_and_hook_echo`（日志两分支且不得再出现 `result=ok`）。
- `cargo test --workspace`：32 + 4 + 317 + 1 + 1 + 106 passed / 0 failed
  （12 + 1 ignored 为既有用例）；`cargo fmt --all -- --check`、
  `cargo check --workspace`、`cargo check -p sayall-windows-app --features runtime-simulation`
  全部 passed。
- `cargo build -p sayall-windows --example preset_inject_probe` passed（真机执行见下）。
- 本机机制层探针（2026-10-08）passed：见上表（VK-only ⇒ `scan=0`；带扫描码 ⇒ 原样到达）。
- **本机真机 passed（走产品真实注入路径，2026-10-08）**：一次性探针调用
  `SendInputRuntime::tap({Escape})`，同进程 LL 钩子读回
  `DOWN vk=0x1B scan=0x01 flags=0x10` / `UP vk=0x1B scan=0x01 flags=0x90`，
  `tap_result=Ok(submitted_events=2)`——修复前同路径为 `scan=0x00`。
  （探针为临时文件，取证后已移入回收站，不入库。）
- 真机/应用层验收 `deferred`（需要在装有本包的真机上做）：
  1. `cargo run -p sayall-windows --release --example preset_inject_probe`：Esc 一行必须
     PASS（`scan=0x01`），方向/Home/Apps 行保持 PASS（E0 语义未退化）。
  2. TV 单击 → Esc：keyboardtest.cn 显示 Esc；Cherry Studio 快速助手用 Esc 返回上一级。
  3. 物理反引号语义与版本前一致（关闭「全按键支持」时被常驻抑制接管属既定代价；
     打开后应恢复）。
  4. RC001 无报告层，「全按键支持」两档都应在真机上确认替换键可达。

## 隐私检查

本文不含个人路径（一律 `%TEMP%` 或仓库相对路径）、不含蓝牙地址、设备身份、语音内容或凭据；
报告人日志为 issue 公开内容，未复制进仓库。
