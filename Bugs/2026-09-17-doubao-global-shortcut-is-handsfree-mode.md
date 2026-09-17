# 豆包"全局语音快捷键"在 v0.9.0.0 界面上的真实名称是「免按模式」

- 日期：2026-09-17
- 分类：外部应用调研 / 功能发现（非缺陷）
- 影响：豆包接入路线的实验设计（原设计基于错误的开关名称，会卡在"找不到开关"）
- 关联：`Testing/investigation/doubao-global-hotkey/README.md`、
  `Bugs/2026-09-17-doubao-injection-cross-repo-verification.md`

> ⚠️ **2026-09-17 稍晚的修正（UIA 实测）**：本文早期版本称该档位界面文案为
> 「免提模式」，那是从 `HandsFreeShortcutBox` 与 DLL 字符串推断的。**直接读取
> 活着的设置窗口 UIA 树后确认，界面实际显示「免按模式」**，且**两档的快捷键
> 并不相同**（长按 = 右 Alt；免按 = 右 Alt + 空格）。详见文末「实测修正」一节。
> 「免提」「免按」指同一档位（内部名 `enableGlobalVoiceShortcut`）。

## 症状

按 vibe-flow 文档的指引，需要在豆包客户端"启用全局语音快捷键"。
但 **Echo 在豆包 v0.9.0.0 的设置界面里找不到任何叫「全局」的选项**。
按字面去找，这个实验的第一阶段就无法开始。

## 调查过程

### 第一步：排除"藏在别的页面"

只读检查配置 `%APPDATA%\DoubaoIme\conf\config.json`（仅作对照观察，不进产品路径）：

```
voice.enableGlobalVoiceShortcut = False
voice.enableVoiceShortcut       = True
voice.voiceLongPressShortcut    = {keyCode: 0,   modifierFlags: 2049}
voice.voiceShortcut             = {keyCode: 32,  modifierFlags: 2049}
voice.handsFreeTipDid           = ''
voice.handsFreeTipShownCount    = 0
```

`enableGlobalVoiceShortcut` 确实存在，说明开关是真实的功能。
`2049 = 0x801 = ALT | 右侧位` → 右 Alt。

### 第二步：静态分析设置界面（决定性证据）

对 `C:\Program Files\DoubaoIME\versions\v0.9.0.0\` 下的文件做三路取证。

**取证 1 —— `enableGlobalVoiceShortcut` 在各文件的命中数（三种编码全扫）：**

| 文件 | UTF-8 | UTF-16LE | UTF-16BE | 结论 |
|---|---|---|---|---|
| `DoubaoIme.Settings.UI.dll` | 0 | 0 | 0 | ❌ 不存在 |
| `DoubaoIme.Settings.ViewModels.dll` | 0 | 0 | 0 | ❌ 不存在 |
| `DoubaoIme.Settings.NativeRuntime.dll` | 0 | 0 | 0 | ❌ 不存在 |
| `DoubaoImeSettings.exe` | 0 | 0 | 0 | ❌ 不存在 |
| `ImeService.exe` | 1 | 0 | 0 | ✅ 命中 |

**→ 这个配置项在设置界面里没有任何对应控件，是配置层内部键。**

**取证 2 —— UI DLL 里的语音相关控件名：**

```
HandsFreeShortcutBox          ← 免提快捷键输入框
LongPressShortcutBox          ← 长按快捷键输入框
ShortcutCaptureBox / ShortcutInputBox
VoiceSettingsPage / VoicePage / ShortcutSetupPage / VoiceTryoutPage
MicrophoneButton
```

**取证 3 —— UI DLL 里的语音相关文案（完整列表）：**

```
语音输入              语音输入模式           语音快捷键设置        长按模式
按住说话，松手结束      按一次即可开始说话，再按任意键可结束
麦克风选择            更换麦克风             体验语音输入
请按住快捷键开始说话...  未设置快捷键           点击可更改快捷键
恢复默认快捷键         输入快捷键             禁用快捷键
```

**关键**：有「**长按模式**」与「**按一次即可开始说话，再按任意键可结束**」两种模式文案，
且 `HandsFreeShortcutBox` 与 `LongPressShortcutBox` 两个输入框并列存在。
**但任何地方都没有「全局」二字，`HandsFreeShortcutBox` 也没有独立的中文标签。**

**取证 4 —— `ImeService.exe` 里的 config schema 键序列：**

```
enableVoiceShortcut
enableGlobalVoiceShortcut     ← 紧跟其后
voiceShortcut
voiceLongPressShortcut
voiceShortcutMode
voiceLongPressShortcutMode
```

它与"语音快捷键"同组，紧邻 `enableVoiceShortcut`。

### 第三步：直接截图界面确认

用 `BitBlt` 抓取 `DoubaoImeSettings.exe` 窗口（第一版用 `PrintWindow` 取不全数据，见下"踩坑"）：

```
语音输入模式
  ├─ 长按模式    按下说话，松手结束                     [右 Alt]
  └─ 免按模式    按一次即可开始说话，再按任意键可结束       [右 Alt + 空格]
麦克风选择       CABLE Output (VB-Audio Virtual Cable)
标点展示         ○ 空格代替标点   ○ 句末不加标点
```

**（上为 OCR 读出的初版，其中两处有误——档位名应为「免按模式」，
免按档的快捷键应为「右 Alt + 空格」；已由文末 UIA 实测修正。）**

## 结论

**`voice.enableGlobalVoiceShortcut` 就是界面上的「免按模式」**（早期称「免提」），
不是独立开关。

| 层面 | 名称 |
|---|---|
| 配置文件键 | `enableGlobalVoiceShortcut` |
| 界面显示 | **免按模式**（语音输入 → 语音输入模式，二选一） |
| UI 控件 | `HandsFreeShortcutBox` |
| 配套字段 | `handsFreeTipDid` / `handsFreeTipShownCount` / `ConsumeHandsFreeTipText` |

vibe-flow 文档说"在豆包中启用全局快捷键"**方向是对的**，
只是它用的是内部命名，与 v0.9.0.0 的界面文案对不上，导致按字面找不到。

**操作方式**：豆包设置 → 语音输入 → 语音输入模式 → 选中**「免按模式」**。

## 为什么这个区分重要

「长按模式」与「免按模式」在**按键语义与快捷键上都不同**，直接影响注入可行性判断：

| 模式 | 语义 | 快捷键 | 对注入的含义 |
|---|---|---|---|
| 长按模式 | 按下开始，松手结束 | 右 Alt | 与语音键生命周期绑定，**与豆包是否前台强相关** |
| **免按模式** | **按一次开始，再按任意键结束** | **右 Alt + 空格** | **切换式、不依赖按住** → 更可能走 `RegisterHotKey` 全局路径 |

免按模式的"再按任意键可结束"意味着豆包必须**持续监听全局按键**，
这为"SendInput 能否触发"提供了新的可能性——**这正是实验要验证的开放项**。

## 实测修正（2026-09-17 稍晚，UIA + 配置双向取证）

前文是从 **DLL 字符串与截图**推断的（截图 OCR 把「免按」误读成「免提」）。
为消除歧义，直接以公开 **UI Automation** 读取**活着的**设置窗口控件树
（脚本：本机临时 UIA 枚举，只用公开 API，不读不改豆包私有配置）。

**实测 UIA 文本（逐字）**：

```
语音输入模式
  长按模式
  按住说话，松手结束
  右 Alt                       ← 长按模式的快捷键
  免按模式                      ← ⚠️ 是「免按」，不是「免提」
  按一次即可开始说话，再按任意键可结束
  右 Alt + 空格                 ← ⚠️ 免按模式的快捷键，与长按不同！
```

**配置侧同向印证**（`%APPDATA%\DoubaoIme\conf\config.json`，只读观察）：

| 键 | 值 | 解出的快捷键 |
|---|---|---|
| `voice.voiceLongPressShortcut` | `{keyCode: 0, modifierFlags: 2049}` | `keyCode 0` = 无主键 → **纯 右 Alt** |
| `voice.voiceShortcut` | `{keyCode: 32, modifierFlags: 2049}` | `keyCode 32` = 空格 → **右 Alt + 空格** |
| `voice.enableGlobalVoiceShortcut` | `false` | 当前生效的是**长按模式** |
| `voice.hasShortcutConflict` | `false` | 无冲突（排除"按键被别的程序占用"） |

`2049 = 0x801 = MOD_ALT(0x1) | 0x800(右侧修饰位)`，两档一致；差异只在 `keyCode`。

### 结论（两处修正）

1. **界面文案是「免按模式」**，不是「免提模式」。两者指同一档位。
2. 🔑 **两档快捷键不同**：长按模式 = `右 Alt`；免按模式 = `右 Alt + 空格`。
   这**推翻**了本仓库代码里"豆包两档共用同一快捷键、只需改变注入时序"的假设
   ——若沿用该假设，免按模式会**发送错误的组合键**（见下"代码影响"）。

### 代码影响（已修）

`crates/sayall-windows/src/voice_target.rs` 原实现只让 `injection_shape()` 随模式
变化，而 `resolved_hotkey()` 在用户未显式录入时固定取 `VoiceTarget::Doubao`
的默认值 `[RightAlt]`。修正：

- 新增 `DoubaoVoiceMode::default_hotkey()`（长按 → `[RightAlt]`；免按 → `[RightAlt, Space]`）；
- 新增 `VoiceTargetConfig::default_hotkey_for_target()`，`resolved_hotkey()`
  改用它，使**和弦内容与注入时序同时随模式变化**；
- `src-tauri/src/lib.rs` 的两处 snapshot 构造改用 `default_hotkey_for_target()`
  （原先用 `target.default_hotkey()`，会把免按模式的实际快捷键**显示错**）；
- 前端 `doubaoVoiceModeLabel` 文案改为「免按模式」，`ConnectionPage.vue` 的
  模式按钮改用该函数（避免文案两处定义漂移）；
- 单测 `doubao_mode_does_not_affect_hotkey`（编码了被推翻的假设）删除，
  替换为 `doubao_mode_changes_the_injected_hotkey` 等 3 个新用例。

## 踩坑记录

1. **按内部命名找 UI 名称会扑空。** 跨应用调研时，配置键名 ≠ 界面文案，
   必须分别取证。本次若只读配置、不查 UI 资源，会一直卡在"找不到开关"。
2. **截图 OCR 不可作为文案的唯一来源。** 「免按」被 OCR 读成「免提」，
   并据此写进了文档与代码，直到用 UIA 读控件树才纠正。
   → **UI 文案若要用在代码/文档里，应以 UIA 读取或截图原图人工核对为准。**
3. **不要假设"两档模式共用快捷键"。** 同一功能的两个档位可能有**不同的**
   默认按键；本仓库因此差点在免按模式下发送错误组合键。
4. **`PrintWindow` 在本机取不全像素。** `PrintWindow(hwnd, dc, PW_RENDERFULLCONTENT)`
   返回 1（成功），但 `GetDIBits` 拿到的数据少于 `w*h*4`，转 PNG 时数组越界。
   → 改用 `BitBlt` 从**屏幕 DC** 直接拷贝 + `CreateDIBSection` 直接拿像素指针，
   实测字节数完全吻合（1753488 = 738×594×4）。
5. **窗口在屏幕外时截图会带黑边。** 本例窗口 rect 为 `(-33,75)-(705,669)`，
   左边缘出屏 → 先 `SetWindowPos` 挪进可视区再截，**截完记得还原原位置**。
6. **`BITMAPINFOHEADER.biHeight` 传负值表示 top-down。** 转 PNG 时要用绝对值，
   否则 PNG 编码报 `struct.error: 'I' format requires 0 <= number <= 4294967295`。
7. **UIA 用原始 COM 即可（本机无 `comtypes`）。** `CoCreateInstance` +
   虚表索引调用；注意 `IUIAutomation` 的 `CreateTrueCondition` 在索引 **21**
   （0-2 是 IUnknown，3-20 是其它方法），`ElementFromHandle` 在索引 **6**，
   `IUIAutomationElement::FindAll` 在 **6**，`ElementArray::get_Length` 在 **3**。

## 后续动作

实验按 `Testing/investigation/doubao-global-hotkey/README.md`（已更新）执行。
**仍是 `deferred`** —— 需要 Echo 在界面上切到「免按模式」并手动按物理键做对照。
