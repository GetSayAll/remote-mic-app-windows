# 临时麦克风切换与音频生命周期验收

本页定义当前上游集成的 F2、F3、P2、P3、P4 验收契约。基线为 `3e11586f7e211df74586baa52b3e1ea7c77a5d56`，移植来源为本地 `a3ff225b37fd15e0f60b6b07afb0e27426e6859e`；后续修改也须以实际被测候选为准。原分支的历史实机结果不证明此次集成通过。

## 预期与失败判据

| 项目 | 正常、边界与失败路径 |
| --- | --- |
| F2：按住期间临时切换麦克风 | 默认关闭。显式开启后，先取消旧语音并确认可选报告合成已释放，准备原选声音写入端，再把当前用户 Capture 的 Console、Multimedia、Communications 三角色切换到目标并读回，最后发送目标快捷键 DOWN。UP、取消、断连、睡眠、退出先释放快捷键，再结束音频与恢复本轮改动。配置锁内不得启动新语音；失败保留旧有效配置和合成目标。默认关闭仍保留上游可选语音合成、输入工具选择与手动写入端。 |
| F2：让出、恢复与持久记录 | 任一角色出现外部改选，本次整体让出，不覆盖其他角色。Console/Multimedia 联动按组确认，原两角色不同则写前拒绝；每个 setter 前保存原值、期望值及组状态。崩溃残留通过可见“恢复 / 保留”处理，不自动调用恢复 setter；若所有角色已精确回到原向量，仅清理记录。设备缺失、记录失败、超时或旧值不再可验证，保持恢复待处理且不得启动语音。 |
| F3：同一条 VB-CABLE 配对 | 用设备属性、拓扑适配器 PnP 身份和标准 pin 描述配对 Capture / Render。保留有效的已选同线写入端（含 16 通道端）；自动选择标准写入端。缺失、身份变化、元数据不全、重复 pin、跨适配器或不同音频线均拒绝猜选。占用重试保留上游原预算，其他写入端兜底也必须同适配器且 pin 唯一。 |
| P2：启动突发音频 | 音频按 32,000 个样本总预算接收，包括待写数据与正在写入的数据。控制通道满时合并唤醒，不能因消息数达到 32 而丢弃预算内音频；回归包含消费者暂停时 40 个 240 样本包。超预算失败需唤醒消费线程并统一中止，旧代次与关闭后的迟到音频不能进入新会话。 |
| P3：原端点重建与准备取消 | WASAPI 失败后保留原 ID / 名称，下次独立语音请求才重建该精确端点，不重启旧流、不猜换其他设备。端点缺失或身份变更失败关闭。CONTROL 回调入口捕获取消代次；buffer 读取、日志、设备准备或回复期间发生 UP、断连、睡眠或配置变更，迟到 START 不得产生快捷键 DOWN。清理失败退休旧资源后再恢复。 |
| P4：物理断连 | 回调记录断连，工作线程在阻塞清理前公布非 Ready / Streaming 状态，清空旧能力和音频状态。已知物理链路断开时跳过音频、控制和电量的远端 CCCD 写入，仍移除本地回调、关闭 GATT service / device。关闭失败保留真实资源状态并在重连周期重试，不覆盖新资源或伪报已释放。 |

上游的逐批增益、输入法激活确认、硬件信号仿真和可选语音合成继续保留。F2 开启时才切换到设备准备后的 SendInput 路径；切换来源必须先结束当前语音、取得报告路径释放确认，不能双写快捷键。BLE 连接建立 / 重连期间的原生 F5 保护也须保留：就绪、失败、挂起或主动断开时关闭；释放沿始终按其按下沿是否全部吞下配对，不能因保护状态变化留下粘键。

## 纯自动化入口与日志

执行 `cargo test --workspace`、`cargo check --workspace`、`cargo fmt --all -- --check`；平台集成追加 `cargo check -p sayall-windows-app --features runtime-simulation`。可单独运行 `cargo test -p sayall-windows --lib`。默认不执行 `--ignored`：真实拓扑、WASAPI 静音及端点恢复实验仍需单独授权。路由 setter 回归使用 Fake 后端；纯测试不修改 Windows 默认设备、不采集语音。

关联事件包括 `voice_route action=initialize|configure|hotkey`、`voice_session action=cancel`、`capture_input action=role_vector|prepare_io|restore|recovery_reconcile`、`audio_route action=pair`、`audio_startup`、`audio_endpoint action=rebuild`、`voice_prepare action=invalidate`、`ble_disconnect phase=callback|published|cleanup_completed`、`ble_cleanup phase=remote_cccd` 和 `app_exit ble_session_shutdown`。读取 `terminal_result` 与最终状态，不能以排队、API 返回或日志入口证明目标收音成功。日志只留代次、阶段、角色 mask、分类、计数、耗时和错误码，不记录端点身份、路径或语音正文。

## 当前验证范围

2026-10-05 集成候选已执行 `scripts/ci-preflight.ps1`：7/7 passed（前端依赖、测试、构建、Rust 格式、workspace 测试/check、runtime-simulation 编译检查）。Windows 平台 416 项通过、18 项依赖实际环境的测试 ignored；相关音频事务与入口取消回归均通过。首次红阶段受共享模块合并标记阻断，不能算音频行为的失败复现。移植保留原回归，并增加配置交接顺序、取消确认、同适配器兜底和 CONTROL 读取期间取消的测试。

RC001 与 RC003 各自的真实收音、三角色切换 / 恢复、外部改选、快按 / 连续会话、冷态首用、端点失效后恢复、断连、睡眠、崩溃恢复选择及可选合成来源切换均为 `deferred`。本轮不运行默认设备 setter、声卡实验或安装验收。实际 WebView/IPC 仿真仍待 CI：本机正式版正在运行，现有仿真与其共用单实例守卫，不把退出仿真或唤起正式版误作通过。

受限 setter 使用边界与来源归 [ATTRIBUTION](../ATTRIBUTION.md)，任务状态归 [TODO](../TODO.md)。Windows 没有此 setter 的 CAS，也不能从通知获知操作方；外部同值操作无法识别，第三方显式固定设备不保证跟随默认麦克风。真机验收须观察实际角色读回、成对键态和目标收音，不能扩大纯测试结论。
