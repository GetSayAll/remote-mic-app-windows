// English strings for permissions. Key = exact Chinese source passed to t().
export default {
  // 页面标题
  "权限": "Permissions",

  // 权限状态
  "语音链路已就绪": "Voice ready",
  "待验证": "Pending verification",
  "当前电脑不支持": "Not supported on this PC",
  "已运行": "Running",
  "已就绪": "Ready",
  "待选择设备": "Select a device",

  // 权限列表
  "蓝牙": "Bluetooth",
  "读取已配对的遥控器并建立连接。": "Read paired remotes and establish a connection.",
  "按键监听与映射": "Button Listening & Mapping",
  "只监听小米遥控器的按键；只有你为按键配置了动作时，才会向电脑发送按键。":
    "Only listens to Xiaomi remote buttons. Keys are sent to the PC only when you have assigned an action to a button.",
  "音频设备": "Audio Device",
  "语音设备由你明确选择，不改动系统默认设备。":
    "You explicitly choose the voice device. System default devices are not changed.",

  // 诊断摘要
  "诊断摘要": "Diagnostic Summary",
  "摘要不含设备地址、语音内容等隐私信息，可放心复制发给开发者排查问题。":
    "The summary contains no device addresses, voice content, or other private information. You can safely copy it to a developer for troubleshooting.",
  "生成中…": "Generating…",
  "生成摘要": "Generate Summary",
  "复制摘要": "Copy Summary",
  "正在打开…": "Opening…",
  "打开日志目录": "Open Log Folder",
  "日志记录本机运行细节，遇到问题时连同摘要一起发给开发者。":
    "Logs record local runtime details. If you run into a problem, send them to a developer together with the summary.",
  "尚未生成诊断摘要": "No diagnostic summary has been generated yet",
  "正在读取当前运行状态…": "Reading the current app status…",
  "诊断摘要已生成；复制前可在页面内检查全部内容":
    "Diagnostic summary generated. Review everything on this page before copying.",
  "当前环境不支持剪贴板写入。": "Clipboard writing is not supported in this environment.",
  "诊断摘要已复制到剪贴板": "Diagnostic summary copied to the clipboard.",
  "正在打开日志目录…": "Opening the log folder…",
  "已打开日志目录：{directory}": "Log folder opened: {directory}",

  // 失败文案（原始错误只进日志）
  "无法生成诊断摘要，请稍后重试。": "Couldn't generate the diagnostic summary. Please try again later.",
  "无法复制诊断摘要，请稍后重试。": "Couldn't copy the diagnostic summary. Please try again later.",
  "无法打开日志目录，请稍后重试。": "Couldn't open the log folder. Please try again later.",
} as Record<string, string>;
