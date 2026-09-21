# 豆包输入法注入路径：跨仓库复核（2026-09-17）

## 背景

2026-09-04 本仓库以四层闭环判定"豆包注入路线判死（failed）"，结论基于豆包 **0.8.2.7**：

- 行为层：激活态下纯 wVk / scan+ext / InputInjector 注入右 Alt 均无反应；
- 逆向层：`ImeService` 全局 LL 钩子 `VoiceKeyHookProc` 回调首查 `LLKHF_INJECTED`，命中即纯透传；
- RegisterHotKey 路线 failed：7 状态矩阵全无语音，热键探针全 FREE（豆包从未注册系统热键）；
- physicalize 机制级死刑：LL 钩子每钩子收到私有结构副本，清标志不跨钩子传播。

2026-09-17 用户提供 4 个参考仓库，要求复核并继续接入豆包。本次结论：**原判定成立，且获三个独立来源交叉印证。** 唯一未变的开放项是"豆包 v0.9.0.0 是否改变了过滤行为"，需真机实测（见文末）。

## 参考仓库复核结果

| 仓库 | 语言 | 许可证 | 与豆包的关系 | 对本仓库的增量 |
|---|---|---|---|---|
| `richlearntodo-debug/vibe-flow` | C# | GPL-3.0 | **V1.x 支持豆包，V2.0 移除** | ⭐ 独立确认"厂商过滤模拟输入"；给出 V1 时代的可用前提 |
| `ZSTDJan/windows-remote-mic-app` | Python | GPL-3.0 | 支持豆包，靠 Frida 改内存 | ⭐ 证实 physicalize 无效，真正生效的是进程内改内存 |
| `QL-4/RemoteMapper` | C# | MIT | 仅微信 WeType，**无豆包** | 侧证：WeType 接受注入，作者不需要豆包路径 |
| `leowzz/axonkey` | Rust | 无许可证 | 语音键→右 Alt，但走 **Interception 内核驱动** | ⭐ 第四个独立印证：同为 Rust/Tauri 同类实现，未选纯 SendInput |

### 1. vibe-flow：独立来源确认"豆包过滤模拟输入"

`docs/V2_0_ENHANCEMENT_RESEARCH_ZH.md`：

> `豆包=无法自动化（厂商过滤模拟输入，已实测）`

> `第三方输入法边界：注入输入会被部分输入法过滤（豆包已实测），产品只能「检测 + 引导 + 换策略」，不能承诺绕过。`

该仓库在 V2.0 中**移除了豆包集成**：

> `docs/COMPATIBILITY_MATRIX_ZH.md`：`| 豆包输入法 | — | **V2.0 候选不再提供** | 已按用户要求移除；保存的旧值会被迁移为微信输入法并提示一次 |`

**这是与 2026-09-04 结论同向的独立复证**：不同作者、不同代码库、不同技术栈，同样实测到豆包过滤注入。

#### vibe-flow V1.x 提供的关键线索（本仓库此前未注意）

`docs/V1_5_USER_GUIDE_ZH.md` L110：

| 工具 | 推荐起点 | 触发方式 |
| --- | --- | --- |
| 豆包输入法 | 客户端全局语音快捷键 | 两边逐键核对 |

`docs/V1_2_1_TUTORIAL_ZH.md` L84：

> `豆包输入法 | 客户端语音快捷键 | 先在豆包中启用全局快捷键。`

即：其豆包支持是**引导用户先在豆包客户端内启用"全局语音快捷键"**，再让本程序发送同一组合键。这暴露出一条本仓库 2026-09-04 未穷尽的路线：

- **"输入法内长按右 Alt"**（快捷键只在豆包为活动输入法时生效）——已被判定注入无效；
- **"豆包全局语音快捷键"**（`enableGlobalVoiceShortcut`，跨应用生效）——语义上更接近**系统级全局热键**，若豆包以 `RegisterHotKey` 注册，则 SendInput 注入**应当**能触发。

本仓库 2026-09-04 的 RegisterHotKey 判决（7 状态矩阵无语音、热键探针全 FREE）是在 **`enableGlobalVoiceShortcut` 开关 × 显式快捷键 × 服务重启 × 激活态** 状态下做的，结论是"豆包从未注册任何系统热键，其 RegisterHotKey 调用点实为设置页冲突检测"。**但该矩阵是否覆盖了"开关为 true 时的注册路径"，需重新核对原始记录**——若当时开关始终为 false，则"开启后是否注册热键"这一格未被真正测过。

**本机现状（2026-09-17 只读观察）**：`%APPDATA%\DoubaoIme\conf\config.json`

```
voice.enableGlobalVoiceShortcut = false
voice.enableVoiceShortcut       = true
voice.voiceLongPressShortcut    = {"keyCode": 0, "modifierFlags": 2049}
voice.voiceShortcut             = {"keyCode": 32, "modifierFlags": 2049}
```

`2049 = 0x801 = MOD_ALT(0x1) | 0x800(右侧修饰位)`，与"长按右 Alt / 右 Alt + 空格"一致。`enableGlobalVoiceShortcut` 当前为 **false**。

> ⚠️ 按 AGENTS.md 边界，本仓库**不读取也不修改**豆包私有配置。以上仅为本机对照观察（判断该功能是否值得设计），**不进入产品路径**；绝不代用户改写该文件。

### 2. ZSTDJan：physicalize 无效、真正生效的是 Frida

`apps/windows/rc003/src/ovb_rc003/doubao_rpc.py` 模块头（逐字）：

> `Doubao ignores the LLKHF_INJECTED flag on ordinary Win32 synthetic keyboard events. The native RPC functions are retained as a narrow diagnostic adapter, while the production voice path uses an optional Frida callback hook to clear that flag inside Doubao's own low-level keyboard callback.`

其 Frida 脚本逻辑：attach `ImeService.exe` 的 `VoiceKeyHookProc`（按模块 SHA-256 校验后硬编码 RVA `0x7426C0`），在 `onEnter` 里把 `flags & ~0x12`（清 `LLKHF_INJECTED|LLKHF_LOWER_IL_INJECTED`）并清 `extra_info` 标记，**写回豆包进程自己的事件结构**。

**这从机理上解释了本仓库 2026-09-04 为何判 physicalize 无效**：
- 本仓库尝试的是**在自己进程的钩子副本上**清标志再转发——无效（LL 钩子每钩子收到私有副本，修改不跨钩子传播，且 `CallNextHookEx` 无"把修改后结构交给下游"的通道）；
- ZSTDJan 生效的是**在豆包进程内、其回调读值之前改它自己的内存**——同一块内存，豆包后续读到的就是已清标志的值。

两者不是同一种手法。原判定的"physicalize 结构性无效"依然正确；ZSTDJan 的有效手法**恰好是本仓库明确禁止的**：AGENTS.md「禁止把第三方进程注入作为稳定语音主路径」、基础语音路径不得依赖 Frida。

其 README L336 亦自述能力边界（逐字）：

> `如果手动快捷键都无法启动豆包，请先解决豆包快捷键或输入设备配置问题，本程序不能让本来就不工作的豆包输入法变得可用。`

即：**连 ZSTDJan 也不承诺"让不能用的豆包变得可用"**，它做的是"用户手动能用之后，帮用户发同一个键"。

另：`scripts/doubao_interception_probe.py` 使用 Interception 内核驱动注入——同样是本仓库排除的路径（需安装驱动、需管理员）。

### 3. RemoteMapper：无豆包，侧证 WeType 接受注入

`RemoteMapper/src/RemoteMic.cs` 头部注释：

> `Hold voice button on remote -> streams decoded mic audio to CABLE + holds [RAlt+Comma] for WeChat IME`

纯 SendInput（`KeyComboSender.cs`）即可驱动 WeType，与本仓库 Round 3 J 的翻案结论一致（WeType 不检查 `LLKHF_INJECTED`）。其未做豆包，与本仓库"豆包是注入死角"的判定相容。

### 4. axonkey：第四个独立印证——语音键走 Interception 内核驱动

同为本仓库技术栈（Rust + Tauri 2 + Vue），但**无许可证**，不可复制代码。其本身就是"Rust/Tauri 遥控器工具 + 豆包输入法"这一最贴近的同类实现，因此其选择极具参考价值。

`README.md` 默认映射表（逐字）：

> `| RC003 按键 | 单击行为 |`
> `| 语音键 | 右 Alt（RAlt，macOS 界面显示为右 Option） |`

系统要求表（逐字）：

> `| Windows 11 x64 | Interception 1.0.1 + 可选 Frida 增强通道 | ... | 基础映射需要 Interception；返回与音量键增强默认关闭，需管理员授权；语音另需 VB-CABLE |`

**关键推论**：axonkey 把语音键映射成右 Alt（正是豆包的长按热键），但它**不通过 SendInput，而是通过 Interception 内核驱动**下发——且明确写"基础映射需要 Interception"，即其 Windows 侧按键能力整体建立在驱动之上，没有"纯用户态注入"的降级路径。

这与本仓库 2026-09-04 的"physicalize/RegisterHotKey 均无法穿透豆包过滤"完全相容：**若纯 SendInput 可用，axonkey 没有理由为此引入一个需要管理员权限、需要重启的内核驱动。**

另外它把 Frida 也做成"可选增强通道"（用于返回/音量键），与其 macOS 侧依赖辅助功能权限一致——即该生态的做法普遍接受驱动级/进程级手段，这与本仓库 AGENTS.md 的边界不同。

（其 `docs/RC003_AUDIO_LATENCY.md` 另提供 RC003 音频参数对照：固件 2671 协商 ATVV 1.0 / ADPCM 16 kHz / 120 字节帧 = 每帧 240 样本 15 ms，实时需约 66.7 帧/秒；macOS 侧协商 ATT MTU 247 + data length 251 + 15 ms 连接间隔。与本仓库豆包结论无关，作为 RC003 链路参考数据记录。）

## 结论

1. **2026-09-04 判定维持**：豆包输入法在**免驱动、免管理员、免进程注入**的前提下，其本地语音热键无法由 SendInput 唤起。三个独立来源同向印证（vibe-flow 实测 + ZSTDJan 机理 + ZSTDJan README 自述边界）。
2. **确认无新增合法路线**：能达到豆包的三条已知路线分别是 Frida 进程注入（ZSTDJan）、Interception 内核驱动（ZSTDJan 探针 / axonkey）、以及物理化后的驱动注入（RemoteMapper 的 KMDF lower filter），**均被 AGENTS.md 明确排除在基础路径之外**。四个参考仓库中，凡覆盖豆包的实现**全部**依赖驱动级或进程级手段。
3. ~~**唯一值得新增测试的开放项**：豆包 `enableGlobalVoiceShortcut` 开启后，豆包是否改用 `RegisterHotKey` 注册系统级热键。~~ **→ 已于 2026-09-18 实测闭合，结论见下「阶段 3」与「最终结论」。**
   - ⚠️ **命名更正（2026-09-17）**：该配置项在 v0.9.0.0 界面上的显示名是「**免按模式**」，**不是**「全局语音快捷键」——按 vibe-flow 文档的字面名称在界面上**找不到该开关**。逐字证据与操作路径见 `Bugs/2026-09-17-doubao-global-shortcut-is-handsfree-mode.md`。（早期版本此处写作「免提模式」，系从 `HandsFreeShortcutBox` / DLL 字符串 / 截图 OCR 推断；同日改用 UIA 直读活窗口控件树后逐字确认为「免按」。）
   - 🔑 **两档快捷键不同（同日 UIA + 配置双向实测）**：长按模式 = 纯 右 Alt（`voiceLongPressShortcut = {keyCode: 0, modifierFlags: 2049}`）；免按模式 = **右 Alt + 空格**（`voiceShortcut = {keyCode: 32, modifierFlags: 2049}`，`keyCode 32` 即空格）。注入形态与**注入内容**必须同时随模式变化。
   - 触发条件：需用户**在豆包设置里自行切换**该模式（本仓库不代改）。
   - ~~现状：长按模式（`enableGlobalVoiceShortcut = false`），故该分支从未被真正测过。~~
     **→ 2026-09-18 已由用户切到免按模式并完成测量，见下。**
4. **v0.9.0.0 已复测（2026-09-18）**：原判定基于 0.8.2.7。静态字符串复核曾显示 `VoiceKeyHookProc` 仍在（2 处）、但 `LLKHF_INJECTED` 字面量 0 命中，无法只靠静态区分。**真机实测结论：原判定在 v0.9.0.0 上依然成立**（两档模式均同）。

## 🏁 最终结论（2026-09-18：两档模式均完成条件受控对照）

**豆包输入法 v0.9.0.0 在「免驱动 / 免提权 / 免进程注入」前提下，无法被 SendInput 唤起。**

| 模式 | 和弦 | 物理组 | 注入组 | `RegisterHotKey` |
|---|---|---|---|---|
| 长按模式 | 纯 右 Alt | ✅ `True`（t≈0.7s） | ❌ `False` | 未注册（TAKEN=0） |
| 免按模式 | 右 Alt + 空格 | ✅ `True`（t≈23.8s） | ❌ `False` | 未注册（TAKEN=0） |

- **免按模式没能改变结论。** 尽管它是「切换式」语义、理论上更可能走系统级热键，
  实测**没有**注册 `RegisterHotKey`，仍走自家 LL 钩子并过滤注入。
  → 结论 2「无新增合法路线」**维持**。
- **结论可归因**：物理组在两种模式下都 `True`，排除了「豆包本身没就绪」；
  注入组的 `False` 才具备归因力。**该格不再是 `deferred`。**
- **后续**：豆包在本仓库**永久记为第三方兼容性边界**（非本仓库缺陷）。
  UI 应如实说明「豆包需要安装可选 Helper（虚拟键盘驱动）才能由本应用触发」，把决策交给用户。

### 🔑 方法论：阴性结果必须先证明前置条件成立

本轮共出现**两次「条件不对却看起来有结果」**，均被识破，未污染结论：

1. 首次 `--watch 30` 得 `False` —— 焦点在 `WorkBuddy.exe`（类 `Chrome_WidgetWin_1`，
   **不接 TSF 文本输入**），不是可编辑文本框。豆包长按右 Alt 只在它是活动输入法时生效，
   焦点不对时按键本就该无反应 → **该 `False` 无效**。
2. `--toggle 2` 得 `False` —— 注入的是**纯右 Alt**，漏了免按模式需要的**空格**
   → **和弦错误，该 `False` 无效**。修正为 `--toggle 2 --hands-free` 后才有意义。

**判据：物理组 `True` 是「前置条件成立」的唯一证明。** 没有它，任何 `False` 都不可归因。

## 验证

- 跨仓库复核：`passed`（4 个仓库全部实际克隆并读取源码/文档；上述引用均为原文）。
- **注入可达性（阶段 0，2026-09-17 实测，`passed`）**：新增探针 `Testing/investigation/doubao-global-hotkey/3-probe-injection-reachability.py`——自装 `WH_KEYBOARD_LL` 钩子 + 注入右 Alt，由自己的钩子读取事件标志。实测（复现两次一致）：

  ```
  LL 键盘钩子已装载 hwnd=25822715
  >>> 注入右 Alt 点击…  注入调用返回 ok=True
  --- 注入按键捕获 ---
    vk=0xA5 scan=0x38 SYSDOWN  injected=True lower_il=False extended=True
    vk=0xA5 scan=0x38 UP       injected=True lower_il=False extended=True
  ```

  **这钉死了归因方向**：注入的右 Alt 确实进入了系统输入流，`LLKHF_INJECTED` 标志正确置位、扫描码 `0x38`、`extended=True`、按 `SYSDOWN`（系统键）投递。→ 豆包若唤不起，原因在**豆包如何对待带 `INJECTED` 标志的事件**，而不是"我们的键没送出去"。这是对结论 1 的**正向支撑证据**（原先只有"注入无效"的观察，没有"注入确实送达"的独立确认）。
  - ⚠️ **该探针不能替代物理对照**：LL 钩子是链式的，每个钩子收到**私有副本**，本探针只能证明"投递成立"，无法推断豆包看见了什么。A/B/C 判定仍须物理键对照。
- **长按模式基线（阶段 1 自动部分，2026-09-17 实测，`passed`）**：
  - `1-probe-hotkey-ownership.py`：6 个候选组合全部 FREE，`TAKEN` 总数 = **0** → 当前模式下豆包未注册任何系统热键。
  - `2-probe-injection-vs-physical.py --inject-rightalt`、`--toggle 2`、以及长按语义正解形态 `--inject-rightalt --hold-ms 1500`：注入 DOWN 成功，`OimeVoiceWaveWindow` **全程 `visible=False`**，录音设备 **0 / 2** 被占用 → 阴性对照干净。
  - **物理对照软件无法替代（已逐条排查）**：`osk.exe` 内部走 SendInput、`keybd_event` 同样带 injected 标记、本机**无** Interception / WinUHid 驱动（`System32` 与 `System32\drivers` 下 `.dll`/`.sys` 均不存在）→ **本机不存在非注入的按键通路**。
- **⚠️ 但上述 `False` 在 2026-09-18 被判为【不可归因】**：当时焦点在 `WorkBuddy.exe`（不接 TSF 文本输入），
  豆包长按右 Alt 只在它是活动输入法时生效 → 焦点不对时按键本就该无反应。见「最终结论」一节的方法论说明。

### 🏁 条件受控对照（2026-09-18 实测，核心证据）

**做法**：用户在**记事本**中点进文本区并切到豆包输入法；监听期间用户不切窗口
（本工具执行命令会抢焦点，故须让用户先就位）。

| 组 | 长按模式（纯右 Alt） | 免按模式（右 Alt + 空格） |
|---|---|---|
| **物理组**（真人按键） | ✅ **`True`**，t≈0.7s 首次出现（轮询 585 次） | ✅ **`True`**，t≈23.8s；且结束时仍 `visible=True`（切换式特征） |
| **注入组** | ❌ **`False`**（`--hold-ms 1500`） | ❌ **`False`**（`--toggle 2 --hands-free`） |
| `RegisterHotKey` | 0 TAKEN | 0 TAKEN |

→ **分支 A 在两种模式下都成立**。物理组的 `True` 排除了「豆包没就绪」，注入组的 `False` 因此可归因。

- 本仓库代码路径（目标选择 + TSF 激活 + 配置分流 + 免按模式切换式注入，含 `be6478d` 的组合键修正）：自动化 `passed`（145 + 36 + 95 用例全绿）；RC001/RC003 真机 `deferred`。

## 后续动作

1. ~~真机实验~~ **已完成（2026-09-18）**，协议与命令见 `Testing/investigation/doubao-global-hotkey/README.md`：
   - ① 长按模式物理对照 ✅ / ② 切到免按模式 ✅ / ③ 免按模式三项测量 ✅ / ④ 还原为长按模式 ✅
     （已用 `enableGlobalVoiceShortcut = false` 只读交叉验证还原到位）。
   - 结论：**两模式注入均失败 → 走原「后续动作 2」**。
2. ✅ **已采用**：豆包在本仓库**永久**记为第三方兼容性边界（非本仓库缺陷）。
   UI 应如实说明「豆包需要安装可选 Helper（虚拟键盘驱动）才能由本应用触发」，把决策交给用户。
3. 新增 `ATTRIBUTION.md` 条目：vibe-flow 的 V1.x 豆包引导线索 + 其 V2.0 移除决定；ZSTDJan 的 Frida 机理（记为"机理解释，不作实现参考"）。
