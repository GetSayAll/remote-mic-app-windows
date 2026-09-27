# 电量显示不准确、充电后不更新(Windows 缓存陈旧)

- 发现日期:2026-09-27
- 状态:已修复(代码层,2026-09-27 按方案 A 实现并经自动化验证;充电实时性、断连/睡眠与 RC001/RC003 真机验收 `deferred`)
- 影响范围:Windows 版当前全部已发布版本;RC001/RC003 走同一显示链路,均受影响;不涉及第三方工具
- 功能点:连接页电量显示(`BatteryIndicator.vue` / `crates/sayall-windows/src/battery.rs` / `ble.rs` BatteryMonitor)
- 现象:用户反馈页面电量不准确;充电后电量长时间不更新("不及时充电");"状态也可能有问题"(具体指代待用户补充)
- 复现条件:正常连接使用;充电前后观察页面电量
- 正常预期:显示值接近遥控器真实电量,充电后应较快跟随更新

## 证据

数据链路(代码可确认,无断裂,瓶颈在数据源):

1. `battery.rs:81` `read_cached_battery` 读 Windows 设备属性缓存
   (`CM_Get_DevNode_PropertyW`,键 `{49cd1f76-…}10` 与 `{104ea319-…}2`);
2. `battery.rs:13` `REFRESH_INTERVAL = 60s`,后台线程每 60 s 读一次该缓存;
3. `ble.rs:915` BatteryRead 写入 worker state(纪元 + phase 校验正常);
4. `ConnectionPage.vue:442` 前端每 1 s 轮询快照。前端拉取不是瓶颈。

关键事实:

- 设备端**支持实时电量推送**:`hardware/RC003/evidence/gatt-probe-listen1.log` 实测
  RC003 电量特征 `0x2A19` props=`read|notify`,订阅后 0.569 s 收到 NOTIFY 值 `0x1E`(30%)。
  应用当前**未订阅**该特征。
- Windows 属性缓存只在 Windows 自身刷新时机更新(典型:设备重连/重新枚举),
  不保证跟随设备实时上报;应用 60 s 轮询读到的因此长期是同一份旧值。
- `TODO.md`(电量条目)与 `ATTRIBUTION.md` 记录了当时的产品决策:
  "不额外进行 GATT 操作 / 不另开 BLE/GATT 会话"——实时通知路径被该约束排除。
- `Testing/WindowsBattery.md`:"当前持续更新……仍为 deferred"——本缺口早已登记,现被用户反馈证实。

## 根因

- 主因:显示值 = Windows OS 缓存值,缓存刷新时机不受应用控制,充电后的新电量不进入缓存;
  设备端实时通知能力存在但被既有约束排除。
- 次因:应用 60 s 轮询间隔,即使缓存更新也最多再叠加 60 s 延迟。
- "状态"问题候选解释(待用户确认具体指代):
  ① 低电量告警(≤20% 变色)基于陈旧值,可能误报/漏报;
  ② 重连/睡眠恢复期间 phase 不接受电量,UI 显示"电量未知",恢复后需等下一轮读取;
  ③ 应用侧从无"充电中"状态数据源,该状态不可能正确显示。

## 修复(2026-09-27,方案 A 已实现)

- 连接建立后在**既有 GATT 会话内**订阅 Battery Service `0x180F`/`0x2A19` notify
  (`BleSession::setup_battery_notify`,best-effort:任何失败清理自身对象并回退,不进
  `connect_stage` 硬失败链,不影响语音主路径);订阅成功后立即 `ReadValueAsync` 一次
  保证 UI 立即有初始值;通知经纪元 + phase 校验复用 `apply_reading` 写入快照。
- 订阅失败或设备无 BAS 时回退为原 60 秒 Windows 缓存轮询(`use_cache_monitor` 决策,
  `BatteryMonitor` 保留为兜底);cleanup 中退订、disable CCCD、Close battery service,
  与 audio/control 同规格成对释放。
- 诊断日志:`remote_battery phase=gatt_subscribe_start/gatt_subscribe/initial_read/notify`,
  一次日志拉取可定位电量链路任一环节。
- 前端悬停文案由"Windows 缓存"改为"随遥控器上报更新";`TODO.md`、
  `Testing/WindowsBattery.md`、`ATTRIBUTION.md` 已同步修订(原"不额外 GATT 操作"约束
  正式废止,记录于 ATTRIBUTION)。
- 自动化验证:`cargo test --workspace`、`cargo fmt`、`cargo check`(含 runtime-simulation)、
  前端 vitest、`vue-tsc`、`vite build` 通过;真机验收 `deferred`。

## 验证

- `deferred`:需真机充电对照——充电前后记录应用日志 `remote_battery level=…` 与
  Windows 设置页电量、遥控器真实状态,RC001/RC003 分别执行;修复后须覆盖
  冷/闲置后首用与断连恢复场景(与按键时序无关,但 phase 清零/恢复路径须回归)。

## 隐私检查

未包含设备身份、蓝牙地址、个人路径、语音内容或凭据。
