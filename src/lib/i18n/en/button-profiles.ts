// English strings for the private Windows button-profiles page.
// Key = exact Chinese source passed to t(). Only this page's own strings live
// here; shared words (上/下/取消/组合动作…) resolve from buttons.ts / lib.ts.
export default {
  // 页面框架
  "键位方案": "Button profiles",
  "音量-": "Volume Down",
  "关机": "Power",
  "为不同 App 配置 10 个按键的单击动作；默认方案继续使用「按键」页面。":
    "Set a single-click action for each of the 10 buttons per app. The default profile keeps using the Buttons page.",
  "离线使用中": "Offline",
  "设计稿状态切换": "Design state switch",
  "可用": "Available",
  "Plus 锁定": "Plus locked",
  "空态": "Empty",
  "正在读取键位方案…": "Reading button profiles…",
  "键位方案组件不可用": "Button profiles component unavailable",
  "当前安装包没有包含键位方案组件；基础按键行为不受影响。":
    "This build does not include the button profiles component; basic button behavior is unaffected.",
  "无法读取键位方案": "Cannot read button profiles",
  "键位方案组件未安装或不可用。":
    "The button profiles component is not installed or unavailable.",
  "读取键位方案失败：{reason}": "Could not read button profiles: {reason}",

  // 方案列表
  "我的键位方案": "My button profiles",
  "搜索键位方案": "Search button profiles",
  "正在使用": "In use",
  "默认方案": "Default profile",
  "由「按键」页面管理": "Managed on the Buttons page",
  "没有匹配的方案": "No matching profiles",
  "新建方案": "New profile",
  "专用方案": "App profile",
  "已设置的按键": "Configured buttons",
  "{count} 个按键设置": "{count} button mappings",
  "未命中时使用默认方案": "Uses the default profile when no app matches",
  "确认删除": "Confirm delete",
  "编辑方案": "Edit profile",
  "还没有专用方案": "No app profiles yet",
  "新建一套方案，为不同 App 配置 10 个按键。默认方案继续使用基础映射。":
    "Create a profile to set 10 buttons per app. The default profile keeps using the basic mappings.",
  "前往按键页面": "Go to the Buttons page",
  "默认方案是「按键」页面的基础映射。":
    "The default profile is the basic mapping from the Buttons page.",
  "这里只显示 10 个按键的单击动作；其他未显示的按键动作仍然有效。":
    "Only the 10 buttons' single-click actions are shown here. Other button actions still work.",
  "修改默认动作请前往「按键」页面。":
    "To change default actions, go to the Buttons page.",

  // 方案元信息与摘要
  "未关联": "None",
  "未关联 App": "No app linked",
  "这个 App": "This app",
  "{app} · 自动应用已开启": "{app} · Auto-apply on",
  "{app} · 自动应用已关闭": "{app} · Auto-apply off",
  "沿用默认 · {summary}": "Default · {summary}",
  "沿用默认": "Use default",
  "不执行操作": "Do nothing",
  "不执行": "Off",
  "不可用": "Unavailable",
  "不可用：{reason}": "Unavailable: {reason}",
  "已设置": "Set",
  "键位方案 {index}": "Button profile {index}",
  "{name} 副本": "{name} copy",
  "WPS 演示": "WPS Presentation",
  "计算器": "Calculator",
  "文件资源管理器": "File Explorer",

  // 新建 / 编辑向导
  "编辑「{name}」": 'Edit "{name}"',
  "新建键位方案": "New button profile",
  "所有修改先进入草稿；最后一步才写入。":
    "All changes stay in a draft until you save on the last step.",
  "设置按键": "Set buttons",
  "确认保存": "Confirm and save",
  "方案名称": "Profile name",
  "例如：网易云音乐": "For example: NetEase Cloud Music",
  "关联 App": "Linked app",
  "扫描本机应用": "Scan Installed Apps",
  "＋ 添加应用": "+ Add App",
  "清除关联": "Clear link",
  "已选择：{name}": "Selected: {name}",
  "已开启": "On",
  "已关闭": "Off",
  "前台自动应用": "Auto-apply in foreground",
  "只有关联的 App 位于前台时才使用这套方案。":
    "This profile is used only when the linked app is in the foreground.",
  "开启自动应用前必须关联一个 App。": "Link an app before turning on auto-apply.",
  "设置 10 个通用键": "Set up 10 buttons",
  "当前按键：{name}": "Current button: {name}",
  "按下后会": "Pressing it will",
  "这个按键在 Windows 上不可用": "This button is unavailable on Windows",
  "{reason}。请重新选择一个动作，或改成「不执行操作」。":
    "{reason}. Choose another action, or switch to Do nothing.",
  "这个按键继续使用「按键」页面里的基础映射；只有在本方案里明确改过的按键才会覆盖它。":
    "This button keeps the basic mapping from the Buttons page. Only buttons changed in this profile override it.",
  "当前基础动作：{summary}": "Current default action: {summary}",
  "按下这个键时将不会执行任何动作；这不会修改「按键」页面的基础映射。":
    "Pressing this button does nothing. This does not change the basic mappings on the Buttons page.",

  // 快捷键录入
  "取消录入": "Cancel recording",
  "录入快捷键": "Record shortcut",
  "请按键…": "Press a key…",
  "支持修饰键": "Modifier keys supported",
  "已取消录入快捷键": "Shortcut recording cancelled",
  "无法录入快捷键：{reason}": "Could not record the shortcut: {reason}",
  "重复执行": "Hold to repeat",
  "先选择一个基础按键或鼠标动作。":
    "Choose a basic button or a mouse action first.",
  "按住会重复执行当前动作，直到松手。":
    "Holding repeats this action until you release.",
  "当前动作只执行一次，不能连续执行。":
    "This action runs once and cannot repeat.",

  // 鼠标动作
  "点击": "Click",
  "移动": "Move",
  "滚轮": "Wheel",
  "向上移动": "Move up",
  "向下移动": "Move down",
  "向左移动": "Move left",
  "向右移动": "Move right",
  "每次{kind}数量": "{kind} amount",
  "没有可用的 App。": "No apps available.",
  "还没有组合动作。": "No routines yet.",
  "选择一个分类查看动作。": "Choose a category to see actions.",

  // 确认页
  "保存方案": "Save profile",
  "名称": "Name",

  // 导入预览
  "导入预览：{name}": "Import preview: {name}",
  "保存前不会修改本机配置。已转换的快捷键仍建议在目标 App 中验证。":
    "Nothing on this PC changes until you save. Verify converted shortcuts in the target app.",
  "Windows 备份": "Windows backup",
  "{count} 个按键": "{count} buttons",
  "{count} 项可用": "{count} usable",
  "{count} 项需确认": "{count} need review",
  "来源：{source}": "Source: {source}",
  "未知": "Unknown",
  "已转换": "Converted",
  "原样导入": "Imported as is",
  "已忽略": "Ignored",
  "错误": "Error",
  "已转换为 Windows 等价键": "Converted to a Windows equivalent",
  "未找到 Windows 等价快捷键": "No Windows equivalent shortcut found",

  // 状态提示
  "已保存「{name}」": 'Saved "{name}"',
  "已保存「{name}」，并替换了「{owners}」的 App 关联":
    'Saved "{name}"; the app link moved from "{owners}".',
  "已删除「{name}」": 'Deleted "{name}"',
  "已复制为「{name}」": 'Copied as "{name}"',
  "已复制为「{name}」，请选择关联 App":
    'Copied as "{name}". Choose an app to link.',
  "已导出「{name}」": 'Exported "{name}"',
  "已导入方案": "Profile imported",
  "保存失败：{reason}": "Could not save: {reason}",
  "删除失败：{reason}": "Could not delete: {reason}",
  "复制失败：{reason}": "Could not copy: {reason}",
  "切换失败：{reason}": "Could not switch: {reason}",
  "导出失败：{reason}": "Could not export: {reason}",
  "导入失败：{reason}": "Could not import: {reason}",
  "读取导入文件失败：{reason}": "Could not read the import file: {reason}",
  "添加应用失败：{reason}": "Could not add the app: {reason}",
  "还没有关联APP，请点击编辑方案选择APP。":
    "No app linked yet. Edit the profile and choose an app.",
  "这个 App 已经关联了其他方案，请替换关联后再保存。":
    "This app is already linked to another profile. Replace the link before saving.",
  "关联的 App 启动路径无效，请重新选择本机应用后再保存。":
    "The linked app's launch path is invalid. Choose the app again before saving.",

  // 替换 App 关联确认
  "这个 App 已经关联了其他方案":
    "This app is already linked to another profile",
  "「{app}」这个 App 已关联到方案「{owners}」。继续保存会把它改到当前方案「{name}」；原方案将不再关联这个 App。":
    "{app} is already linked to the profile {owners}. Saving moves it to the current profile {name}; the original profile will no longer be linked to this app.",
  "保存中…": "Saving…",
  "替换并保存": "Replace and save",

  // 动作摘要（actions.ts）
  "聚焦当前输入框": "Focus the current input field",
  "组合动作 {id}": "Routine {id}",
  "鼠标{label}": "Mouse: {label}",
  "鼠标向{direction}移动 {distance} 像素": "Mouse: {direction} {distance} px",
  "滚轮向{direction}滚动 {steps} 格": "Wheel: {direction} {steps} steps",
  "重复：{summary}": "Repeat: {summary}",
  "鼠标点击": "Mouse click",
  "移动光标": "Move pointer",
  "滚动": "Scroll",
  "这个快捷键在 Windows 上没有对应按键":
    "This shortcut has no matching key on Windows",
  "原方案的快捷键定义缺失": "The original profile has no shortcut definition",
  "这个组合动作无法转换成 Windows 动作":
    "This routine cannot be converted to a Windows action",
  "原方案的组合动作定义缺失": "The original profile has no routine definition",
  "原方案关联的 App 在本机找不到":
    "The app linked to the original profile is not on this PC",
  "这个系统动作在 Windows 上不支持":
    "This system action is not supported on Windows",
  "这个按键在 Windows 上没有等价动作":
    "This button has no equivalent action on Windows",
  "OK": "OK",

  // 演示数据里的应用显示名
  "网易云音乐": "NetEase Cloud Music",
  "Chrome 浏览器": "Chrome",
  "微信": "WeChat",
  "记事本": "Notepad",
  "Visual Studio Code": "Visual Studio Code",
} as Record<string, string>;
