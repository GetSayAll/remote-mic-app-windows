// English strings for connection. Key = exact Chinese source passed to t().
export default {
  // 通用操作失败（原始 IPC/后端错误只进控制台）
  "操作没有完成，请稍后重试。": "The action did not complete. Try again later.",
  "连接出现问题，请稍后重试。": "There is a connection problem. Try again later.",
  "语音设备出现问题，请稍后重试。": "There is an audio device problem. Try again later.",

  // 输入工具卡片与状态胶囊
  "豆包输入法": "Doubao",
  "要开启“支持更多输入工具”": "Turn on “More Input Tools”",
  "微信输入法": "WeType",
  "用默认语音键，最省事": "Uses the default voice key; simplest option",
  "Vokie": "Vokie",
  "流式显示，智能整理，不占用输入法":
    "Shows text live, organizes speech intelligently, does not replace your input method",
  "其他工具": "Other App",
  "自己指定按键": "Choose the key yourself",
  "已自动设置": "Set up automatically",
  "未同步，点左侧卡片重设": "Out of sync. Click the card on the left to reset.",

  // 快捷键录入
  "已切换输入工具，已取消录入": "Input tool switched. Recording cancelled.",
  "已取消录入": "Recording cancelled.",
  "窗口失去焦点，已取消录入": "The window lost focus. Recording cancelled.",
  "录入已超时，请重新录入": "Recording timed out. Record again.",
  "按住说话快捷键已设为 {chord}": "Hold-to-talk shortcut set to {chord}",
  "按住说话快捷键已关闭，语音键仅输出语音":
    "Hold-to-talk shortcut is off. The voice key only sends voice.",
  "检测到仍有按住的按键，请先松开所有按键；松开后即可按新组合，录入将自动开始":
    "Some keys are still held down. Release all keys first; recording starts automatically once they are released.",
  "你按下的组合触发了微信输入法的语音（按键被其拦截，内容无法读取），已按微信输入法语音键默认值 左 Ctrl + 左 Win 生效。此快捷键需与微信输入法语音键一致；若你修改过微信输入法的语音键，请在微信输入法设置中查看后重新录入对应组合":
    "The keys you pressed triggered WeType voice (the IME intercepted them, so the combination could not be read). Applied the WeType default voice key, Left Ctrl + Left Win. This shortcut must match the WeType voice key; if you changed that key, check it in WeType settings and record the matching combination again.",
  "本次未捕获到任何按键。微信输入法会拦截它自己的语音键（默认 左 Ctrl + 左 Win），本次也未观测到语音被触发；请重试，或直接选择上方工具卡片使用固定组合":
    "No keys were captured. WeType intercepts its own voice key (default Left Ctrl + Left Win), and no voice trigger was observed this time. Try again, or pick a tool card above to use a fixed shortcut.",
  "。注意：按住说话会把该组合注入系统来唤起微信输入法语音，若与微信输入法语音键不一致将无法生效":
    ". Note: hold-to-talk injects this combination to trigger WeType voice; if it does not match the WeType voice key, it will not work.",
  "选与你输入工具里一致的语音键：":
    "Choose the voice key that matches your input tool:",
  "请按下你输入工具当前设置的语音键（也可单独按一个修饰键）；按 Esc 取消":
    "Press the voice key currently set in your input tool (a single modifier key also works). Press Esc to cancel.",
  "按“自定义组合键”时仍按着键的组合不会完整录入，先松手即可。":
    "If keys are still held when you click “Custom shortcut”, the combination is not recorded completely. Release them first.",
  "录入中…（按 Esc 取消）": "Recording… (Esc to cancel)",
  "自定义组合键": "Custom shortcut",
  "自定义组合键（暂未开放）": "Custom shortcut (not available yet)",
  "避免其他 App 使用同一个快捷键（Vokie、Chatterfly 这类语音工具会抢在输入法前面）。按住语音键时，如果当前输入法还不是你选的工具，可能需要再按住一次才能正常使用。":
    "Avoid using the same shortcut in other apps (voice tools such as Vokie and Chatterfly grab it before the input tool). While holding the voice key, if the current input tool is not the one you selected, you may need to hold it once more.",

  // Vokie 面板
  "已打开 Vokie 官网；安装后点“重新检测”。":
    "Opened the Vokie website. After installing, click “Check again”.",
  "Vokie 已在运行。": "Vokie is running.",
  "还没检测到 Vokie 在运行；如果它已经打开，点“重新检测”。":
    "Vokie is not running yet. If you have already opened it, click “Check again”.",
  "已打开 Vokie；正在等待它启动…": "Opened Vokie. Waiting for it to start…",
  "检测到 Vokie 正在运行：它和豆包用的是同一个快捷键（右 Alt），按住遥控器语音键可能唤起 Vokie。要用豆包，请先退出 Vokie。":
    "Vokie is running. It uses the same shortcut as Doubao (Right Alt), so holding the remote's voice key may open Vokie. To use Doubao, quit Vokie first.",
  "没有检测到 Vokie。先安装，再回来设置。":
    "Vokie was not found. Install it, then come back to set it up.",
  "打开官网 vokie.com": "Open vokie.com",
  "Vokie 没有运行：按住遥控器语音键不会唤起它（如果豆包输入法正在使用，出现的是豆包语音条）。打开 Vokie 后再试。":
    "Vokie is not running: holding the remote's voice key will not open it (if Doubao is in use, the Doubao voice bar appears instead). Open Vokie and try again.",
  "打开 Vokie": "Open Vokie",
  "检测中…": "Checking…",

  // 遥控器连接
  "蓝牙音频不完整": "Bluetooth audio is incomplete",
  "连接": "Connect",
  "遥控器连接": "Remote connection",
  "扫描中…": "Scanning…",
  "扫描已配对设备": "Scan for paired devices",
  "断开中…": "Disconnecting…",
  "断开": "Disconnect",
  "连接中…": "Connecting…",
  "语音按键已就绪": "Voice key ready",
  "正在确认语音功能": "Confirming voice support",
  "睡眠唤醒后自动重连": "Reconnects after sleep",
  "语音功能已就绪，按住遥控器语音键就能说话":
    "Voice is ready. Hold the remote's voice key and speak.",
  "连接后即可使用遥控器语音键": "Connect the remote to use its voice key.",
  "正在寻找小米遥控器…": "Looking for Xiaomi remotes…",
  "找到 {count} 个已配对的小米遥控器":
    "Paired Xiaomi remotes found: {count}",
  "没有找到已配对的小米遥控器": "No paired Xiaomi remote found",
  "已连接，正在确认语音功能": "Connected. Confirming voice support…",
  "遥控器连接已释放，本次运行已停止自动重连":
    "Remote connection released. Automatic reconnection is off for this session.",

  // 语音设备
  "语音设备": "Audio device",
  "读取中…": "Reading…",
  "刷新设备列表": "Refresh device list",
  "收起列表": "Hide list",
  "更换设备": "Change device",
  "选择设备": "Choose device",
  "共 {count} 个设备可选": "Devices available: {count}",
  "推荐": "Recommended",
  "其他音频设备": "Other audio device",
  "正在启用…": "Enabling…",
  "当前设备": "Current device",
  "选择": "Select",
  "语音会写入选中的设备": "Voice is written to the selected device.",
  "不会自动改动系统默认设备，需要在这里明确选择":
    "Your system default device is not changed automatically. Choose a device here.",
  "正在读取语音设备…": "Reading audio devices…",
  "已检测到 {count} 个 VB-CABLE 语音设备":
    "VB-CABLE audio devices found: {count}",
  "未检测到 VB-CABLE；安装完成后需要重启电脑，再重新检测":
    "VB-CABLE was not found. After installing it, restart your computer and check again.",
  "正在打开语音设备…": "Opening audio device…",
  "已自动选择 {name}": "Selected {name} automatically",
  "已选择 {name}": "Selected {name}",
  "已自动改用 {name}（{previous} 暂时打不开）":
    "Switched to {name} automatically ({previous} could not be opened for now).",
  "已打开 VB-CABLE 官方下载页面；安装时需要管理员权限，完成后请重启电脑":
    "Opened the official VB-CABLE download page. Installation requires administrator permission; restart your computer when it finishes.",
  "需要安装 VB-CABLE": "VB-CABLE is required",
  "由 VB-Audio 提供的免费虚拟声卡。安装需要管理员权限，完成后需重启电脑。":
    "A free virtual audio device from VB-Audio. Installation requires administrator permission; restart your computer when it finishes.",
  "正在打开…": "Opening…",
  "打开官方下载页": "Open official download page",
  "重新检测": "Check again",
  "语音设备已就绪。": "Audio device ready.",
  "已检测到 VB-CABLE。在下面列表里选择带「推荐」标记的 CABLE 设备；输入法的麦克风请选择 CABLE Output。":
    "VB-CABLE detected. Choose the CABLE device marked “Recommended” in the list below; set your input tool's microphone to CABLE Output.",
  "正在检测 VB-CABLE…": "Detecting VB-CABLE…",

  // 增益
  "增益": "Gain",
  "0 dB 保持原始音量；数值越大声音越响，也会放大环境噪声。建议先从 6–12 dB 开始。":
    "0 dB keeps the original volume. Higher values make the voice louder and also amplify background noise. Start with 6–12 dB.",
  "增益没有保存，已恢复到上一次的值。":
    "Gain was not saved. Restored the previous value.",

  // 语音输入设置
  "语音输入设置": "Voice input setup",
  "设置一次，之后按住遥控器语音键说话，松开就出字。":
    "Set this up once. After that, hold the remote's voice key to speak and release to insert text.",
  "选择你在用的输入工具": "Choose the input tool you use",
  "遥控器语音键替你按哪个键": "Which key the remote's voice key presses",
  "按住遥控器语音键": "Hold the remote's voice key",
  "右 Alt": "Right Alt",
  "左 Ctrl": "Left Ctrl",
  "左 Win": "Left Win",
  "左 Alt": "Left Alt",
  "不按键": "No key",
  "支持更多输入工具": "More Input Tools",
  "“支持更多输入工具”": "“More Input Tools”",
  "微信输入法不需要“支持更多输入工具”开关。":
    "WeType does not need the “More Input Tools” switch.",
  "“支持更多输入工具”在这台电脑上暂不可用；新版本支持后会恢复正常。":
    "“More Input Tools” is not available on this PC yet. It will work again in a version that supports it.",
  "这台电脑暂不支持“支持更多输入工具”，其他按键不受影响。":
    "This PC does not support “More Input Tools” yet. Other buttons are not affected.",

  // 照着做
  "照着做（只做一次）": "Follow these steps (once)",
  "豆包麦克风选 CABLE Output": "Set Doubao's microphone to CABLE Output",
  "豆包长按语音键选 右 Alt": "Set Doubao's hold-to-talk voice key to Right Alt",
  "切到豆包后，按住遥控器语音键说话":
    "Switch to Doubao, then hold the remote's voice key and speak",
  "微信输入法麦克风选 CABLE Output":
    "Set WeType's microphone to CABLE Output",
  "切到微信输入法后，按住遥控器语音键说话":
    "Switch to WeType, then hold the remote's voice key and speak",
  "Vokie 麦克风选 CABLE Output": "Set Vokie's microphone to CABLE Output",
  "Vokie 快捷键保持默认的 右 Alt": "Keep Vokie's default shortcut, Right Alt",
  "在要写字的地方按住遥控器语音键说话":
    "Hold the remote's voice key where you want to type and speak",
  "输入工具麦克风选 CABLE Output":
    "Set your input tool's microphone to CABLE Output",
  "语音键与第 2 步选的键一致":
    "The voice key matches the key you chose in step 2",
  "切到该输入法后，按住遥控器语音键说话":
    "Switch to that input tool, then hold the remote's voice key and speak",

  // 常见问题
  "常见问题（点开查看）": "FAQ (click to open)",
  "为什么语音键要“替你按一个键”？遥控器语音键不是键盘按键，输入法只认键盘按键，所以应用替你按住它。":
    "Why does the voice key press a key for me? The remote's voice key is not a keyboard key, and input tools only respond to keyboard keys, so SayAll holds that key for you.",
  "“支持更多输入工具”和“按键”页的“全按键支持”是同一个开关，两处随时同步；每次开启都会弹出一次系统授权。":
    "“More Input Tools” and “All Buttons” on the Buttons page are the same switch and stay in sync; turning it on always shows a system authorization prompt.",
  "“替你按下的键”目前提供 左 Ctrl + 左 Win、右 Alt、左 Alt 和不按键四种；自由录入正在重做，暂未开放。":
    "“The key SayAll presses for you” currently offers Left Ctrl + Left Win, Right Alt, Left Alt, and no key. Free recording is being reworked and is not available yet.",
  "微信输入法要求按住约半秒以上（需要联网），快速点按不出字是它自己的要求，不是故障。":
    "WeType requires holding for about half a second or longer (it needs a network connection). Quick taps not producing text is its own requirement, not a fault.",
  "豆包要是当前输入法，否则按住遥控器语音键只会弹出 Windows 的 Alt 菜单（记事本里会出现“文件(F)、编辑(E)”这类字母）。应用会在你**按住语音键时**把输入法切到所选工具；刚切换过输入工具后的第一次按住如果没反应，松开再按一次即可（第一次那下用于切换输入法）。":
    "Doubao must be the current input tool; otherwise holding the remote's voice key only opens the Windows Alt menu (Notepad shows letters such as “File (F), Edit (E)”). When you **hold the voice key**, SayAll switches the input tool to the one you selected. If the first hold right after switching tools does nothing, release and hold again (the first one is used to switch input tools).",
  "Vokie、微信输入法或豆包如果没有单独的麦克风选项，把系统默认录音设备设为 CABLE Output。":
    "If Vokie, WeType, or Doubao has no separate microphone option, set the system default recording device to CABLE Output.",
  "Vokie 和豆包用的是同一个快捷键（右 Alt），同一时间只会有一个在响应：Vokie 要在运行中，豆包要是当前输入法；两个同时开着时 Vokie 会抢先。":
    "Vokie and Doubao use the same shortcut (Right Alt), so only one responds at a time: Vokie must be running and Doubao must be the current input tool. If both are open, Vokie takes priority.",
} as Record<string, string>;
