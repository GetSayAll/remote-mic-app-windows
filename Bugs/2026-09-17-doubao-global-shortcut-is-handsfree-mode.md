# 豆包"全局语音快捷键"在 v0.9.0.0 界面上的真实名称是「免提模式」

- 日期：2026-09-17
- 分类：外部应用调研 / 功能发现（非缺陷）
- 影响：豆包接入路线的实验设计（原设计基于错误的开关名称，会卡在"找不到开关"）
- 关联：`Testing/investigation/doubao-global-hotkey/README.md`、
  `Bugs/2026-09-17-doubao-injection-cross-repo-verification.md`

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
  └─ 免提模式    按一次即可开始说话，再按任意键可结束       [右 Alt]
麦克风选择       CABLE Output (VB-Audio Virtual Cable)
标点展示         ○ 空格代替标点   ○ 句末不加标点
```

**结论确凿。**

## 结论

**`voice.enableGlobalVoiceShortcut` 就是界面上的「免提模式」，不是独立开关。**

| 层面 | 名称 |
|---|---|
| 配置文件键 | `enableGlobalVoiceShortcut` |
| 界面显示 | **免提模式**（语音输入 → 语音输入模式，二选一） |
| UI 控件 | `HandsFreeShortcutBox` |
| 配套字段 | `handsFreeTipDid` / `handsFreeTipShownCount` / `ConsumeHandsFreeTipText` |

vibe-flow 文档说"在豆包中启用全局快捷键"**方向是对的**，
只是它用的是内部命名，与 v0.9.0.0 的界面文案对不上，导致按字面找不到。

**操作方式**：豆包设置 → 语音输入 → 语音输入模式 → 选中**「免提模式」**。

## 为什么这个区分重要

「长按模式」与「免提模式」在**按键语义上根本不同**，直接影响注入可行性判断：

| 模式 | 语义 | 对注入的含义 |
|---|---|---|
| 长按模式 | 按下开始，松手结束 | 与语音键生命周期绑定，**与豆包是否前台强相关** |
| **免提模式** | **按一次开始，再按任意键结束** | **切换式、不依赖按住** → 更可能走 `RegisterHotKey` 全局路径 |

免提模式的"再按任意键可结束"意味着豆包必须**持续监听全局按键**，
这为"SendInput 能否触发"提供了新的可能性——**这正是实验要验证的开放项**。

## 踩坑记录

1. **按内部命名找 UI 名称会扑空。** 跨应用调研时，配置键名 ≠ 界面文案，
   必须分别取证。本次若只读配置、不查 UI 资源，会一直卡在"找不到开关"。
2. **`PrintWindow` 在本机取不全像素。** `PrintWindow(hwnd, dc, PW_RENDERFULLCONTENT)`
   返回 1（成功），但 `GetDIBits` 拿到的数据少于 `w*h*4`，转 PNG 时数组越界。
   → 改用 `BitBlt` 从**屏幕 DC** 直接拷贝 + `CreateDIBSection` 直接拿像素指针，
   实测字节数完全吻合（1753488 = 738×594×4）。
3. **窗口在屏幕外时截图会带黑边。** 本例窗口 rect 为 `(-33,75)-(705,669)`，
   左边缘出屏 → 先 `SetWindowPos` 挪进可视区再截，**截完记得还原原位置**。
4. **`BITMAPINFOHEADER.biHeight` 传负值表示 top-down。** 转 PNG 时要用绝对值，
   否则 PNG 编码报 `struct.error: 'I' format requires 0 <= number <= 4294967295`。

## 后续动作

实验按 `Testing/investigation/doubao-global-hotkey/README.md`（已更新为「免提模式」版）执行。
**仍是 `deferred`** —— 需要 Echo 在界面上勾选「免提模式」并手动按物理键做对照。
