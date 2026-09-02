# Windows RC001 / RC003 Preview 测试手册

## 适用范围

- 仓库：`GetSayAll/remote-mic-app-windows`
- 平台：Windows 10 1809+ / Windows 11 x64
- 硬件 A：小米蓝牙遥控器 2 / RC001
- 硬件 B：小米蓝牙遥控器 2 Pro / RC003
- 当前状态：WinRT BLE / ATVV / PCM / WASAPI、设备名与 2A24 型号识别、端点持久化、退避重连、睡眠恢复、Raw Input、本机使用统计和 Windows 10 1809 双层版本门禁代码路径已实现；Windows、RC001、RC003、VB-CABLE 真机验收均为 deferred

## 测试前准备

1. 记录 Commit、版本、Windows 版本和电脑蓝牙适配器。
2. 分别准备真实 RC001 和 RC003，并确认两者都可以在 Windows 蓝牙设置中完成配对。不得用其中一种型号的结果代替另一种。
3. 安装待测构建。
4. 若测试虚拟麦克风，单独安装 VB-CABLE，并重启 Windows。
5. 准备记事本和至少一个真实语音输入应用。
6. 保存测试开始前的应用日志。

## 用例零：Windows 版本门禁

1. 在低于 Windows 10 1809（build 17763）的隔离虚拟机运行 NSIS 安装器。
2. 确认安装器显示最低版本提示并退出，应用文件、快捷方式和卸载条目均未创建；WebView2 bootstrapper 是否已发生动作需单独记录。
3. 将已解压的应用 exe 直接复制到同一虚拟机并运行。
4. 在 Windows 10 1809 x64 和当前 Windows 11 x64 分别安装并启动同一候选。

预期：低于 build 17763 时，安装器在复制应用文件前以退出码 1633 失败关闭；直接运行 exe 时在创建 WebView、BLE、WASAPI 或 Raw Input 资源前显示原生中英文提示并退出。Windows 10 1809 和更高版本不触发版本拒绝，继续进入正常安装与启动流程。

失败判定：只在文档声明最低版本；低版本仍复制应用文件或启动后台平台线程；直接运行 exe 崩溃、静默退出或进入主界面；Windows 10 1809 被误拒绝；把 CI 编译或纯逻辑测试表述为已完成上述虚拟机/真机验收。

## 用例一：首次连接

1. 只配对 RC001，启动无线麦并打开“连接与语音”。
2. 点击扫描，选择 RC001 并连接。
3. 确认界面显示“小米蓝牙遥控器 2（RC001）”。
4. 断开并移除 RC001 配对，改为只配对 RC003，重复扫描和连接。
5. 确认界面显示“小米蓝牙遥控器 2 Pro（RC003）”。
6. 对于名称只能归入白名单、不能确定型号的设备，确认连接时尝试读取标准 Model Number（2A24）；读取失败时显示“型号待设备确认”，但不因此阻断 ATVV。

预期：只显示名称在批准白名单内的 RC001/RC003 候选；零个或多个候选时不猜测。型号来自精确设备名或标准 2A24，不使用两者共用的 HID VID/PID 猜测。页面依次反映连接、特征发现、能力确认和就绪状态；只有收到有效 16 kHz ATVV 能力后才显示“BLE / ATVV 已就绪”。

失败判定：RC001 被标成 RC003，或 RC003 被标成 RC001；仅凭共用 HID VID/PID 判定型号；2A24 不可读时拒绝原本可用的 ATVV；仅进程运行或发现设备名称就显示 ATVV 就绪；把 ATVV 就绪显示成系统麦克风可用；连接失败后需要清配置或重装。

## 用例二：首次语音会话

1. 确认连接页显示“BLE / ATVV 已就绪”。
2. 按住当前型号的语音键并说一整句。
3. 观察页面进入“正在接收遥控器语音”，确认解码采样数增加。
4. 释放语音键。
5. 分别使用 RC001 和 RC003 重复步骤 1–4，两者都必须在第一次会话成功。

预期：第一次会话即完成一组 `STREAM_START → AUDIO → STREAM_STOP`；会话代次只增加一次；解码采样数增加；释放后回到 ATVV 就绪。WASAPI 完成前不要求文本框收到语音。

失败判定：第二次或第三次才开始收到采样、重复启动、释放后仍显示流式接收、旧连接的断开或通知结束了新连接。

## 用例三：极速和连续会话

1. 连续执行 20 次快速按下/释放。
2. 连续执行 20 次 2～5 秒语音。
3. 在上一段文字刚完成时立即开始下一段。

预期：每次按下和释放严格配对，没有卡键、重复音频、跨会话尾音，旧连接通知和上一代停止事件不能结束新连接或新会话。

## 用例四：断开和恢复

1. 建立 ATVV 就绪连接后，让遥控器断电或离开蓝牙范围，记录重连阶段和每次尝试间隔。
2. 保持设备不可用至少 70 秒，确认间隔按约 2、4、8、16、30、30 秒封顶，而不是忙循环。
3. 让遥控器恢复可用，确认一次成功能力协商后重连计数归零，第一次语音即可使用。
4. 点击“断开”，保持应用运行至少 35 秒，确认本次运行不再自动重连；重新选择设备后恢复自动重连。
5. ATVV 就绪时让 Windows 睡眠再唤醒，确认页面先显示睡眠释放，再重新经历连接、发现、能力确认。
6. 关闭再打开 Windows 蓝牙，重复步骤 1～3。
7. 应用退出再启动，确认自动恢复上次明确选择的遥控器及其已识别型号；若首次失败，继续按退避策略重试。
8. 分别使用 RC001 和 RC003 完整执行本用例。

预期：每次失败或睡眠都先释放通知、GATT service、device、ATVV pipeline 和活动音频；旧会话通知不会改变新会话；主动断开不会在本次运行反弹连接；恢复后第一次语音正常，不要求清配置。

失败判定：无间隔忙重试、清理失败后覆盖式创建新代次、主动断开后自动反弹、睡眠后仍显示旧会话就绪、恢复后需要第二次或第三次语音才成功。

## 用例五：音频端点

1. 不安装 VB-CABLE，点击“读取输出端点”，确认不会伪造 CABLE Input。
2. 安装 VB-CABLE 但不选择端点，按下语音键。
3. 明确选择 CABLE Input，确认页面显示“WASAPI 已就绪”。
4. 完成一次语音，观察“写入”“排空”阶段和已提交采样数。
5. 在 CABLE Output 侧录制或监听，确认首尾完整且没有重复。
6. 语音期间尝试切换端点，然后在空闲时切换。
7. 退出并重新启动无线麦，确认仍恢复同一个 endpoint ID 和名称，且没有改用系统默认输出。
8. 禁用或移除当前端点后重新启动，再次开始语音。
9. 让同一个 endpoint ID 的显示名称发生变化后重新启动；如果驱动无法稳定复现该状态，记录为 deferred，不得用另一个 ID 替代。

预期：缺少或未选择端点时语音失败关闭但 BLE 连接保持；语音期间禁止切换；正确端点时 16 kHz PCM 进入 CABLE Input，并可从 CABLE Output 收到；`STREAM_STOP` 后等待 WASAPI padding 清零再结束会话；重启只恢复稳定 ID 与原名称都一致的端点；端点缺失或名称变化时保留原选择用于提示，但 WASAPI 不进入就绪，也不退回系统默认输出。应用不读取或修改 Windows 默认输入、输出。

失败判定：自动使用默认扬声器、只按名称匹配导致误选、端点 ID 或名称变化后仍自动恢复、队列无限增长、停止时直接丢弃尾音、端点失败后仍显示系统语音可用。

## 用例六：可靠按键

1. 在“按键”页面点击“启动监听”。
2. 确认页面只匹配一个 RC001/RC003 共用设备族的 Raw Input 路径；零个或多个匹配时记录错误并停止，不得自动选择第一个。
3. 逐个测试 Windows 公共 API 能稳定收到的按键：按下、释放、持续按住、系统重复、快速连续。
4. 对同一实体按键核对原始事件数和语义边沿数，确认 Keyboard 与 HID 同时报告时不会产生两次语义按下。
5. 按住普通按键时点击“停止监听”，再重新启动监听。
6. 按住并释放语音键，确认普通按键最近边沿和语义边沿计数不因语音键变化。
7. 分别使用 RC001 和 RC003 完整执行步骤 1–6，单独记录两种型号的 Keyboard、HID-only 或双路径报告形态。

预期：监听使用隐藏 message-only window 和 `RIDEV_INPUTSINK`；每个事件都按启动时唯一选中的完整设备路径过滤；一次实体动作只产生一次语义按下和一次语义释放；重复 key-down 不重复触发；停止时释放仍处于按下状态的普通键；线程未能停止时页面必须显示失败，不能伪装为已关闭；普通键盘不被映射；语音键不进入普通动作路径。

失败判定：零匹配仍显示就绪、多个匹配时取第一个、其他键盘改变最近按键、一次按住产生多次语义按下、Keyboard/HID 双路径重复触发、停止后残留按下状态、语音键进入普通按键统计或动作路径。

## 用例七：安装升级

1. 安装旧候选并修改配置。
2. 安装新候选。
3. 检查安装条目、配置、映射、统计和日志。
4. 卸载应用。

预期：升级只保留一个安装条目并保留用户数据；卸载范围与界面说明一致。

## 用例八：映射保存与 SendInput

1. 在“按键”页面选择一个普通按键，并从页面内按钮网格选择快捷键预设。
2. 点击“保存映射”，退出并重新启动应用，确认映射从 `button-mappings.json` 恢复。
3. 在记事本中点击“测试当前快捷键”，分别测试单键、`Ctrl+C` / `Ctrl+V` 和 `Win+D`。
4. 测试左右修饰键、方向键、Home/Delete 等扩展键，并核对按下顺序和逆序释放。
5. 通过自动化故障注入模拟 SendInput 只提交部分事件以及调用后抛出未知错误。

预期：一个快捷键的全部按下和逆序释放通过一次 SendInput 批量调用提交；部分提交只释放可能已按下或尚未释放的键；未知交付状态逐键 best-effort 释放全部可能按下的键并报告失败；保存后无需重启即可用于显式测试。真实 RC001/RC003 边沿当前都不会自动执行映射。

失败判定：逐键分别调用 SendInput、按键释放顺序与按下相同、部分失败后修饰键卡住、无效或超过四键的映射被保存、测试失败仍显示成功、未完成真机来源确认就自动把 Raw Input 边沿注入前台应用。

## 用例九：NSIS Preview 安装包

1. 从对应 Windows CI Run 下载名称包含精确 source commit 的 `sayall-windows-unsigned-preview-*` artifact。
2. 确认 artifact 只包含一个 `*-setup.exe`、`SHA256SUMS.txt` 和 `build-metadata.json`。
3. 重新计算安装器 SHA-256，并与两份记录逐字匹配。
4. 确认 metadata 中 productName、identifier、publisher、版本和 source commit 与仓库一致，且 `signatureStatus` 为 `NotSigned`、`distributionStatus` 为 `unsigned-ci-preview-not-for-public-release`。
5. 确认 Windows CI 的 `Test silent current-user install and uninstall` 步骤通过：`/S` 安装后只有一个 HKCU 卸载条目、安装目录位于当前用户 `LOCALAPPDATA`、开始菜单只有一个入口，已安装进程持续运行 8 秒后由测试关闭。
6. 确认同一步骤使用注册表中的真实卸载命令执行 `/S` 卸载，程序目录、开始菜单和卸载条目均移除；位于 Tauri `app_config_dir` 的测试标记仍保留，证明默认静默卸载不会删除当前用户设置目录。
7. 仍需在隔离的 Windows 测试用户中手动运行安装器，检查可见语言选择、安装界面、开始菜单启动、退出、再次启动和卸载界面；不得用 CI 进程存活代替可见 UI 验收。

预期：CI 从干净提交构建唯一 NSIS 包；校验脚本拒绝错误产品身份、多个安装器、异常小文件、签名状态不符合当前 CI 边界或摘要不一致。静默生命周期必须证明当前用户安装、使用 Tauri updater 兼容的 `/S /UPDATE` 升级路径在等待旧卸载器退出并经过有界稳定窗口后收敛、单一卸载条目、单一开始菜单入口、进程不立即崩溃、程序文件完整卸载和设置目录默认保留。安装器无需管理员权限，不允许用旧版本覆盖新版本。

失败判定：artifact 无法绑定精确 commit、存在多个候选安装器、SHA-256 不一致、CI 包意外带未知签名、静默安装非零退出、安装到当前用户目录之外、卸载条目或开始菜单入口不唯一、应用进程立即退出、静默卸载残留程序文件或误删设置标记、安装器要求全局管理员权限、把未签名 artifact 当作公开发布包。

当前自动化状态：第 2 项生命周期矩阵此前失败在降级用例：普通 `/S` 静默页不会执行 Tauri 的版本比较，导致 `allowDowngrades=false` 未阻止 predecessor。测试现改用 `/S /P` passive 路径，待 Windows CI 重新验证；在矩阵通过前不得进入 Authenticode 发布演练。

## 用例十：隐私安全诊断摘要

1. 依次在未连接、连接中、ATVV 就绪、语音流式、排空、断开和失败状态打开“权限”页面。
2. 点击“生成摘要”，确认页面内 JSON 的阶段、能力、代次和计数与同一时刻的连接、音频、Raw Input 和 SendInput 状态一致。
3. 搜索摘要，确认不存在真实设备 ID、蓝牙地址、完整 HID 路径、遥控器名称、音频端点 ID/名称、错误原文、窗口标题、语音内容或用户文本。
4. 点击“复制摘要”，粘贴到本地记事本，确认内容与页面内可见 JSON 逐字一致。
5. 在 900 × 620 和 1080 × 720 窗口重复生成，确认权限状态、按钮、摘要和滚动均可访问，没有横向裁切。

预期：摘要由当前运行快照即时生成，不读取其他 App 或系统私有数据；敏感字段在 Rust 诊断结构生成阶段即被排除。复制失败必须显示错误，不能显示成功或静默上传。

失败判定：摘要包含任何设备或用户身份、路径、端点名称或错误原文；状态与当前快照不一致；浏览器预览被描述为 Windows 可用；未点击时自动读取或复制；剪贴板写入失败仍提示成功；页面出现横向溢出或中文小于 12pt。

## 用例十一：本机使用统计

1. 记录测试前“统计”页面今日、本周和全部的按键次数、语音次数与语音时长。
2. 启动 Raw Input，执行 10 次普通按键完整按下/释放；包含持续按住产生的系统重复，以及 Keyboard/HID 双路径同时报告的按键。
3. 按住并释放语音键 3 次，确认语音键不增加普通按键次数。
4. 完成 2 次包含真实音频、且 WASAPI 排空成功的语音会话；另制造 1 次端点失败或断连中断的语音会话。
5. 保持统计页打开，确认数据在约 1 秒内更新；依次切换今日、本周和全部，并核对最近 7 天对应本机日期。
6. 退出并重新启动应用，确认统计不丢失、不重复累计；跨本机自然日后重复一次普通按键和一次完整语音。
7. 从上一候选版本升级后再次启动，确认旧设置、按键映射和已有统计仍保留。

预期：每个去重后的普通按键语义按下只增加 1 次；系统重复、对应释放、语音键和 Keyboard/HID 双路径不重复累计。只有 `STREAM_STOP` 后 WASAPI 排空及 ATVV drain 均成功的会话增加语音次数；语音时长按该会话 16 kHz 已解码采样数折算。每日数据按本机日期保存，页面提供今日、本周、全部与最近 7 天汇总；退出前最后增量落盘，重启和升级不重置或重复。`usage_statistics` 统计字段不得包含设备 ID、蓝牙地址、HID 路径、遥控器/按键名称、端点身份、语音内容、识别文字或第三方 App 上下文；应用原有的用户明确选择设备设置按既有配置边界保存。

失败判定：一次实体按键增加多次、语音键进入普通按键统计、中断会话被记为完成、页面只显示进程内计数而重启归零、升级丢失或重复统计、日期跨天错误、统计文件含任何设备或用户内容、统计写入阻塞 BLE/Raw Input 回调而造成语音或按键响应退化。

## 用例十二：Windows Tauri/WebView 运行时仿真

1. 以 `runtime-simulation` Cargo feature 和 `VITE_SAYALL_RUNTIME_SIMULATION=1` 构建专用测试程序；普通构建不得包含仿真前端入口或仿真专用 Tauri command。
2. 在 `windows-latest` 启动该程序，并设置唯一的运行报告路径；不得向真实桌面发送 SendInput，也不得尝试扫描真实 BLE、音频或 HID 设备。
3. 由实际 Windows WebView JavaScript 依次通过 Tauri IPC 读取运行快照、渲染 RC001/RC003 扫描结果、连接 RC001、选择仿真 CABLE Input、启动 Raw Input、保存并显式测试 Ctrl+C 映射。
4. 依次打开按键、统计、权限、关于和连接与语音页面；在权限页生成诊断摘要，确认平台明确标记为 `windows-ci-simulation`。
5. 通过测试专用 command 驱动纯 Rust ATVV 管线完成首次 `STREAM_START → 40 + 80 AUDIO → STREAM_STOP → DRAIN`，确认得到 240 个采样、generation 为 1、连接和音频均回到 ready。
6. 停止 Raw Input 并断开，确认最终快照为 `rawInput.phase = stopped` 和 `connection.phase = disconnected`；程序写入报告并自行以成功退出码结束。

预期：真实 Windows WebView、Tauri invoke 和 Rust command 边界完成闭环；五个侧栏页面均能挂载；RC001/RC003 和 IPC camelCase 数据可被 Vue 消费；测试专用 SendInput 只记录批次和事件数，不向桌面注入；普通生产构建经字符串检查不包含仿真平台名称或仿真 command。

失败判定：只调用 Rust 单元测试或浏览器预览而没有启动 Windows Tauri WebView；普通构建能启用仿真；测试误调用真实 BLE/WASAPI/Raw Input/SendInput；WebView 进程存活但没有完成报告；把仿真 240 个采样、就绪状态或页面渲染表述为 RC001/RC003 真机通过。

## 用例十三：NSIS 安装生命周期矩阵

1. 使用配置覆盖构建版本低于当前版本的同源码 NSIS predecessor fixture，将其移出当前 bundle 目录，再按普通生产配置构建当前 NSIS。
2. 静默安装 predecessor，确认只有一个 HKCU 卸载条目和一个开始菜单入口，安装位置位于当前用户目录。
3. 在本应用配置目录写入有效的 `settings.json`、`button-mappings.json` 和保留标记；设置夹具包含遥控器选择、音频端点、增益和每日使用统计，并记录两个 JSON 的 SHA-256。
4. 静默安装当前版本，确认版本升级、安装位置不变、卸载条目和快捷方式仍各一个，两个 JSON 的 SHA-256 逐字节不变；启动升级后程序并确认持续运行 8 秒。
5. 再次运行 predecessor 安装器，确认当前版本、当前主程序 SHA-256、设置、映射、统计和单一安装身份均未被替换；单独记录安装器退出码。
6. 通过当前卸载条目的真实静默命令卸载，确认程序目录、快捷方式和卸载条目移除，而设置、映射和保留标记继续存在。

预期：同一产品身份只存在一个当前用户安装；覆盖升级不改变本应用用户数据；较低版本安装器不能把当前版本降级；最终卸载边界与现有约定一致。

失败判定：使用当前版本直接覆盖当前版本冒充升级；升级后出现两个安装条目或快捷方式；设置、映射或统计被重写；降级后版本或主程序发生变化；卸载残留程序身份或误删用户数据；把同源码 predecessor fixture 表述为真实历史版本兼容性验收。

## 日志收集

日志不得包含语音内容、识别文字、真实蓝牙地址、完整 HID 路径、窗口标题或个人文档路径。报告应包含 Commit、版本、时间段、Windows 版本和失败步骤。

## 验证边界

- Mac 自动化：前端构建、Rust 核心测试、格式化和静态检查。
- Windows CI：Windows 编译、自动化测试、测试专用 Tauri/WebView/IPC 仿真、当前用户静默安装/启动存活/静默卸载和设置目录保留边界；不包含可见安装界面、SmartScreen、Windows 10 1809、真实硬件或第三方应用验收。
- 用户/维护者：RC001 与 RC003 分别的型号识别、蓝牙、音频、Raw Input、输入法、睡眠和安装升级真机验收。

## 当前自动化记录

2026-09-02 在 `windows-latest` 完成 Tauri/WebView/IPC 运行时仿真：

- Windows CI Run [`33611750471`](https://github.com/GetSayAll/remote-mic-app-windows/actions/runs/33611750471) 使用 Tauri CLI 构建仅测试 feature 程序，并在真实 Windows WebView 中完成 11 个结构化步骤；
- WebView JavaScript 通过真实 Tauri IPC 读取仿真快照，渲染 RC001/RC003 扫描结果，连接 RC001、选择仿真 CABLE Input、启动 Raw Input、保存映射并由非注入式 SendInput 记录器验证 Ctrl+C 四事件；
- 五个侧栏页面均完成导航和挂载，权限页生成平台标记为 `windows-ci-simulation` 的去标识化诊断摘要，统计 IPC 返回最近七天结构；
- 首次 `STREAM_START → 40 + 80 AUDIO → STREAM_STOP → DRAIN` 通过纯 Rust ATVV 管线得到 240 个采样和 generation 1，随后 Raw Input 与连接状态均完成释放；
- 同一 Run 重新构建普通 NSIS，二进制和前端资源均确认不含仿真平台名或仿真 command，并继续通过静默安装/卸载回归；
- Run [`33610428178`](https://github.com/GetSayAll/remote-mic-app-windows/actions/runs/33610428178) 首次暴露直接 `cargo build` 生成开发协议程序会等待 Vite dev server；改为 `tauri build --no-bundle --features runtime-simulation` 后修复；
- 该结果不访问真实 WinRT BLE、WASAPI、Raw Input 或桌面 SendInput，不代表 RC001/RC003、音质、VB-CABLE 或真实按键已经通过。

2026-09-02 在 `windows-latest` 完成 NSIS 静默安装与卸载生命周期验证：

- Windows CI Run [`33593663937`](https://github.com/GetSayAll/remote-mic-app-windows/actions/runs/33593663937) 从提交 `5f01fa41cfbd15b967a760e490b9343260e768a6` 构建 `无线麦 SayAll_0.1.0_x64-setup.exe`，SHA-256 为 `a9fa2bfbfd519189629abc3ed91d5a811752081452d93c475c62129dd1e16e44`；
- `/S` 安装以普通 Runner 用户完成，唯一卸载条目位于 HKCU，安装目录为 `C:\Users\runneradmin\AppData\Local\无线麦 SayAll`，开始菜单文件夹内只有一个 `无线麦 SayAll.lnk`；
- 安装后的主进程持续运行 8 秒，未立即崩溃；测试随后关闭进程，并使用卸载注册表中的真实命令执行 `/S` 卸载；
- 卸载后程序目录、开始菜单文件夹和卸载条目均移除，`%APPDATA%\app.getsayall.remote-mic.windows` 中的测试标记保留，确认默认静默卸载不删除 Tauri `app_config_dir`；
- Run [`33591214805`](https://github.com/GetSayAll/remote-mic-app-windows/actions/runs/33591214805) 在安装前暴露空注册表结果的 PowerShell 数组边界，Run [`33592013119`](https://github.com/GetSayAll/remote-mic-app-windows/actions/runs/33592013119) 暴露带引号 `InstallLocation`，Run [`33592799733`](https://github.com/GetSayAll/remote-mic-app-windows/actions/runs/33592799733) 暴露非交互 Runner 无法通过 WScript 读取快捷方式目标；三项均按实际 Tauri NSIS 注册表契约修正；
- 该结果不证明可见安装/卸载界面、语言选择、SmartScreen、Windows 10 1809、Windows 11 真实桌面、签名发布包、RC001/RC003 或第三方应用已经通过。

2026-09-02 在 Apple Silicon Mac 上完成 Rust ↔ TypeScript IPC 契约快照验证：

- 同一份 JSON 夹具覆盖完整 `PlatformSnapshot`、嵌套连接/音频/Raw Input 快照，以及 RC001、RC003、unknown 三种 `PairedRemote`；
- Rust 测试从真实结构序列化并与夹具结构级全等比较，同时拒绝任何 snake_case IPC 字段；
- TypeScript 测试使用前端接口装载同一夹具，核对枚举白名单、精确字段集合、`remoteModel` 和 `isSupportedCandidate` 的 camelCase 边界；
- 该结果只证明仓库两端静态类型和序列化契约一致，不证明 Tauri command、Windows WebView、WinRT 或真机运行时数据已经通过。

2026-09-02 在 Apple Silicon Mac 上完成 RC001 协议场景回放：

- 从 `GetSayAll/hardware-simulation@65248499cac7da3ad46cd0c11dca1478f7733255` 的 RC001 短语音场景提取纯 ATVV JSON 夹具，保留 `STREAM_START`、40 + 80 字节音频拆包和 `STREAM_STOP`；
- 夹具回放确认无主动 `MIC_OPEN`，两段音频恰好组成一个 120 字节帧并解码为 240 个采样，停止时没有遗留半帧；
- 同一夹具扩展覆盖 20 次极速按下/释放空会话、20 次完整语音会话，以及首段 40 字节半帧后中断、下一会话仍首次完整解码；
- 该结果只证明纯 Rust ATVV/帧累积/ADPCM/会话状态机在确定输入下的行为，不证明真实 RC001 固件、WinRT 通知顺序或实际音质。

2026-09-02 在 Apple Silicon Mac 上完成 RC001 支持实现级验证：

- 对照 macOS `b233a88cc4457b00413dda6b37ec8b4af12c5121` 的设备档案、标准 2A24 型号识别和 RC001 首次短语音路径；
- Rust 单元测试覆盖 RC001/RC003 名称、`RC001` / `RC003` Model Number、通用名称不猜测型号、两种型号高半字节优先 ADPCM，以及 RC001 无需主动 `MIC_OPEN` 的 `STREAM_START → AUDIO → STREAM_STOP`；
- `sayall-core` 25 项、macOS `sayall-windows` 24 项、Tauri Host 6 项测试通过，`cargo check --workspace` 和 `x86_64-pc-windows-msvc` 平台层交叉静态检查通过；
- Vue 4 个测试文件共 9 项测试、TypeScript 类型检查、Vite 生产构建和 `tauri build --no-bundle` 通过；
- 以上结果只证明型号字段、WinRT API 类型、共用 ATVV 管线和界面可编译且自动化通过。真实 Windows 上的 RC001 与 RC003 设备名/2A24 值、首次语音、按键报告、睡眠恢复和 VB-CABLE 回环均为 deferred。

2026-09-01 在 Apple Silicon Mac 上完成：

- Vue 导航、连接阶段映射、诊断格式和权限页复制组件测试、类型检查和 Vite 生产构建；
- 900 × 620 与 1080 × 720 浏览器界面检查，全部五个侧栏入口可打开；权限页无横向溢出，诊断摘要可在页面内生成和滚动；
- 浏览器控制台零错误；
- `sayall-core` 22 项测试；macOS `sayall-windows` 20 项平台边界、退避、Raw Input 和 SendInput 纯逻辑测试；Tauri 设置与诊断 4 项测试；Windows CI 额外运行 WASAPI generation、有界队列、持久端点身份、重连调度、电源回调转发、Raw Input 纯逻辑测试和完整 Tauri Host 编译；
- 诊断隐私测试使用设备名称、蓝牙地址、端点身份、用户路径、HID 路径和后端错误作为敏感夹具，序列化摘要均未包含这些值；浏览器生成摘要也未出现 `remoteName`、`selectedEndpointId`、`selectedEndpointName` 或 `lastError` 字段；
- Raw Input 自动化覆盖两种 Windows 设备路径、零/多匹配 fail-closed、6/7/9 字节 HID 报告、RAWHID 批量拆分、重复 key-down、Keyboard/HID 双来源并集、停止释放和语音键隔离；隐藏窗口和真实事件形态仍需 Windows/RC001/RC003 分别验收；
- 纯 Rust 首次 `CAPS → STREAM_START → AUDIO → STREAM_STOP → DRAIN`、同步帧、8 kHz 拒绝和流外音频测试；
- macOS 全 workspace `cargo test`、`cargo check` 和格式化检查；
- `x86_64-pc-windows-msvc` 目标下 WinRT BLE/GATT/ATVV/WASAPI/Raw Input 平台层交叉静态检查。
- Windows CI Run [`33516627294`](https://github.com/GetSayAll/remote-mic-app-windows/actions/runs/33516627294) 通过；该结果证明 Windows 代码和 Tauri Host 可编译、自动化可运行，不证明隐藏窗口收到真实 RC001 或 RC003 报告。
- Windows CI Run [`33522534257`](https://github.com/GetSayAll/remote-mic-app-windows/actions/runs/33522534257) 通过并生成 artifact `sayall-windows-unsigned-preview-e11e460f15287468c01225a8ab86b302d59f09d4`；下载后确认只有一个 4,565,134 字节 NSIS 安装器、`SHA256SUMS.txt` 和 `build-metadata.json`，UTF-8 中文文件名可由 macOS `shasum -a 256 -c` 校验，安装器实算 SHA-256 与两份记录一致为 `e1cccf73c8b20487874eadb12eb9823635f52a0d7664c07fe6b8928532d246d6`，metadata 标记 `NotSigned` 和 `unsigned-ci-preview-not-for-public-release`。
- main Windows CI Run [`33525406955`](https://github.com/GetSayAll/remote-mic-app-windows/actions/runs/33525406955) 从 merge commit `94923dea303a66604b61ca2572fd13d9d48a9d60` 生成同边界 artifact 并通过下载后 SHA-256、metadata、PE/NSIS 文件类型和 UTF-8 校验文件复验。
- main Windows CI Run [`33529876579`](https://github.com/GetSayAll/remote-mic-app-windows/actions/runs/33529876579) 从诊断功能 merge commit `f8a65d33c31283c3db9e7964d9a9a36bc1e9f6c3` 成功完成前端、Rust、Windows Host、NSIS 和 artifact 上传；下载后确认唯一安装器为 4,561,376 字节，SHA-256 为 `cfddc29da4ed40d543039c545a0ab23993165243b95bf4b7335d98ff5ed848b3`，metadata 来源提交、`NotSigned` 和 `unsigned-ci-preview-not-for-public-release` 均一致。

2026-09-02 在 Apple Silicon Mac 上完成本机使用统计实现级验证：

- `sayall-core` 覆盖每日批量累计、今日/周/总汇总、饱和计数、非法持久时长归一化和旧 `settings.json` schema 迁移；
- Windows 平台计数覆盖普通按键按下次数、完整语音会话次数和 16 kHz 采样累计；语音只在成功排空后提交统计，中断路径清除未完成采样；
- Tauri 统计层覆盖进程计数增量、采样到秒换算、周一起始的本周汇总和最近 7 天日期桶；文件写入由独立后台线程执行，不进入 BLE、WASAPI 或 Raw Input 回调；
- Vue 组件覆盖今日、本周、全部切换、时长格式、最近 7 天数据和零数据空状态；900 × 620 与 1080 × 720 浏览器检查无横向溢出，新增统计界面最终字号不小于 16px，控制台零 warning/error；
- `x86_64-pc-windows-msvc` 目标下 `sayall-windows` 平台层静态检查通过；完整 Tauri Host 交叉检查在 Mac 上仍受缺少 `llvm-rc` 限制，交由 `windows-latest` CI 验证；
- Windows/RC001/RC003 真实按键计数、真实语音计数、跨自然日、退出落盘和升级保留均为 deferred，必须对两种型号执行用例十一后才能称为真机通过。

2026-09-02 在 Apple Silicon Mac 上完成 Windows 10 1809 版本门禁实现级验证：

- 纯 Rust 版本比较覆盖 Windows 10 build 17762 拒绝、17763 边界接受、更新 Windows 10 build 和未来主版本接受；
- `sayall-windows` 的 `x86_64-pc-windows-msvc` 静态检查通过，确认 `RtlGetVersion` 封装与原生 `MessageBoxW` 代码可编译；
- Tauri 配置解析、macOS workspace 测试/检查、前端 8 项测试、生产构建和 `tauri build --no-bundle` 通过；
- NSIS hook 的实际编译交由 Windows CI；低版本安装提示、退出码 1633、直接运行 exe、Windows 10 1809 接受和 Windows 11 接受仍为 deferred，必须执行用例零后才能称为运行验收通过。

以上结果只证明 Tauri Windows NSIS 未签名 Preview 可在干净 Windows Runner 生成且产物身份、摘要和来源元数据一致；不能证明安装、升级、卸载、WinRT 运行时、真实 RC001/RC003、WASAPI、Raw Input 或 SendInput 已经通过，这些用例仍为 deferred。
macOS 上对完整 Tauri Host 的 Windows 目标交叉检查需要 `llvm-rc`，本机未提供该 Windows 资源编译器，因此完整 Host 由 `windows-latest` CI 验证。

诊断复制已通过前端组件测试确认向 Clipboard API 写入完整可见摘要；应用内浏览器的剪贴板回读与页面 Clipboard API 不共享同一自动化通道，因此不能把浏览器提示当作真实 Windows WebView 剪贴板验收。Windows 安装包中的生成、复制和记事本逐字比对仍为 deferred。
