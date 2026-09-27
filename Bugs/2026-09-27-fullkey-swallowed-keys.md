# 2026-09-27 全按键支持开启后按键无法输入（吞键缝隙）

## 现象

Andy 真机反馈（v0.2.6, source_revision=e354137，19:28 会话 pid=29908）：开启「全按键支持」后，键盘无法输入——按遥控器/键盘上的键没有反应，**退出无线麦后才能输入**。

诊断日志佐证：遥控器按键边沿正常到达（`map_edges detail=Tv=true/false`），但全部
`map_skip_inject reason=action_disabled button=Tv trigger=Single`——按键被引擎吞掉且无任何输出。

## 根因：两层判定条件不一致的吞键缝隙

「全按键支持」（PR：codex/full-key-enhanced-capture，TODO 2026-09-27 条目）把增强捕获
目标从三键扩展为「开启状态 ∩ `mapped_mask()`」，且有两条吞键路径：

1. **报告层**（Helper 在 WUDFHost 内清空目标 usage 的键盘报告）；
2. **门控**（key_gate 低级键盘钩子，其中 Home/TV 按 2026-09-07 方案 C 无需武装直接吞）。

吞键按**整键**判定：`mapped_mask()` = `any_configured()` = **三列（single/double/long）
中任一列有动作**。而动作注入按**单列**判定：`action_for(button, trigger)`。于是：

- 某键只配置了 **long**（或 double）动作、**single 列是 Disabled** 时，整键被吞；
- 单击触发进入 `fire_gesture` 的 `ButtonAction::Disabled` 分支，直接 `return false`；
- 原始键已被报告层/门控吞掉，注入又被跳过 → **按键凭空消失**。

真机映射表（targets `usages=35,4a,65,80,81,f1`）中 Tv(0x35) 即此形态：long 有动作、
single Disabled → 单击 `~`（OEM_3）打不出。同理影响 Home/Apps 等配置过 long 的键。

「还是」无法输入的背景：方案 C（2026-09-07）早已接受 Home/TV 接管劫持物理键盘的代价
（低频键），但 Disabled 触发列的彻底消失是本次新暴露的缝隙——连回注都没有。

## 修复（本提交）

1. **`fire_gesture` Disabled 分支回注原生键**（button_mapping.rs）：判据沿用泄漏对冲
   标记——`native_pending` 不含该键 = 原始键未进 OS（门控吞下或报告层接管），回注一次
   原生完整按压（`injector.tap` = DOWN+UP 成对；key_gate 对 `LLKHF_INJECTED` 放行，
   不会被二次拦截）。泄漏路径（native_pending 含该键，原生已交付）保持跳过防双输入。
   厂商键（Back/Power，`native_key=None`）维持「无动作即无输出」既有语义。
2. **`KeyCode::Oem3` 变体**（send_input.rs）：VK 0xC0 / 扫描码 0x29，`~` 键可回注；
   serde 序列化为 `oem3`，仅新增枚举值，旧配置兼容。
3. **`native_key(Tv)` → `Oem3`**：TV usage 0x0035 在 Windows 键盘映射里就是 OEM_3；
   泄漏对冲（native_covers）同步受益。

## 测试与阳性对照

- 新增 `gate_edge_disabled_trigger_replays_native_key_leak_path_skips`（4 场景）：
  门控吞下的 Tv/Up Disabled 单击分别回注 `Oem3`/`Up`；泄漏路径 Up 单击不回注；
  厂商键 Back 不产生 tap。**阳性对照已验证**：禁用回注后测试精确红在场景 1
  （`实际 []` vs `期望 [Oem3]`）。
- 更新 `native_key_covers_common_keys_and_none_for_vendor`（Tv 由 None → Oem3）。
- `sayall-windows` lib：161 passed（含既有 flaky 项本次通过）；
  `cargo check -p sayall-windows-app --features runtime-simulation` 通过；fmt 通过。

## 边界与残留

- **Double 触发被吞时只回注一次按压**（理想为两次）——罕见形态，记录不阻塞。
- **gate 线程死亡且报告层仍接管的异常窗口**：`gate_not_alive` 分支仍跳过回注
  （gate 死通常意味着映射关闭 → targets 为空 → 不接管；崩溃窗口概率极低）。
- **物理键盘同 usage 劫持（设计内，未修）**：报告层按 usage 清空无法区分设备，
  配置了动作的键（Home/TV/Menu…）其物理键盘同键会被映射动作接管——Home/TV 为
  方案 C 既定代价；Menu/方向/Enter 等高频键被接管后是否需要排除，属产品决策，
  待 Andy 拍板后再动（TODO 待办）。
- 真机验收 deferred：需重出包确认 ~ 键可输入、退出恢复、三键映射动作不受影响。
