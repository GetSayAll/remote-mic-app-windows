// English strings for settings. Key = exact Chinese source passed to t().
export default {
  // 顶部概览
  "无线麦 SayAll 应用图标": "SayAll app icon",
  "让语音触手可及": "Voice within reach",
  "当前版本": "Current Version",
  "可更新": "Update Available",
  "检查预览版更新": "Check for pre-release updates",
  "开启后，检查更新时也会包含尚在测试中的预览版本。":
    "Off by default. When enabled, update checks also include preview versions that are still in testing.",
  "重试检查": "Check Again",
  "检查更新…": "Check for Updates…",
  "下载并安装": "Download and Install",
  "发现新版本": "Update available:",
  "（当前 {current}）": " (current {current})",
  "正在安装更新，应用将自动重启…": "Installing the update. The app will restart automatically…",
  "正在下载更新… {progress}": "Downloading update… {progress}",
  "正在检查更新…": "Checking for updates…",
  "已经是最新版本。": "You're up to date.",
  "检查更新失败，请稍后重试。": "Couldn't check for updates. Please try again later.",
  "手动检查是否有新版本。": "Check for a new version manually.",
  "无法读取或保存更新设置，请稍后重试。":
    "Couldn't read or save the update setting. Please try again later.",

  // 通用
  "通用": "General",
  "外观": "Appearance",
  "系统": "System",
  "浅色": "Light",
  "深色": "Dark",
  "显示模式": "Display mode",
  "启动行为": "Launch Behavior",
  "登录时自动启动": "Launch at login",
  "使用当前用户的 Windows 登录启动项，不需要管理员权限。":
    "Uses the current user's Windows startup entry. Administrator rights are not required.",
  "启动时自动打开主面板": "Open main window at launch",
  "关闭后普通启动仅保留托盘入口；更新完成后仍会显示主面板。":
    "When off, ordinary launches only keep the tray entry; the main window still appears after an update.",
  "应用语言": "App Language",
  "跟随系统": "System Default",
  // 语言名一律用原语言写法：中文项不翻译成英文名。
  "简体中文": "简体中文",
  "English": "English",
  "应用图标": "App Icon",
  "默认": "Default",
  "几何鸭": "Faceted Duck",

  // 会员与账号
  "会员与账号": "Membership & Account",
  "已登录账号": "Signed In",
  "未登录": "Not Signed In",
  "Plus 已开通，权益已同步。": "Plus is active and your benefits are in sync.",
  "Plus 已开通，正在使用离线租约。": "Plus is active using an offline lease.",
  "当前账号未开通 Plus；开通后可解锁会员功能。":
    "This account doesn't have Plus. Subscribe to unlock membership features.",
  "登录账号后可以开通或同步 Plus。": "Sign in to subscribe to or sync Plus.",
  "管理会员": "Manage Membership",
  "开通 Plus 会员": "Subscribe to Plus",
  "登录并开通 Plus": "Sign In and Subscribe to Plus",

  // 首次设置
  "首次设置": "First-Time Setup",
  "重新运行设置向导": "Run Setup Guide Again",
  "重新走一遍首次设置；不会清除按键映射、设备与语音设置。":
    "Walk through first-time setup again. Button mappings, devices, and voice settings are not cleared.",
  "重新运行向导": "Run Again",
  "无法开始设置向导，请稍后重试。": "Couldn't start setup. Please try again later.",

  // 问题反馈
  "问题反馈": "Feedback",
  "反馈无线麦 SayAll 的问题，或提出改进建议。":
    "Report a problem or suggest an improvement for SayAll.",
  "官网": "Website",

  // 设置保存失败（IPC 原始错误只进日志）
  "无法保存登录启动设置，请稍后重试。":
    "Couldn't save the launch-at-login setting. Please try again later.",
  "无法保存启动页面设置，请稍后重试。":
    "Couldn't save the launch window setting. Please try again later.",
  "无法保存应用图标设置，请稍后重试。":
    "Couldn't save the app icon setting. Please try again later.",

  // 设置读取失败（IPC 原始错误只进日志）
  "无法读取登录启动设置，请稍后重试。":
    "Couldn't read the launch-at-login setting. Please try again later.",
  "无法读取启动页面设置，请稍后重试。":
    "Couldn't read the launch window setting. Please try again later.",
  "无法读取应用图标设置，请稍后重试。":
    "Couldn't read the app icon setting. Please try again later.",

  // 外观/语言错误来源在 src/lib/theme.ts 与 src/lib/language.ts，展示在本页
  "无法读取外观设置，已暂时使用上次的选择。":
    "Couldn't read the appearance setting. Using your last choice for now.",
  "外观设置保存失败，请稍后重试。": "Couldn't save the appearance setting. Please try again later.",
  "无法读取应用语言设置，已暂时使用上次的选择。":
    "Couldn't read the app language setting. Using your last choice for now.",
  "应用语言保存失败，请稍后重试。": "Couldn't save the app language setting. Please try again later.",

  // 会员入口按钮默认文案
  "点击开通会员功能": "Unlock membership feature",
} as Record<string, string>;
