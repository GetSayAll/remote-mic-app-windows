# RC003 返回 / 音量按键诊断工具

## v3：逐段对照检查（当前版本）

v2 首段上方向键 Raw=6、LL=0，末段 Raw=6、LL=6；目标键两通道均为零。
因此 v2 的低级钩子阴性结果不能用来断言三键不可见，异常原因尚未确定。

v3 只检查 LL / Raw Input，不重复订阅已在 v2 检查过的 GATT 服务。
保持工具前台，不操作其他键盘；每个目标键测试前后，都要求按一次遥控器上键并松开。
只有两个通道都收到完整按下释放才继续；15 秒未通过或失去前台立即停止，记录 validation_failed，不能解释为硬件不支持。
三个目标键窗口各 6 秒。时间取决于对照键操作速度，通常约半分钟；以下 v2/v1 的固定时长说明仅用于追溯。
编译、自检（包含 Raw 有事件但 LL 缺失、缺释放沿的拒绝用例）与 LL/Raw 启停通过；实体逐段对照待验证。

## v2：用户确认本机为 RC001

2026-09-13 首轮日志的标准 Model Number 返回 RC001，用户确认实体也是 RC001。
两轮上键均为 3 DOWN + 3 UP；返回/音量±各窗口 Raw Input 为零，四条 GATT 通知均为电量。
不能将仓库旧注释“RC001 可收到 VK 0xFF”视为本机已经验证。

v2 改为 **30 秒**，五段各 **6 秒**：上键三次、返回三次、音量加三次、音量减三次、上键三次。
新增短期 `WH_KEYBOARD_LL` 观察，与 Raw Input/GATT 并行。只记录 0xFF、音量、BrowserBack、上键、Backspace、Escape 候选，不记录字母/数字等普通输入；所有边沿调用 CallNextHookEx 放行。
LL 不提供设备身份，日志明确标为 unattributed；测试期间不要操作普通键盘或其他遥控器。
收到 LL 候选也不等于已实现可靠映射；需核对扫描码、按下释放及对照键。
下面 64 秒八段描述仅对应 v1，v2 以本节和界面为准。目录名暂保留以追溯 v1。

v2 编译、自检、2 秒 LL 注册/Raw 消息循环/LL 注销通过；实际物理按键事件待用户复测。

这是第一阶段的独立采集工具，不是已修复映射的 SayAll 安装包。
使用 Windows 自带 .NET Framework 4.x，x64；无需安装 Rust、驱动或管理员权限。

## 使用

1. 从 SayAll 托盘菜单正常退出程序。工具检测到它仍运行时会拒绝开始，不会强杀。
2. 保持 RC003 已配对，只连接一台小米遥控器。
3. 双击 `RC003-Diagnostic.exe`。选择 RC003，点击“开始 64 秒逐键采集”。
4. 等待设备枚举与 GATT 订阅完成（总准备阶段最多约 90 秒）。跟随窗口中的倒计时逐键测试；不要提前按键，不要按语音键。
5. 八段各 8 秒：上方向键三次、返回三次、音量加三次、音量减三次、返回按住两秒后松开、音量加按住两秒后松开、音量减按住两秒后松开、上方向键三次。
6. 等工具显示清理完成。日志保存在程序旁 `captures/rc003-日期-随机值.jsonl`。把此文件交给开发者分析，然后可以重新打开 SayAll。

可以点击“停止并清理”中止；清理通常即时完成，蓝牙 API 超时情况下约需额外 10 秒。
按键原有 Windows 效果仍可能发生，因为本工具只观察，不吞键或注入按键。
没有匹配到遥控器时请保留这一结果，不要用耳机等其他蓝牙设备代替。

## 采集内容与边界

- SetupAPI 枚举 VID 2717 / PID 32B8 的全部当前 HID 接口；使用父设备关系限定 GATT 目标，地址只在内存中使用。
- 以零读写权限打开 HID 接口，读取 `HidD_GetPreparsedData` / `HidP_GetCaps` 与按钮/数值能力信息。不声称零权限句柄可以读取键盘报告。
- Raw Input 同时注册键盘、Consumer Control 及描述符发现的其他集合；只记录匹配小米 VID/PID 的事件。不会保存普通键盘输入或设备路径。
- HID 报告按实际长度拆分，并用设备自身的 preparsed data 调用 `HidP_GetUsagesEx`。原始报告最多保留前 256 字节，截断会明确标记。此工具不改变产品中的固定报文解码器。
- GATT 使用 `DataReader` 复制通知；按属性选择 Notify 或 Indicate；记录服务/特征状态、属性、句柄、标准 HID Report Map 和 Report Reference。
- 先读取 CCCD 原值，先挂回调再订阅，退出时移除回调、尝试恢复原 CCCD、释放服务和设备。恢复失败会记录并提示。
- 不写未知厂商指令，不写 ATVV Transmit，不订阅已知 ATVV AUDIO 特征。未知厂商通知仍可能包含未知数据，因此测试期间不要使用语音功能。日志仅保存在本机，不自动上传，单个日志上限约 10 MiB。
- 单次异步调用约 10 秒超时。GATT 初始化报错时仍尽可能完成 Raw Input 采集；不能把访问拒绝或超时当作“硬件没有事件”。
- 每段的计数仅表示时间窗口内的事件；不能单凭通知计数认定某个键已经识别。需要对照原始字节、DOWN/UP 和前后对照键。

## 本机验证（2026-09-13）

- passed：Framework C# 编译；x64 结构大小；报文多包拆分、短包/溢出拒绝；Notify/Indicate 选择；真实 WinRT IBuffer 字节往返。
- passed：SetupAPI/HID 实际枚举，发现一个匹配 HID 接口和一个 Raw Input 键盘接口；HID 最大输入 121 字节，输入能力包含 Page 7 / Report ID 1 和 Page FF00 / Report ID 6、7、8。这不证明三个目标键已经发送。
- passed：父设备关联后的蓝牙目标枚举，匹配一个遥控器；2 秒 Raw Input 注册/消息循环/清理测试正常退出；本轮无人按键，事件数 0，不构成按键接收验证。
- passed：WinForms 界面渲染检查。
- failed then fixed：早期 DEVNOTIFY 回调重复注册，监听测试未按时退出；已改为只注册新增集合，修复后的 2 秒测试正常退出。测试启动器额外设置 10 秒进程超时保护。
- 环境发现：本机 WinRT DeviceInformation.FindAllAsync 返回 0x80070002；设备列表改用公开 SetupAPI 父设备关联，不依赖该调用。
- deferred：真实按键 DOWN/UP、GATT 订阅及恢复、逐键报告归因、取消后的实体设备行为和恢复 SayAll 的端到端检查。需要用户按遥控器，尚未宣称支持映射。
- Rust 文件没有修改；本机没有 cargo/rustfmt，因此未运行 cargo fmt 或 Rust 工作区测试。

## 源码与复现

`Diagnostic.cs` 为独立诊断实现，`Compile.ps1` 调用 Windows 自带 Framework C# 编译器。

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\Compile.ps1
.\RC003-Diagnostic.exe --self-test
.\RC003-Diagnostic.exe --inventory inventory.jsonl
.\RC003-Diagnostic.exe --paired-smoke paired.jsonl
.\RC003-Diagnostic.exe --capture-smoke capture.jsonl
```

命令行测试通过退出码报告结果；`--capture-smoke` 监听约 2 秒，不注入测试按键。
正式采集用无参数 GUI，必须正常退出 SayAll 以避免钩子/会话干扰。

## 后续判据

优先查看两段 `control_up` 是否收到键盘事件，再对照目标键窗口的 `keyboard`、`hid_report`、`hid_usages`、`gatt_notify`。
如果只有 GATT API 错误，说明对应路径未测通；如果只有对照键事件而目标键持续无事件，再考虑更低层 HID 报告采集。
收到可重复区分的三个键及其释放事件后，才改产品的前端禁用集合、后端配置过滤与输入适配器。

源码基线：`1086c1904ac9afe4adc8f3ff7d8540be1c83bec6`。许可证沿用项目 GPL-3.0-only。
