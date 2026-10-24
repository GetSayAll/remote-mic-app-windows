// English strings for onboarding. Key = exact Chinese source passed to t().
export default {
  // ---- Shell / navigation ----
  "设置阶段": "Setup stage",
  "准备": "Prepare",
  "设置": "Set Up",
  "试一下": "Try It",
  "返回": "Back",
  "继续": "Continue",
  "开始使用": "Start Using SayAll",
  "重试": "Retry",

  // ---- Step 1: welcome / pairing ----
  "欢迎使用无线麦 SayAll": "Welcome to SayAll",
  "接下来会完成真实的设置和验证，不是走一遍页面教程。每一步都确认可用后才会放行。":
    "The next steps set up and verify the real voice path, not just a page tour. You can continue only after each step is confirmed working.",
  "首次使用：遥控器怎么连电脑": "First time: connect the remote to your PC",
  "准备好：小米蓝牙语音遥控器 2 或 2 Pro（目前只支持这两种型号），并确保它有电。":
    "Get your Xiaomi Bluetooth Voice Remote 2 or 2 Pro ready (only these two models are supported) and make sure it has battery power.",
  "在电脑上打开蓝牙设置（可直接点击下方按钮），打开电脑的蓝牙开关后点击「添加设备」然后选择「蓝牙」选项。":
    "Open Bluetooth settings on your PC (use the button below if you like), turn on Bluetooth, select Add device, then choose Bluetooth.",
  "打开蓝牙设置": "Open Bluetooth Settings",
  "长按「TV」键约 3 秒，直到指示灯（小米蓝牙语音遥控器 2 Pro 底部）开始闪烁（进入配对模式）。":
    "Press and hold the TV button for about 3 seconds until the indicator light (on the bottom of the Xiaomi Bluetooth Voice Remote 2 Pro) starts flashing. This puts the remote in pairing mode.",
  "同时长按「主页」+「菜单」直到指示灯快速闪烁——重置遥控器的蓝牙并重新进入配对模式。":
    "Press and hold Home + Menu together until the indicator flashes quickly. This resets the remote's Bluetooth and returns it to pairing mode.",
  "选中「小米蓝牙语音遥控器」，等待配对完成，列表里找不到设备时，先点开「显示所有设备」。":
    "Select Xiaomi Bluetooth Voice Remote and wait for pairing to finish. If it is not listed, first select Show all devices.",
  "配对失败，或遥控器以前配对过别的设备：回到第 4 步重置蓝牙后重新配对。":
    "If pairing fails, or the remote was paired with another device before, go back to step 4, reset Bluetooth, and pair again.",

  // ---- Step 2: remote connection ----
  "连接遥控器": "Connect the remote",
  "无线麦只支持小米蓝牙语音遥控器 2 和小米蓝牙语音遥控器 2 Pro（RC001 / RC003）。":
    "SayAll supports only the Xiaomi Bluetooth Voice Remote 2 and 2 Pro (RC001 / RC003).",
  "正在扫描…": "Scanning…",
  "扫描已配对设备": "Scan for paired devices",
  "已配对的小米蓝牙语音遥控器": "Paired Xiaomi Bluetooth Voice Remote",
  "正在连接…": "Connecting…",
  "已连接": "Connected",
  "连接": "Connect",
  "已收到遥控器按键": "Remote button received",
  "等待控制按键": "Waiting for a control button",
  "正在寻找小米遥控器…": "Looking for Xiaomi remotes…",
  "找到 1 个已配对的小米遥控器": "Found 1 paired Xiaomi remote",
  "找到 {count} 个已配对的小米遥控器": "Paired Xiaomi remotes found: {count}",
  "按一下遥控器上的普通按键（比如主页键），不要按语音键。":
    "Press a regular button on the remote, such as Home. Do not press the voice button.",
  "按键已收到。还差一步：点“扫描已配对设备”选中遥控器连接。":
    "Button received. One more step: select Scan for Paired Devices and choose the remote to connect.",
  "按键已收到。还差一步：点遥控器一行右侧的“连接”。":
    "Button received. One more step: select Connect on the right side of the remote row.",
  "刚收到的是另一支遥控器的按键": "That button came from another remote",
  "当前连接的是「{name}」。请按这一支的普通按键；要用另一支，就点它那一行的「连接」。":
    "Currently connected: “{name}”. Press a regular button on this remote. To use the other one, select Connect on its row.",
  "请先在上面点一行「连接」，选中你手上正在按的这一支。":
    "Select Connect on the row for the remote in your hand.",
  "正在自动恢复连接（第 {attempt} 次尝试），最多等约 60 秒，不需要重新配对。":
    "Reconnecting automatically (attempt {attempt}). This can take up to 60 seconds; you do not need to pair again.",
  "刚才按的是语音键": "That was the voice button",
  "请按遥控器上除语音键以外的普通按键（比如主页键）。":
    "Press a regular button on the remote, such as Home. Do not press the voice button.",
  "重新扫描": "Scan Again",
  "蓝牙当前不可用": "Bluetooth is unavailable",
  "还没有已配对的遥控器": "No paired remote yet",
  "没有找到遥控器": "Remote not found",
  "连接没有成功": "Connection failed",
  "当前没有已配对设备，请先按照上一步完成设备配对。":
    "No paired devices found. Complete the pairing step first.",
  "读取已配对设备失败，请重试。": "Could not read paired devices. Try again.",
  "找到 1 个已配对遥控器，正在自动连接…": "Found a paired remote. Connecting automatically…",
  "连接遥控器失败，请重试。": "Could not connect to the remote. Try again.",
  "已打开系统蓝牙设置；配对完成后回来点「扫描已配对设备」。":
    "Bluetooth settings opened. After pairing, come back and select Scan for Paired Devices.",
  "无法打开蓝牙设置，请重试。": "Could not open Bluetooth settings. Try again.",

  // ---- Step 3: audio device ----
  "选择语音设备": "Choose an audio device",
  "无线麦把遥控器的声音送到播放端（CABLE Input）；语音工具再从录音端（CABLE Output）读取声音。":
    "SayAll sends the remote's audio to CABLE Input; your voice app reads it from CABLE Output.",
  "这台电脑还没有 VB-CABLE 语音设备": "This PC does not have a VB-CABLE audio device yet",
  "先安装，再继续：": "Install it to continue:",
  "下载压缩包并完整解压，右键安装程序选择「以管理员身份运行」。":
    "Download the archive and extract it completely. Right-click the installer and select Run as administrator.",
  "安装完成后重启电脑，再回来点「重新检测」。":
    "Restart your PC after installation, then come back and select Check Again.",
  "正在打开…": "Opening…",
  "打开官方下载页": "Open official download page",
  "正在检测…": "Checking…",
  "重新检测": "Check again",
  "选择带「推荐」标记的设备：": "Choose the device marked Recommended:",
  "推荐": "Recommended",
  "这一步只需要这一个播放端。": "This step needs this playback device only.",
  "已就绪": "Ready",
  "选择": "Choose",
  "重新检测设备": "Check for Devices Again",
  "装了 VB-CABLE，但还没有检测到设备": "VB-CABLE is installed, but no device was detected",
  "语音设备还没有准备好": "The audio device is not ready yet",
  "正在读取语音设备…": "Reading audio devices…",
  "已检测到 {count} 个 VB-CABLE 语音设备": "VB-CABLE audio devices found: {count}",
  "没有检测到 VB-CABLE；安装完成后要重启电脑，再回来重新检测":
    "VB-CABLE was not found. Restart your PC after installing it, then come back and check again.",
  "读取语音设备失败，请重试。": "Could not read audio devices. Try again.",
  "正在打开语音设备…": "Opening audio device…",
  "已自动选择 {name}": "Selected {name} automatically",
  "已选择 {name}": "Selected {name}",
  "已自动改用 {name}（{previous} 暂时打不开）":
    "Switched to {name} automatically ({previous} could not be opened for now).",
  "选择语音设备失败，请重试。": "Could not select the audio device. Try again.",
  "已打开官方下载页；安装需要管理员权限，完成后请重启电脑":
    "The official download page is open. Installation requires administrator rights; restart your PC when it finishes.",
  "无法打开下载页，请重试。": "Could not open the download page. Try again.",
  "这台电脑还没有 VB-CABLE 语音设备，先安装再继续。":
    "This PC does not have a VB-CABLE audio device yet. Install it to continue.",
  "选择带「推荐」标记的语音设备，再继续。":
    "Choose an audio device marked Recommended to continue.",
  "语音设备没准备好；重新选择或刷新设备列表后再试。":
    "The audio device is not ready. Choose it again or refresh the device list, then retry.",

  // ---- Step 4: voice app ----
  "选择你要用的输入工具": "Choose your voice app",
  "无线麦会替你按住它的语音键；选好后，下面显示这个工具还需要怎么设置。":
    "SayAll holds the voice button for you. Once you choose an app, the steps below show what else it needs.",
  "检测到 Vokie 正在运行": "Vokie is running",
  "它会抢先响应右 Alt，豆包收不到语音键。请先退出 Vokie，再点「重新检测」。":
    "Vokie will capture Right Alt first, so Doubao will not receive the voice shortcut. Quit Vokie, then select Check Again.",
  "豆包输入法": "Doubao",
  "微信输入法": "WeType",
  "其他工具": "Other App",
  "自己选一个语音键": "Choose your own voice trigger key",
  "按住遥控器语音键 = 右 Alt": "Remote voice button = Right Alt",
  "按住遥控器语音键 = 左 Ctrl + 左 Win": "Remote voice button = Left Ctrl + Left Win",
  "按住遥控器语音键 = {hotkey}": "Remote voice button = {hotkey}",
  "本次设置": "This setup",
  "在豆包里把麦克风选为 CABLE Output；长按语音键保持右 Alt（出厂默认就是它）。":
    "In Doubao, set the microphone to CABLE Output. Keep the voice shortcut on Right Alt, which is the default.",
  "支持更多输入工具": "More Input Tools",
  "每次开启都会弹出 Windows 授权窗口，请选择“是”。":
    "Each time you turn it on, Windows asks for authorization. Select Yes.",
  "在微信输入法里把麦克风选为 CABLE Output；语音需要联网，按住约半秒以上再说话。":
    "In WeType, set the microphone to CABLE Output. Voice input needs a network connection; press and hold for about half a second before speaking.",
  "Vokie 正在运行，这一项可以继续了。": "Vokie is running. You can continue.",
  "Vokie 已安装但没有运行：先启动它，再回来「重新检测」。":
    "Vokie is installed but not running. Open it, then come back and select Check Again.",
  "正在启动…": "Opening…",
  "打开 Vokie": "Open Vokie",
  "没有检测到 Vokie。先安装，再回来「重新检测」。":
    "Vokie was not found. Install it, then come back and select Check Again.",
  "打开官网 vokie.com": "Open vokie.com",
  "在 Vokie 里把麦克风选为 CABLE Output，快捷键保持右 Alt。":
    "In Vokie, set the microphone to CABLE Output and keep the shortcut on Right Alt.",
  "选与该工具里一致的语音键；选「不按键」只把声音送到 CABLE，不替它按键：":
    "Choose the same voice trigger key as in the app. “No key” sends audio to CABLE without pressing anything:",
  "工具里把麦克风选为 CABLE Output。": "In the app, set the microphone to CABLE Output.",
  "先为「其他工具」选一个语音键。": "Choose a voice trigger key for Other App first.",
  "右 Alt": "Right Alt",
  "左 Alt": "Left Alt",
  "不按键": "No key",
  "“支持更多输入工具”": "“More Input Tools”",
  "「支持更多输入工具」已开启": "“More Input Tools” is on",
  "已选择输入工具": "Voice app selected",
  "语音键已适配": "Voice trigger key configured",
  "Vokie 已运行": "Vokie is running",
  "无需额外设置": "No extra setup needed",
  "已选择语音键": "Voice trigger key selected",
  "等待选择输入工具": "Waiting for a voice app",
  "工具检查": "Voice App Check",
  "读取语音工具设置失败，请重试。": "Could not read voice app settings. Try again.",
  "保存语音工具设置失败，请重试。": "Could not save the voice app settings. Try again.",
  "保存语音键失败，请重试。": "Could not save the voice trigger key. Try again.",
  "读取 Vokie 状态失败，请重试。": "Could not check Vokie. Try again.",
  "无法读取当前状态，请重试。": "Could not read the current state. Try again.",
  "已打开 Vokie 官网；安装后回来点「重新检测」。":
    "The Vokie website is open. Install it, come back, and select Check Again.",
  "无法打开 Vokie 官网，请重试。": "Could not open the Vokie website. Try again.",
  "已发出启动请求；启动后点「重新检测」。":
    "Launch requested. Once Vokie is running, select Check Again.",
  "无法启动 Vokie，请重试。": "Could not open Vokie. Try again.",
  "先选择一个输入工具，再继续。": "Choose a voice app to continue.",
  "Vokie 正在运行，它会抢先响应右 Alt；退出 Vokie 或改选 Vokie，再继续。":
    "Vokie is running and will capture Right Alt first. Quit Vokie or select Vokie here to continue.",
  "豆包需要开启「支持更多输入工具」：打开开关并按提示完成系统授权。":
    "Doubao requires More Input Tools. Turn it on and complete the Windows authorization prompt.",
  "没有检测到 Vokie；从官网安装后回来重新检测。":
    "Vokie was not found. Install it from the official website, then check again.",
  "Vokie 没有运行；打开它再重新检测。": "Vokie is not running. Open it, then check again.",
  "为「其他工具」选择语音键，再继续。": "Choose a voice trigger key for Other App to continue.",

  // ---- Step 5: voice test ----
  "按住遥控器语音键，试一句话": "Press and hold the remote's voice button and say a few words",
  "点一下下面的输入框，按住遥控器上的语音键说一句话，然后松开。文字应该会出现在输入框里。":
    "Select the field below, press and hold the remote's voice button, say a few words, then release. The text should appear in the field.",
  "开始之前，按你选的工具核对三件事": "Before you start, check three things for your voice app",
  "开始之前，按你选的工具核对两件事": "Before you start, check two things for your voice app",
  "输入法切到“{tool}”。": "Switch the input method to “{tool}”.",
  "麦克风选 CABLE Output（在“{tool}”的语音输入设置里）。":
    "Set the microphone to CABLE Output in {tool}'s voice input settings.",
  "工具里的语音键和这次设置一致（{hotkey}）。":
    "Match the app's voice trigger key to this setup ({hotkey}).",
  "本次设置快捷键：{hotkey}（“本次设置”）；语音设备：{audio}。":
    "Shortcut for this setup: {hotkey}. Audio device: {audio}.",
  "你说的话会出现在这里": "Your words will appear here",
  "正在接收你说的话…": "Listening to you…",
  "正在收尾…": "Finishing up…",
  "正在等文字出现…": "Waiting for text…",
  "等待你按住遥控器语音键。": "Press and hold the remote's voice button.",
  "先点一下输入框，再按住遥控器语音键。":
    "Select the field first, then press and hold the remote's voice button.",
  "成功：文字已经出现在输入框里，这一步通过了。":
    "Success: the text appeared in the field, so this step is complete.",
  "重新测试": "Test Again",
  "输入法还没切到“{expected}”：现在系统里选的是“{active}”。请在桌面右下角把输入法切成“{expected}”，或者回到上一步改选你的工具。":
    "The input method is not set to “{expected}”; the system is currently using “{active}”. Switch to “{expected}” from the taskbar, or go back and choose a different app.",
  "输入法还没切到“{expected}”。请在桌面右下角把输入法切成“{expected}”，或者回到上一步改选你的工具。":
    "The input method is not set to “{expected}”. Switch to “{expected}” from the taskbar, or go back and choose a different app.",
  "没有检测到语音会话。确认遥控器已就绪，按住语音键不要放，再试一次。":
    "No voice session was detected. Make sure the remote is ready, then press and hold the voice button and try again.",
  "收到了语音会话，但没有声音数据。对着遥控器正常说话，再试一次。":
    "The voice session started, but no audio arrived. Speak normally into the remote and try again.",
  "声音没有完整送达语音设备。检查设备是否被其他程序占用，再试一次。":
    "The audio did not fully reach the audio device. Check whether another app is using it, then try again.",
  "松开后这次语音没有正常结束。稍等一下或断开重连遥控器，再试一次。":
    "The voice session did not end normally after you released the key. Wait a moment, or reconnect the remote and try again.",
  "语音链路正常，但输入工具没有写出文字。检查工具麦克风是否选 CABLE Output、工具里的语音键是否和本次设置一致、工具是否在运行。":
    "The voice path worked, but your voice app did not enter text. Check that its microphone is set to CABLE Output, its voice trigger key matches this setup, and the app is running.",
  "检测到键盘输入。这一步请只用遥控器语音键，不要用键盘打字。":
    "Keyboard input was detected. Use only the remote's voice button for this step; do not type on the keyboard.",
  "输入框没有聚焦。先点一下输入框，再按住遥控器语音键。":
    "The input field was not focused. Select the field, then press and hold the remote's voice button.",
  "测试过程中输入框失去了焦点。点回输入框，再试一次。":
    "The input field lost focus during the test. Select it again and retry.",
  "“支持更多输入工具”现在是关闭的，请先打开它，再点“重新测试”。":
    "“More Input Tools” is off. Turn it on, then select Test Again.",
  "“支持更多输入工具”还在准备中（刚开启时通常几秒）。等几秒后点“重新测试”再试一次。":
    "“More Input Tools” is still starting; this usually takes a few seconds right after you turn it on. Wait a few seconds, then select Test Again.",
  "“支持更多输入工具”还没有生效，正在自动重试；如果一直不恢复，请把开关关掉再打开，然后重新测试。":
    "“More Input Tools” is not active yet, and SayAll is retrying automatically. If it does not recover, turn the switch off and on again, then test again.",
  "这次测试没有通过，再试一次。": "This test did not pass. Try again.",

  // ---- Step 6: control buttons ----
  "按一下遥控器的普通按键": "Press a regular button on the remote",
  "请用遥控器上除语音键以外的按键，按 3 个不同的键（比如 主页 / OK / 方向键）。这一步只看按键能不能用，不会执行你以前配置的动作。":
    "Press three different buttons other than the voice button, such as Home, OK, or an arrow key. This step only checks that the buttons work; it does not run your configured actions.",
  "再按 {count} 个不同的普通按键。": "More different regular buttons needed: {count}.",
  "三个按键都收到了，这一项可以继续了。": "All three buttons received. You can continue.",
  "还没有收到按键；按一下遥控器上的任意普通按键开始。":
    "No button received yet. Press any regular button on the remote to begin.",
  "这是语音键": "That is the voice button",
  "请按普通按键（比如主页键或方向键）。":
    "Press a regular button, such as Home or an arrow key.",
  "已按 3 个不同的普通按键": "Three different regular buttons pressed",
  "按键映射暂时暂停，完成后恢复": "Button mapping paused; restored when done",

  // ---- Step 7: complete ----
  "设置完成": "Setup Complete",
  "开始使用前，再确认一次关键条件都还在。":
    "One last check that the key requirements are still in place.",
  "有设置不再可用，修好后再点「开始使用」":
    "A setting is no longer available. Fix it, then select Start Using SayAll.",
  "需要先处理：{label}。": "Fix this first: {label}.",
  "去修复": "Fix It",
  "完成后随时可以在「设置」里重新运行设置向导。":
    "You can rerun the setup wizard from Settings at any time.",
  "遥控器已连接，语音按键可用": "Remote connected, voice button available",
  "语音设备已就绪": "Audio device ready",
  "输入工具已就绪": "Voice app ready",
  "按住说话验证已通过": "Press-and-hold voice test passed",
  "开始使用无线麦": "Start using SayAll",
  "有设置不再可用，回到对应步骤修复后再完成。":
    "A setting is no longer available. Return to the affected step, fix it, then finish.",

  // ---- Side panel ----
  "无线说话，也能随手控制": "Speak wirelessly. Control instantly.",
  "连接检查": "Connection Check",
  "小米蓝牙语音遥控器示意图": "Xiaomi Bluetooth Voice Remote",
  "遥控器已连接": "Remote connected",
  "控制按键已收到": "Button Press Detected",
  "音频检查": "Audio Check",
  "已检测到带「推荐」标记的 CABLE 设备": "Recommended CABLE device detected",
  "已选中推荐设备": "Recommended device selected",
  "音频输出正常": "Audio output ready",
  "实时检查": "Voice Typing Check",
  "语音键已按下并松开": "Voice session started and ended",
  "语音声音已收到": "Voice audio received",
  "文字已经出现": "Text appeared",
  "按键检查": "Button Check",

  // ---- Gate hints ----
  "先连接遥控器，再继续。": "Connect the remote to continue.",
  "按键监听正在启动，请稍等一下。": "Button detection is starting. Wait a moment.",
  "连接已就绪。按一下遥控器上的普通按键（比如主页键），不要按语音键。":
    "The connection is ready. Press a regular button on the remote, such as Home, not the voice button.",
  "完成一次真实的语音上屏测试，再继续。":
    "Complete one real voice-to-text test to continue.",
  "按 3 个不同的普通按键（推荐主页 / OK / 方向键），再继续。":
    "Press three different regular buttons, such as Home, OK, or an arrow key, to continue.",

  // ---- Progress / diagnostics ----
  "进度保存失败，请重试。": "Could not save your progress. Try again.",
  "无法读取设置进度，请重试。": "Could not read setup progress. Try again.",
  "无法更新按键映射状态，请重试。": "Could not update button mapping. Try again.",
  "无法完成设置，请重试。": "Could not finish setup. Try again.",
  "正在整理…": "Preparing…",
  "复制诊断信息": "Copy Setup Diagnostics",
  "已复制，可直接粘贴发给开发者": "Copied. Paste it to the developer.",
  "复制失败，请重试。": "Copy failed; try again",

  // ---- Voice test helpers ----
  "你选的输入工具": "your voice app",
  "未选择": "Not selected",
  "{name}（{state}）": "{name} ({state})",
  "就绪": "ready",
  "未就绪": "not ready",

  // ---- Status row ----
  "待满足": "Pending",
  "进行中": "In progress",
  "需处理": "Needs attention",
} as Record<string, string>;
