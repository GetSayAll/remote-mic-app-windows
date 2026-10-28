// English strings for common. Key = exact Chinese source passed to t().
export default {
  // 产品名
  "无线麦 SayAll": "SayAll",
  "设置页面": "Page navigation",
  // 通用动作
  "取消": "Cancel",
  "关闭": "Close",
  "好": "OK",
  "查看": "View",
  "忽略此提醒": "Dismiss this notice",
  // 全按键支持确认弹窗
  "开启“全按键支持”？": "Enable Full-key support?",
  "开启后，无线麦会直接读取遥控器上已配置的按键，避免影响物理键盘上的同名按键；返回 / 音量+ / 音量−也需要开启后才能使用。每次开启都会弹出 Windows 授权窗口，请选择“是”；关闭开关即停。":
    "When on, SayAll reads this remote's configured keys directly, so they do not affect same-named keys on a physical keyboard. Back, Volume +, and Volume − also require this setting. Every time you turn it on, Windows asks for authorization—choose Yes. Turn the switch off to stop.",
  "个别杀毒软件可能把这个功能当成风险，拦截或关闭它，导致按键失灵、开启失败。":
    "Some antivirus apps may treat this feature as a risk and block or turn it off, which can make the keys stop working or prevent it from turning on.",
  "个别游戏带有防作弊保护，可能和这个功能合不来：表现为按键失灵、游戏打不开等。玩这类游戏前，建议先把全按键支持关掉。":
    "Some games use anti-cheat protection that may conflict with this feature, causing keys to stop working or the game to fail to open. Turn off Full-key support before playing these games.",
  "无线麦只读取这个遥控器的按键，不收集、不上传任何其他信息。随时可以关闭，关闭后一切恢复原样。":
    "SayAll only reads this remote's keys. It does not collect or upload any other information. You can turn it off at any time; everything returns to normal.",
  "开启": "Enable",
  // 电量指示与会员锁标
  "电量未知": "Battery level unknown",
  "遥控器未连接": "Remote not connected",
  "暂时读不到遥控器电量": "Battery level is temporarily unavailable",
  "遥控器电量 {level}%": "Remote battery {level}%",
  "需要 Plus 会员": "Requires Plus membership",
  // 主界面
  "无法读取运行状态，请稍后重试。": "Couldn't read the app status. Please try again later.",
  "发现新版本 {version}": "Update available: {version}",
  // 更新完成提示（对齐 Mac update.completed.*）
  "无线麦已更新": "SayAll Updated",
  "已成功更新到版本 {version}。": "Successfully updated to version {version}.",
  // 本机应用选择器
  "本机应用": "Installed Apps",
  "关闭应用选择": "Close app picker",
  "搜索本机应用": "Search installed apps",
  "搜索应用": "Search apps",
  "重新扫描": "Scan Again",
  "全选当前结果": "Select all results",
  "{count} 个应用 · 已选 {selected}": "{count} apps · {selected} selected",
  "正在扫描…": "Scanning…",
  "未找到匹配的应用": "No matching apps found",
  "已添加": "Added",
  "正在添加…": "Adding…",
  "添加所选": "Add Selected",
  "（{count}）": " ({count})",
  "无法读取本机应用列表，请稍后重试。": "Couldn't read the list of installed apps. Please try again later.",
} as Record<string, string>;
