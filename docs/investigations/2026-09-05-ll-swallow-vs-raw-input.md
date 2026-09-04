# 调查：LL 钩子吞键对 Raw Input 交付的影响（按键映射门控架构依据）

日期：2026-09-05
方法：`crates/sayall-windows/examples/swallow_probe.rs`（v1，单线程）与 `swallow_probe2.rs`（v2，钩子线程与 Raw Input 线程分离，与生产架构 key_suppressor 同构）
证据：探针两轮运行输出（见下），注入 Enter tap 对照"放行阶段/吞键阶段"。

## 结论（passed，两轮一致）

1. **LL 钩子（WH_KEYBOARD_LL）返回 1 吞掉的键盘事件，Raw Input（RIDEV_INPUTSINK, WM_INPUT）不会观察到同一事件**——即使 Raw Input 窗口位于独立线程。
   - v1 单线程：放行阶段 raw DOWN=3/UP=3；吞键阶段 raw DOWN=0/UP=0。
   - v2 双线程：放行阶段 raw DOWN=3/UP=3；吞键阶段 raw DOWN=0/UP=0，钩子全程 DOWN=6/UP=6。
2. 推论：RIT 先同步调用 LL 钩子，事件被吞则不再向 Raw Input 注册窗口投递 WM_INPUT；这与 key_suppressor.rs 的"60ms 有界等待期间 Raw Input 归因线程可刷新武装"设计自洽——归因信号只能来自**尚未被吞的先前事件**（如首个泄漏沿）或**独立管线**（HID 报文、BLE 会话信号）。
3. 推论（架构决策）：按键映射门控不能依赖"Raw Input 监听器观察被吞事件"。语义按键事件流必须由双源合并驱动：
   - 监听器线程：HID 报文（usage pair，独立管线，不受键盘 LL 钩子影响）+ 未被吞的键盘事件；
   - 门控钩子线程：被吞键盘事件的边沿（press/release）。
   两源汇入引擎线程的 ButtonStateMerger（并集去重，双源语义本就为此设计）。
4. 吞键归因：HID 报文在监听器线程到达时武装对应按键的键盘候选（VK/make），LL 钩子对候选键做 60ms 有界等待（key_suppressor 同款）后裁决；VK 0xFF 族（厂商键，物理键盘不产生）可免武装直接归因。

## 边界

- 未在 RC001/RC003 真机上验证"HID 报文与键盘事件到达钩子的相对时延"（需实体按键）；60ms 有界等待 + 250ms 武装宽限沿用 key_suppressor 实证参数，真机复验列入验收。
- 本实验用注入事件（LLKHF_INJECTED 标记）驱动；注入与硬件事件走同一 RIT 投递路径，标志只影响来源标记，不影响吞键/交付语义。

## 运行方式

```
cargo run -p sayall-windows --example swallow_probe
cargo run -p sayall-windows --example swallow_probe2
```
