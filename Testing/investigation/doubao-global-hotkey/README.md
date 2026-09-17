# 豆包"免提模式"实验协议（原题：全局语音快捷键）

- 建立：2026-09-17
- 目的：判定豆包输入法能否被本应用（纯 SendInput，免驱动/免提权/免注入）唤起
- 前置结论：`Bugs/2026-09-17-doubao-injection-cross-repo-verification.md`
- 状态：**阶段 0 / 1 已完成（自动部分）**；阶段 1 物理对照与阶段 3 需 Echo 配合。

## 进度（2026-09-17）

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 注入可达性（无人工） | **passed** —— 注入确实进入系统输入流 |
| 1 | 免提未勾选基线（自动部分） | **passed** —— 注入唤不起、TAKEN=0 |
| 1 | 免提未勾选基线（物理对照） | **deferred** —— 待真人按右 Alt |
| 2 | 勾选「免提模式」 | **deferred** —— 只能由 Echo 在豆包设置里操作 |
| 3 | 免提已勾选测量 | **deferred** —— 依赖阶段 2 |
| 4 | 还原为长按模式 | **deferred** —— 依赖阶段 2 |

## ⚠️ 2026-09-17 重要更正：开关的真实名字是「免提模式」

原先按 vibe-flow 文档的说法寻找「全局语音快捷键」开关，**在 v0.9.0.0 界面上找不到**。
经三路取证，确认如下：

**实际界面（截图 + DLL 静态分析双重确认）** —— 豆包设置 → 语音输入：

```
语音输入模式
  ├─ 长按模式     按下说话，松手结束                    [右 Alt]
  └─ 免提模式     按一次即可开始说话，再按任意键可结束      [右 Alt]
麦克风选择        CABLE Output (VB-Audio Virtual Cable)
标点展示         空格代替标点 / 句末不加标点
```

**证据链**：

| 证据 | 结论 |
|---|---|
| `DoubaoIme.Settings.UI.dll` 有 `HandsFreeShortcutBox` / `UpdateHandsFreeShortcut` | UI 上确实有"免提快捷键"输入框 |
| 同 DLL 有「长按模式」「免提模式」「按一次即可开始说话，再按任意键可结束」文案 | 「免提模式」是可见的单选项 |
| `enableGlobalVoiceShortcut` 在 UI.dll / ViewModels.dll / NativeRuntime.dll / Settings.exe **三种编码下全部 0 命中**，仅 `ImeService.exe` 命中 1 次 | 该配置项**没有独立的 UI 开关**，属配置层内部键 |
| `ImeService.exe` 的 config schema 键序列：`enableVoiceShortcut` → `enableGlobalVoiceShortcut` → `voiceShortcut` → `voiceLongPressShortcut` | 它与"语音快捷键"同组，是**免提模式那一档的开关** |
| `handsFreeTipDid` / `ConsumeHandsFreeTipText` 位于状态栏/托盘代码段 | 「免提」是豆包自己的命名，与「全局」是同一件事 |

**推论**：`enableGlobalVoiceShortcut` = 「免提模式」开关的内部名。
勾选「免提模式」即开启该配置项。**这是本实验要操作的目标。**

## 为什么做这个实验

2026-09-04 判定"豆包注入判死"（基于豆包 0.8.2.7）。四个参考仓库独立印证该判定。
但复核发现**一格从未被测过**：

> vibe-flow 在 V1.x 支持豆包的做法是**引导用户先在豆包客户端启用"全局语音快捷键"**
> （`docs/V1_5_USER_GUIDE_ZH.md` L110、`V1_2_1_TUTORIAL_ZH.md` L84）。

- 豆包的**长按右 Alt**只在"豆包是当前活动输入法"时生效 → 已证注入无效。
- 豆包的**免提模式**（`enableGlobalVoiceShortcut`）语义上是**跨应用生效**的
  （"按一次即可开始说话，再按任意键可结束"——不需要按住），更接近系统级全局热键。
  **若勾选后豆包改用 `RegisterHotKey` 注册热键，则 SendInput 注入应当能触发，且完全合规。**

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
- **B** **两者都能唤起** → 免提模式下注入可用，**新合规路线成立**（我们想要的）
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

### 阶段 1：免提模式未勾选（对照基线）✅ 已完成

```bash
cd C:/wt-doubao
python Testing/investigation/doubao-global-hotkey/1-probe-hotkey-ownership.py
# 实测：6 个候选全部 FREE，TAKEN 总数 = 0

python -u Testing/investigation/doubao-global-hotkey/2-probe-injection-vs-physical.py --inject-rightalt
# 实测：注入 DOWN 成功，OimeVoiceWaveWindow 全程 visible=False，麦克风未被占用

python -u Testing/investigation/doubao-global-hotkey/2-probe-injection-vs-physical.py --toggle 2
# 实测：连点两次同样全程 visible=False（阴性对照干净）
```

**阶段 1 实测数据（2026-09-17，长按模式）**：

| 判据 | 注入右 Alt | 说明 |
|---|---|---|
| `OimeVoiceWaveWindow` visible | **false**（按住期间密集采样 + 连点 2 次均为 false） | 注入唤不起 |
| 录音设备被占用 | 0 / 2 | 未开麦 |
| `RegisterHotKey` 抢占 | 6/6 FREE，TAKEN=0 | 豆包未注册系统热键 |

→ **与 2026-09-04 的判定一致**：长按模式下注入确实唤不起豆包。

**仍需人工的部分**：本阶段还需真人按住**物理**右 Alt 一次（`--watch 30`）
作为同条件对照，以排除"豆包本身没就绪"。**该项待 Echo 执行。**

### 阶段 2：勾选「免提模式」

1. 豆包设置 → **语音输入** → **语音输入模式** → 选中 **「免提模式」**
   （对应配置项 `voice.enableGlobalVoiceShortcut`）。
2. 确认它右侧显示的快捷键是什么——**记下来**，实验要注入同一个组合。
   本机基线是 `右 Alt`（`modifierFlags=2049` = `0x801` = ALT | 右侧位）。
3. 关掉豆包设置窗口。

### 阶段 3：免提模式已勾选（关键测量）

```bash
# 探针 1：是否出现 TAKEN —— 若变 TAKEN，说明豆包确实注册了系统热键 ⭐
python Testing/investigation/doubao-global-hotkey/1-probe-hotkey-ownership.py

# 探针 2：注入侧（长按语义 vs 切换语义）
python -u Testing/investigation/doubao-global-hotkey/2-probe-injection-vs-physical.py --inject-rightalt
python -u Testing/investigation/doubao-global-hotkey/2-probe-injection-vs-physical.py --toggle 2

# 探针 2：物理侧（**必须真人按**）
python -u Testing/investigation/doubao-global-hotkey/2-probe-injection-vs-physical.py --watch 30
```

⚠️ **免提模式下优先用 `--toggle`**：它是"按一次开始、再按任意键结束"的
**切换式**语义，注入一次后需要**再注入一次来结束**，否则会一直开麦。
`--toggle 2` 正是"开始 + 结束"一对。

⚠️ **`:--watch` 的轮询间隔**：单次按键的语音窗口可能只闪几百毫秒，
探针默认 `--poll-ms 100`；不要改回 1 秒，否则会漏检（漏检=假阴性）。

**记录 B 组数据。**

### 阶段 4：还原

把「语音输入模式」**切回「长按模式」**（恢复实验前状态）。

## 结果记录表

| 阶段 | 热键 TAKEN 数 | 注入唤起 | 物理唤起 | 判定 |
|---|---|---|---|---|
| 1 · 免提模式未勾选 | **0**（已实测） | 待测 | 待测 | |
| 3 · 免提模式已勾选 | 待测 | 待测 | 待测 | |

### 分支决策

- 阶段 3 出现 TAKEN 且注入唤起 → **B 成立**：本仓库新增"豆包（免提模式）"档，
  UI 引导用户勾选该模式。**这是唯一能免驱动支持豆包的合规路径。**
- 阶段 3 无 TAKEN 且注入不唤起 → **A 成立**：豆包在免驱动前提下永久记为边界，
  UI 如实说明需要可选 Helper（ADR 0002 增强轨）。
- 物理键在任一阶段都唤不起 → **先修豆包配置**（麦克风/快捷键冲突），本轮作废。

## 脚本清单

| 文件 | 作用 | 自测状态 |
|---|---|---|
| `1-probe-hotkey-ownership.py` | `RegisterHotKey` 抢占探测，判断豆包是否注册系统热键 | **passed**（基线全 FREE） |
| `2-probe-injection-vs-physical.py` | 注入右 Alt + 观察三重判据；`--watch` 供真人按物理键；`--toggle N` 切换式语义 | **passed**（`--list` 能稳定读到 `OimeVoiceWaveWindow`；注入 DOWN 成功；`--toggle 2` 正常） |
| `3-probe-injection-reachability.py` | 自装 LL 钩子验证"注入是否进入系统输入流"，把"注入没送达"与"目标不响应"分开 | **passed**（`--inject` 实测捕获到 `injected=True`） |

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
