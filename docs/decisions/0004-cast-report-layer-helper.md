# ADR 0004：Chromecast 遥控器接入报告层 Helper（复用 RC003 增强捕获框架）

- 状态：已接受（2026-11-10）
- 关联：ADR 0002（双轨注入可选 Helper）、ADR 0003（ARM64 载荷）、`Testing/WindowsChromecast.md`（厂商键通道与缓存错位全程证据）、CAST-12/CAST-13

## 背景

2026-11-10 的 HCI 抓包把 Chromecast Voice Remote（18D1:9450）按键在 Windows 上
"零到达"的根因链完整闭环：

1. 该遥控器对 Windows 主机**可以**走 HID（vibe-mote 用户群、A3 批次实测），但对
   本项目测试机**从不发送 HID 输入报告**，普通按键只走 Google 键事件服务
   `{D343BFC0-…}` 的特征通知（空中句柄 0x29，键码 = legacy report-1 位索引）。
2. 该测试机的 Windows GATT 属性表在键服务区与空中表错位（订阅落在 0x37，通知在
   0x29），删除设备、重启、深度清理（注册表 `BTHPORT\Parameters\Devices` 键）+
   全新配对后**重建的表仍错位**——`Uncached` 查询零发现流量，WinRT 无原始 ATT
   通道。ATVV（语音）/电池句柄全部一致，故语音链路一直正常。
3. 开源先例 **Tilkmilk/vibe-mote**（MIT，Windows 10/11 已发布产品）用与本仓库
   RC003 增强捕获**同构**的方案解决了同一只遥控器的按键读取：Frida 注入
   `WUDFHost.exe`，钩 `ntdll!NtDeviceIoControlFile` 过滤 `IOCTL 0x80018483`，
   从输出缓冲区抄走 3 字节报告 `[0x02, usage_lo, usage_hi]`，并把已映射键的
   usage **原地清零**抑制原生动作。独立佐证：knobs1 的 14 项 Windows 实验
   （vRemoter Windows 移植团队）结论一致——常规用户态路径全部无解。

## 决策

为 Chromecast 增加报告层 Helper（Google 版），**复用 RC003 增强捕获框架**：
同一个 `sayall-helper`（x64/arm64 双载荷）+ 同一注入/租约/计划任务机制，
agent（`rc003_agent.js`）扩展第二类报告：

- 现有键盘报告类：`out[0]==0x01`、9 字节、usage 槽位 3..8（RC003）。
- 新增消费页报告类：`out[0]==0x02`、**3 字节** `[0x02, usage_lo, usage_hi]`
  （Chromecast；usage 16 位小端，经既有
  `raw_input::chromecast_button_for_consumer_usage` 直接映射语义键）。
- 边沿照旧走 `{"type":"edge",...}` 上报；已映射键的 usage 清零沿用 vibe-mote
  验证过的"快照后清零"手法（与 RC003 的清键写法同源，安全门禁同款：IOCTL、
  长度、租约、白名单、清键上限逐条校验）。
- 语音键不经 HID（该型号语音键无 HID 报文，ATVV 专属），Google 版**不需要**
  synth/gate/release-hold 机制。

## 覆盖边界（必须写进产品文案）

该方案在"遥控器向 Windows 投递 HID 报文"的主机上生效。测试机这类**空中无 HID
流**的主机（厂商通道 + 属性表错位）上 Helper 计数恒零，不在本方案覆盖内——
这类主机的出路是未来评估的 ATT 级 Helper，或等系统栈修复。装了不生效的机器
必须能从诊断计数（`ioctl` 命中 / `edge` 上报）一眼分辨。

## 开源边界与署名

- 不复制 vibe-mote 代码；其"快照后清零抑制原生动作"作为**独立先例**登记进
  `ATTRIBUTION.md` 调研记录（与本仓库 2026-09-23 的 WUDF 写入实验互为佐证）。
- vRemoter 的 legacy 键码表沿用既有 MIT 署名（`THIRD_PARTY_NOTICES.md`）。

## 任务分解（CAST-13）

1. agent：`rc003_agent.js` 新增 0x02 报告类（观察 + 清零）+ `agent_logic_test.mjs`
   用例（含阳性对照）。
2. helper（`hardware/RC003/helper/src/main.rs`）：edge 消息带 `device=cast` 维度；
   目标下发与租约按设备维度隔离（两支遥控器可同宿主）。
3. 主程序：`rc003_task.rs` 泛化为按型号选 agent 目标集；`ChromecastInput` 新增
   `ReportTap` 来源接 `chromecast_button_for_consumer_usage`；`capture_support()`
   对 Chromecast 放开并注明覆盖边界；按键页/连接页文案。
4. 打包：双架构载荷不变（helper 单二进制）；`ATTRIBUTION.md` 登记 vibe-mote。
5. 验收：有 HID 流的真机 13 键矩阵 + 断连 fail-open + 哨兵键（清一个 Windows
   原生可用键证明清零生效）；无 HID 流主机验证计数恒零的诊断可读性。
