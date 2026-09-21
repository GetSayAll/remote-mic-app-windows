# 豆包「免按模式」实验协议（原题：全局语音快捷键）

- 建立：2026-09-17
- 目的：判定豆包输入法能否被本应用（纯 SendInput，免驱动/免提权/免注入）唤起
- 前置结论：`Bugs/2026-09-17-doubao-injection-cross-repo-verification.md`
- 状态：**阶段 0 / 1 已完成（自动部分）**；阶段 1 物理对照与阶段 3 需 Echo 配合。

## 进度（2026-09-17）

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 注入可达性（无人工） | **passed** —— 注入确实进入系统输入流 |
| 1 | 长按模式：物理 vs 注入对照 | **passed** —— 物理 `True`(0.7s) / 注入 `False` → **分支 A** |
| 2 | 切到「免按模式」 | **passed** —— Echo 已操作 |
| 3 | 免按模式测量 | **passed** —— 物理 `True`(23.8s) / 注入 `False` / TAKEN=0 → **分支 A** |
| 4 | 还原为长按模式 | **passed** —— `enableGlobalVoiceShortcut=false` 已确认还原 |

> ## 🏁 最终结论（2026-09-18，两档模式均实测）
>
> **豆包输入法 v0.9.0.0 在「免驱动 / 免提权 / 免进程注入」前提下，无法被 SendInput 唤起。**
>
> | 模式 | 和弦 | 物理组 | 注入组 | `RegisterHotKey` |
> |---|---|---|---|---|
> | 长按模式 | 纯 右 Alt | ✅ `True`（t≈0.7s） | ❌ `False` | 未注册（TAKEN=0） |
> | 免按模式 | 右 Alt + 空格 | ✅ `True`（t≈23.8s） | ❌ `False` | 未注册（TAKEN=0） |
>
> **分支 A 在两种模式下都成立。** 免按模式虽然是「切换式」语义、理论上更可能走
> 系统级热键，但实测**没有**注册 `RegisterHotKey`，仍走自家 LL 钩子并过滤注入。
>
> **这个结论是可归因的**：物理组在两种模式下都 `True`，排除了「豆包本身没就绪」
> 这一解释；因此注入组的 `False` 才具备归因力。**不再是 `deferred`。**
>
> → 后续：豆包在本仓库**永久记为第三方兼容性边界**（非本仓库缺陷）。
> UI 应如实说明「豆包需要安装可选 Helper（虚拟键盘驱动）才能由本应用触发」，把决策交给用户。

> 🔑 **方法论：阴性结果必须先证明前置条件成立。** 本轮实验中共出现**两次
> 「条件不对却看起来有结果」**，两次都被识破，未污染结论：
> 1. 首次 `--watch 30` 得 `False` —— 焦点在 `WorkBuddy.exe`（不接 TSF 文本输入），
>    不是可编辑文本框。**豆包长按右 Alt 只在它是活动输入法时生效**，焦点不对时按键本就该无反应。
> 2. `--toggle 2` 得 `False` —— 注入的是**纯右 Alt**，漏了免按模式需要的**空格**。
>    修正为 `--toggle 2 --hands-free` 后才是有效对照。
>
> **判据：物理组 `True` 是"前置条件成立"的证明。** 没有它，任何 `False` 都不可归因。

## ⚠️ 2026-09-17 两处重要更正

### 更正 1：档位名是「免按模式」，且快捷键与长按档**不同**

原先按 vibe-flow 文档的说法寻找「全局语音快捷键」，**在 v0.9.0.0 界面上找不到**。
经三路取证确认它就是「语音输入模式」里的第二档。**但档位名与快捷键此前都记错了**
（初版由截图 OCR + DLL 字符串推断：「免提模式」/ 右 Alt）。

**UIA 直接读取活着的设置窗口**（公开 UI Automation，不读不改豆包私有配置），
逐字结果：

```
语音输入模式
  长按模式     按住说话，松手结束                    右 Alt
  免按模式     按一次即可开始说话，再按任意键可结束      右 Alt + 空格
麦克风选择    CABLE Output (VB-Audio Virtual Cable)
```

配置侧同向印证（只读观察）：

| 键 | 值 | 解出的快捷键 |
|---|---|---|
| `voice.voiceLongPressShortcut` | `{keyCode: 0, modifierFlags: 2049}` | `keyCode 0` = 无主键 → **纯 右 Alt** |
| `voice.voiceShortcut` | `{keyCode: 32, modifierFlags: 2049}` | `keyCode 32` = 空格 → **右 Alt + 空格** |
| `voice.enableGlobalVoiceShortcut` | `false` | 当前生效 = **长按模式** |
| `voice.hasShortcutConflict` | `false` | 无冲突（排除按键被别的程序占用） |

🔑 **两档快捷键不同**——注入时必须**同时**改「发什么和弦」与「怎么按」。
详见 `Bugs/2026-09-17-doubao-global-shortcut-is-handsfree-mode.md`（含代码修正记录）。

### 更正 2：`enableGlobalVoiceShortcut` 是免按档的配置层内部名

`DoubaoIme.Settings.UI.dll` 有 `HandsFreeShortcutBox` / `UpdateHandsFreeShortcut`
（内部命名用 HandsFree），界面文案却写「免按」；该键在设置 UI/ViewModel/
NativeRuntime 各 DLL 与设置 exe 中三种编码全 0 命中，仅 `ImeService.exe` 命中 1 次
→ **无独立 UI 开关**，它就是免按模式那一档。

**「免提」「免按」「HandsFree」指同一档位。**

## 为什么做这个实验

2026-09-04 判定"豆包注入判死"（基于豆包 0.8.2.7）。四个参考仓库独立印证该判定。
但复核发现**一格从未被测过**：

> vibe-flow 在 V1.x 支持豆包的做法是**引导用户先在豆包客户端启用"全局语音快捷键"**
> （`docs/V1_5_USER_GUIDE_ZH.md` L110、`V1_2_1_TUTORIAL_ZH.md` L84）。

- 豆包的**长按右 Alt**只在"豆包是当前活动输入法"时生效 → 已证注入无效。
- 豆包的**免按模式**（`enableGlobalVoiceShortcut`）语义上是**跨应用生效**的
  （"按一次即可开始说话，再按任意键可结束"——不需要按住），更接近系统级全局热键。
  **若启用后豆包改用 `RegisterHotKey` 注册热键，则 SendInput 注入应当能触发，且完全合规。**

本机基线（2026-09-17 只读观察）：`enableGlobalVoiceShortcut = false` → 该分支从未激活。

## 实验设计：三重判据 + 物理键对照

判据（任一命中即"唤起"）：

| # | 判据 | 强度 |
|---|---|---|
| 1 | 豆包语音窗口 `OimeVoiceWaveWindow` 变为 **visible=True** | ⭐⭐⭐ 最强（已实测可稳定读到） |
| 2 | 豆包开麦导致系统麦克风被占用 | ⭐⭐ |
| 3 | 屏幕差异 | ⭐ 最弱，仅辅助 |

**关键设计：物理键必须由真人按。** 若我们注入"物理键对照组"，就分不清
"注入"和"注入的物理键"了——对照会自我污染。

三个结果分支：

- **A** 物理能唤起、注入不能 → 原判定成立，豆包记为第三方兼容性边界（预期结果）
- **B** **两者都能唤起** → 免按模式下注入可用，**新合规路线成立**（我们想要的）
- **C** 两者都不能唤起 → 豆包自身没配好（快捷键冲突/麦克风未选 CABLE Output），
  本轮实验无效，先修豆包配置再复测

## 步骤

### 准备工作（一次性）

1. **确认豆包麦克风是 `CABLE Output`**（截图确认本机已是此项，✅ 无需调整）。
2. 打开记事本，点进文本框，确保**输入法切到豆包**（看任务栏输入指示器）。

### 阶段 0：注入可达性（不需要人工按键）✅ 已完成

**这一步把"注入有没有到达系统输入流"单独钉死**，避免后面把"豆包不响应"
误判成"注入没送达"。

```bash
python -u Testing/investigation/doubao-global-hotkey/3-probe-injection-reachability.py --inject
```

⚠️ **必须加 `-u`**：不加时 Python 的行缓冲会让输出在脚本被中断时整段丢失，
表现为"跑完什么都不打印"，极易误判成脚本被沙箱拦截。

**2026-09-17 实测结果（passed）**：

```
LL 键盘钩子已装载
>>> 注入右 Alt 点击…
    注入调用返回 ok=True
--- 注入按键捕获 ---
  vk=0xA5 scan=0x38 SYSDOWN  injected=True lower_il=False extended=True
  vk=0xA5 scan=0x38 UP       injected=True lower_il=False extended=True
```

**结论**：注入的右 Alt **确实进入了系统输入流**，且 `LLKHF_INJECTED` 标志
正确置位、`scan=0x38`、`extended=True`、Alt 按系统键（`SYSDOWN`）投递。
→ 因此若豆包唤不起，原因在**豆包如何对待带 `INJECTED` 标志的事件**，
而不是"我们的键没送出去"。这为原判定提供了正向支撑证据。

### 阶段 1：免按模式未启用（对照基线）✅ 已完成

```bash
cd C:/wt-doubao
python Testing/investigation/doubao-global-hotkey/1-probe-hotkey-ownership.py
# 实测：6 个候选全部 FREE，TAKEN 总数 = 0

python -u Testing/investigation/doubao-global-hotkey/2-probe-injection-vs-physical.py --inject-rightalt
# 实测：注入 DOWN 成功，OimeVoiceWaveWindow 全程 visible=False，麦克风未被占用

python -u Testing/investigation/doubao-global-hotkey/2-probe-injection-vs-physical.py --inject-rightalt --hold-ms 1500
# 实测：长按语义 1.5 秒同样 visible=False（长按模式的正解形态也不行）

python -u Testing/investigation/doubao-global-hotkey/2-probe-injection-vs-physical.py --toggle 2
# 实测：连点两次同样全程 visible=False（阴性对照干净）
```

> ⚠️ **候选键表已去重**：`Alt+Space` 曾同时以「免按模式正解」与「无 Win 位」两条出现，
> 同一 `(modifiers, vk)` 第二次 `RegisterHotKey` 必然失败并报 `1409`
> → **假 TAKEN**。现已删去重复项（保留 6 条互不相同的组合）。

**阶段 1 实测数据（2026-09-17，当前为长按模式）**：

| 判据 | 注入右 Alt | 说明 |
|---|---|---|
| `OimeVoiceWaveWindow` visible | **false**（单击 / 长按 1.5s / 连点 2 次 全部 false） | 注入唤不起 |
| 录音设备被占用 | 0 / 2 | 未开麦 |
| `RegisterHotKey` 抢占 | 6/6 FREE，TAKEN=0 | 豆包未注册系统热键 |

> ⚠️ **上表早期的 false 全部不可归因**（见下条）。真正的对照在 2026-09-18 完成。

#### ✅ 2026-09-18：条件受控的对照完成 → **长按模式 = 分支 A**

**关键教训：焦点条件必须先验证。** 早期多次 `--watch` 得到 `false`，原因是**前台窗口不是可编辑文本框**
（实测为 `WorkBuddy.exe`，类 `Chrome_WidgetWin_1`，内部不接 TSF 文本输入）。
豆包长按右 Alt 只在「豆包是活动输入法」时生效，焦点不在文本框上时按键本就该无反应
→ **那些 `false` 是无效对照，不能作为证据**。

**正确姿势**：用户在记事本里点进文本区 + 切到豆包输入法，**且监听期间不要切窗口**
（本工具一执行命令就会抢焦点，所以必须用后台方式跑、用户在命令启动后自己切回记事本）。

**同条件对照结果（记事本 + 豆包为活动输入法）**：

| 组 | 条件 | `OimeVoiceWaveWindow` 是否可见 |
|---|---|---|
| **物理组** | 真人按住物理右 Alt | ✅ **`True`** —— **t≈0.7s** 首次出现（轮询 585 次） |
| **注入组** | `--inject-rightalt --hold-ms 1500` | ❌ **`False`** —— 全程不可见 |

→ **分支 A 成立**：物理能唤起、注入不能。**且 2026-09-04 的判定在 v0.9.0.0 上依然成立**
（本轮是**重测验过**的，不是继承旧结论）。物理组的 `True` 排除了「豆包本身没就绪」这一解释，
注入组的 `False` 才具备归因力。

**仍需人工的部分**：长按模式下本阶段已完成，无需再补。

#### ⚠️ 为什么这一格软件无法替代（2026-09-17 逐条排查）

「物理对照」要求按键**不带 `LLKHF_INJECTED` 标志**。本机可行的替代途径已全部排查：

| 候选途径 | 排查结果 |
|---|---|
| `osk.exe`（屏幕键盘） | 未运行；且其内部实现走 `SendInput` → 仍带 injected 标志 |
| `keybd_event`（旧 API） | 存在，但与 `SendInput` 同一条注入路径 → 标志同样置位 |
| Interception 驱动 | **本机不存在**（`System32` 与 `System32\drivers` 下无 `interception.dll`/`.sys`） |
| WinUHid / 虚拟 HID | **本机不存在**（无 `WinUHid.dll`/`.sys`）；且属 ADR 0002 增强轨，须先审计 |

→ **本机不存在任何非注入的按键通路**，故真正的物理对照只能由真人完成。
如果注入对照组去造假"物理键"，那只是又一次注入，对照会自我污染（见「实验设计」）。

### 阶段 2：切到「免按模式」

1. 豆包设置 → **语音输入** → **语音输入模式** → 选中 **「免按模式」**
   （对应配置项 `voice.enableGlobalVoiceShortcut`）。
2. **确认它右侧显示的快捷键**：实测为 **`右 Alt + 空格`**
   （`keyCode 32` = 空格、`modifierFlags 2049` = ALT|右侧位），**与长按档的纯右 Alt 不同**。
   若用户改过，需在本应用的语音目标设置里同步录入同一组合。
3. 关掉豆包设置窗口。

### 阶段 3：免按模式已启用（关键测量）✅ 已完成 → 分支 A

```bash
# 探针 1：是否出现 TAKEN —— 若变 TAKEN，说明豆包确实注册了系统热键 ⭐
python Testing/investigation/doubao-global-hotkey/1-probe-hotkey-ownership.py
# 实测：6/6 FREE，TAKEN=0 → 免按模式下豆包【依然没有】注册系统热键

# 探针 2：注入侧 —— ⚠️ 必须加 --hands-free，否则送的是纯右 Alt（和弦错误！）
python -u Testing/investigation/doubao-global-hotkey/2-probe-injection-vs-physical.py --toggle 2 --hands-free
# 实测：注入「右 Alt + 空格」2 次，OimeVoiceWaveWindow 全程 visible=False

# 探针 2：物理侧（真人按住「右 Alt + 空格」）
python -u Testing/investigation/doubao-global-hotkey/2-probe-injection-vs-physical.py --watch 60
# 实测：True（t≈23.8s），且结束时窗口仍 visible=True —— 切换式语义特征
```

⚠️ **免按模式下的三个要点**：

1. **注入必须加 `--hands-free`**：免按档快捷键是 `右 Alt + 空格`，不加该开关送的是纯右 Alt
   → **和弦错误，阴性结果不可归因**（2026-09-18 实际踩过）。
2. **用 `--toggle` 而非 `--hold-ms`**：它是切换式，注入一次开始、再一次结束。
3. **物理侧按的是 `右 Alt + 空格`**，不是纯右 Alt。

**阶段 3 结果**：物理 `True` / 注入 `False` / TAKEN=0 → **分支 A**（同长按模式）。

### 阶段 4：还原 ✅ 已完成

已把「语音输入模式」切回「长按模式」（用户操作）。**只读交叉验证**：

```
%APPDATA%\DoubaoIme\conf\config.json
  voice.enableGlobalVoiceShortcut = False   ← 长按模式（与实验前基线一致 ✅）
```

## 结果记录表

| 阶段 | 热键 TAKEN 数 | 注入唤起 | 物理唤起 | 判定 |
|---|---|---|---|---|
| 1 · 长按模式 | **0** | ❌ 否 | ✅ **是**（0.7s） | **A 成立** |
| 3 · 免按模式 | **0** | ❌ 否 | ✅ **是**（23.8s） | **A 成立** |

### 分支决策（已定案）

- 阶段 3 无 TAKEN 且注入不唤起 → **A 成立**：豆包在免驱动前提下永久记为边界，
  UI 如实说明需要可选 Helper（ADR 0002 增强轨）。**已采用此结论。**
- ~~阶段 3 出现 TAKEN 且注入唤起 → B 成立~~：实测未出现。
- 物理键确认能唤起（两模式均 ✅）→ 排除「豆包没配好」，本轮实验**有效**。

## 脚本清单

| 文件 | 作用 | 自测状态 |
|---|---|---|
| `1-probe-hotkey-ownership.py` | `RegisterHotKey` 抢占探测，判断豆包是否注册系统热键 | **passed**（两模式均 6/6 FREE） |
| `2-probe-injection-vs-physical.py` | 注入 + 观察三重判据；`--watch` 供真人按物理键；`--toggle N`；**`--hands-free` 让注入带空格** | **passed**（两模式对照完成） |
| `3-probe-injection-reachability.py` | 自装 LL 钩子验证"注入是否进入系统输入流"，把"注入没送达"与"目标不响应"分开 | **passed**（`--inject` 实测捕获到 `injected=True`） |
| `4-probe-uia-reader.py` | 用公开 UI Automation 直读第三方设置窗口控件树，拿到**逐字界面文案** | **passed**（读到 72 个元素，确认「免按模式」「右 Alt + 空格」） |

### `4-probe-uia-reader.py` 用法

```bash
python -u Testing/investigation/doubao-global-hotkey/4-probe-uia-reader.py
# 默认找 class 含 DoubaoIme 的可见窗口；也可 --class-substr / --title-substr 指定
```

**为什么需要它**：跨应用调研中「配置键名 ≠ 界面文案，DLL 字符串 ≠ 界面文案」。
本仓库曾因截图 OCR 把「免按」读成「免提」而写错代码与文档。
**凡是要把 UI 文案写进代码/文档，都应以本脚本的读取结果或截图原图人工核对为准。**

⚠️ 本机**没有 `comtypes`**，脚本用 `ctypes` 直接调原始 COM 虚表。虚表索引见脚本头注释
（注意 `IUIAutomation::CreateTrueCondition` = **21**，算错会报 `0x80070057`）。
⚠️ 目标窗口**必须处于可见状态**，否则 `EnumWindows` 找不到它。

## 工具陷阱（已踩，勿重犯）

1. **`Add-Type` 被本机安全策略硬拦**（`Command blocked for security: Add-Type compiles
   and loads .NET code at runtime`），**沙箱内外都拦**。→ 改用 `python3 + ctypes`。
2. **`os.path.islink` 在本机不可靠**（一律返回 False）→ 判定联接要用
   `GetFileAttributesW & FILE_ATTRIBUTE_REPARSE_POINT`。
3. **SendInput 在沙箱内静默失败**（返回 0，不报错）。→ 注入类探针必须用
   `dangerouslyDisableSandbox` 或让用户手动跑。
4. **`INPUT` 结构体必须是 40 字节**。第一版 union 只填 24 字节 → `sizeof(INPUT)=32`
   → 注入静默无效（**这正是 2026-09-04 那个著名 bug 的重现**）。
   脚本已加启动断言 `assert sizeof(INPUT) == 40`，别再手写这个结构。
5. **bash heredoc 写文件会触发安全启发式**（被误判为"从 bash 调 PowerShell"）。
   → 用 Write 工具写文件，不要 `cat > file <<EOF`。
6. **PowerShell 工具 stdout 不返回** → 本方案已全部改用 Python，规避该问题。
7. **截图用 `PrintWindow` 在本机取不全数据**（返回字节数少于 w*h*4）→
   改用 `BitBlt` 从屏幕 DC 直接拷贝 + `CreateDIBSection` 拿像素指针。
   另注意窗口若部分在屏幕外（负坐标），会截到黑边 → 先校正位置。
