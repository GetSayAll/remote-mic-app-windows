# 新版 VB-CABLE（16 Ch）渲染端点初始化报 0x8889000A，且引导文案指向不存在的 CABLE Input

- 发现日期：2026-10-02（用户现场反馈，本地测试包 0.5.0 / main `6693a53`）
- 状态：已修复（应用侧重试 + 文案修正）；真机复测待操作人确认
- 影响范围：Windows 0.5.0；安装 VB-CABLE 3.3.1.7（渲染端点名为 `CABLE In 16 Ch` 的新版驱动）
  的机器；与遥控器型号无关（问题在音频输出段）
- 功能点：语音设备选择与 WASAPI 输出端点初始化（`crates/sayall-windows/src/audio.rs`）、
  连接页引导文案（`src/pages/ConnectionPage.vue`、`src/lib/bridge.ts`）
- 现象：在「语音设备」列表点选 `CABLE In 16 Ch (… VB-Audio Virtual Cable)` 后立刻报
  `初始化 16 kHz WASAPI 输出失败：Windows returned an error: 0x8889000A`；同一列表里的
  `扬声器 (… VB-Audio Virtual Cable)` 可以正常选择并推流。页面上另一处引导写着「这里选择
  CABLE Input」，而这台机器上根本没有 CABLE Input——旧驱动的 2 通道端点已是 NOTPRESENT
  残留（驱动被换成了 16 通道版本）。
- 复现条件：渲染端同时存在「扬声器 (… VB-Audio Virtual Cable)」与「CABLE In 16 Ch (…
  VB-Audio Virtual Cable)」两个候选、录音端为 `CABLE Output` 的机器上，应用已选中前者时
  改选后者。
- 正常预期：选中后进入「已就绪」，语音键推流可从 CABLE Output 录到（输入法上屏）。
- 证据：
  - 诊断日志 2026-10-02 15:45:46–15:48:31 UTC（本地 23:45–23:48，revision `6693a53`）：
    `action=restore` 打开旧选择成功 → `attempt_id=2`、`attempt_id=3` 两次
    `action=select … terminal_result=failed error_domain=wasapi error_code=open_failed
    reason=endpoint_format_or_state_unavailable`（0x8889000A）→ `attempt_id=4` 改选成功。
  - `examples/wasapi_probe.rs`（本机实测）：`CABLE In 16 Ch` 在设备空闲时用**完全相同的产品
    参数**（16 kHz 单声道 s16、共享、autoconvert、设备默认周期）可以打开，四种参数变体
    （含独占、48k/2ch、设备混音格式）也都能打开；失败只出现在另一端点被持有/引擎刚释放的
    状态下，且同端点共享模式多客户端（两个客户端同时开）始终成立。
  - `examples/cable_loopback_probe.rs`（本机实测）：两个渲染端点回环到 `CABLE Output` 都有
    信号（1 kHz 正弦，peak ≈ 11.4k / 11.4k），说明两者都是有效的语音输出端点。
  - 本机 VB-Audio 设备状态：3 个 MEDIA 设备实例（驱动 3.3.1.7），其中 2 个处于错误状态
    （问题代码 10），并残留 NOTPRESENT 的旧端点——属于驱动重复安装/半更新的现场状态。
- 根因：已确认的事实——产品参数与代码路径没有问题（探针在设备空闲时用同一参数四种变体
  全部成功）；失败码 `AUDCLNT_E_DEVICE_IN_USE` 出现在两种情形：①瞬态（同设备另一端点刚
  释放 / 引擎尚未就绪，先读一次混音格式预热后即可成功）；②持久（同一进程里持有该虚拟设备
  的另一个端点时，目标端点连续失败且 3 s 级重试无效，`examples/wasapi_probe.rs` 的 hold
  对照 3/3 复现）。仍未知的边界——这台机器上触发瞬时占用的确切持有者没有被捕获（驱动残留
  状态是可疑背景：3 个 MEDIA 设备实例里 2 个处于错误状态、问题代码 10，但仅凭推测不下结论）。
- 修复：
  1. `AudioSink::open` 对 0x8889000A 增加有界重试（档位 0/150/400/900/1600 ms，累计 3.05 s，
     远小于 IPC 请求超时 10 s），每次重试前先 `get_mixformat()` 预热端点引擎；失败消息带上
     端点名，便于用户与日志直接对应到具体设备。
  2. 用户点选的端点最终仍打不开时，自动兜底到**同一台虚拟声卡设备**的其它 CABLE 端点
     （判据 = 端点友好名括号里的设备描述一致，且都是 CABLE 候选；跨设备绝不兜底，否则输入法
     监听的 `CABLE Output` 收不到声音），成功后在日志记 `reason=fallback_sibling`、界面按
     实际使用的设备报文案（「已自动改用 …（… 暂时打不开）」）。
  3. 结构化日志补重试与兜底事件（`phase=retry` / `phase=fallback`）；按生产日志脱敏规则
     **不写入端点名称/ID**（沿用既有 `endpoint_kind` 分类），只有失败消息（界面可见）带端点名。
  4. 连接页引导不再硬编码 `CABLE Input`，改为「选择带『推荐』标记的 CABLE 设备」；推荐判据
     接受新版驱动名（`CABLE In 16 Ch`），仍排除 VB-CABLE A/B 与不带 CABLE 名的同设备端点。
  5. 文档同步：`docs/installation-and-configuration.md`、`docs/product-copy.md`。
  6. 取证工具入库：`examples/wasapi_probe.rs`（端点初始化矩阵 + hold 对照）、
     `examples/cable_loopback_probe.rs`（回环判定）。
- 验证：
  - `cargo test --release -p sayall-windows --lib`：242 passed / 0 failed / 12 ignored，含新增
    `only_endpoint_in_use_triggers_the_open_retry`、`open_retry_schedule_starts_immediate_and_
    stays_within_request_budget`、`same_device_fallback_only_accepts_the_sibling_description`。
  - `vitest run src/pages/ConnectionPage.test.ts src/lib/bridge.test.ts`：50 passed / 10 skipped，
    含新增「推荐 CABLE In 16 Ch、引导文案不再出现 CABLE Input」「兜底替换后按实际设备报文案」。
  - `vue-tsc --noEmit` passed；探针取证见上（`wasapi_probe`、`cable_loopback_probe` 本机 passed）。
  - **真机复测 deferred**：需操作人在本地测试包上重新点选 `CABLE In 16 Ch`——预期直接成功，
    或日志出现 `phase=retry` / `reason=fallback_sibling` 后成功。
- 隐私检查：未含设备地址、端点 GUID、个人路径、语音内容或凭据；端点名称为 VB-Audio 官方
  通用名称。
