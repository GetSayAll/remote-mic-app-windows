// English strings for the private Windows button-profiles page.
// Key = exact Chinese source passed to t(). Only this page's own strings live
// here; shared words (上/下/取消/组合动作…) resolve from buttons.ts / lib.ts.
export default {
  // 页面框架
  "键位方案": "Button profiles",
  "正在编辑": "Editing",
  "基础映射": "Basic mapping",
  "当前生效：{name}": "Active: {name}",
  "只读预览": "Read-only preview",
  "管理键位方案": "Manage button profiles",
  "开通会员": "Get membership",
  "在按键页编辑": "Edit on the Buttons page",
  "当前方案只能查看，不能编辑。": "This profile is read-only.",
  "键位方案只支持 10 个普通按键。":
    "Button profiles support only the 10 standard buttons.",
  "键位方案不支持这个开关。":
    "Button profiles do not support this switch.",
  "自动保存成功": "Saved automatically",
  "保存失败，请重试。": "Could not save. Try again.",
  "当前是只读预览：点「编辑」后可修改这些按键。":
    "Read-only preview. Select Edit to change these buttons.",
  "音量-": "Volume Down",
  "关机": "Power",
  "为不同 App 配置 10 个按键的单击、双击和长按动作；默认方案继续使用「按键」页面。":
    "Set single-click, double-click, and long-press actions for each of the 10 buttons per app. The default profile keeps using the Buttons page.",
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
  "默认": "Default",
  "开关机": "Power",
  "默认按键映射": "Default button mapping",
  "正在查看": "Viewing",
  "键位方案市场": "Button profile market",
  "设置前台自动应用": "Set foreground auto-apply",
  "打开后，目标 App 位于前台时使用这套方案。关闭后使用默认方案。":
    "When on, this profile is used while the target app is in the foreground. When off, the default profile is used.",
  "前往按键页面修改": "Go to the Buttons page to edit",
  "展开另外 {count} 个": "Show {count} more",
  "展开 App 列表": "Expand app list",
  "展开全部 App": "Expand all apps",
  "收起全部 App": "Collapse all apps",
  "收起 App 列表": "Show less",
  "重命名方案": "Rename profile",
  "保存名称": "Save name",
  "取消重命名": "Cancel renaming",
  "方案名称不能为空": "Profile name cannot be empty",
  "已重命名为「{name}」": "Renamed to “{name}”",
  "重命名失败：{reason}": "Could not rename: {reason}",
  "{direction} {distance} px": "{direction} {distance} px",
  "App": "App",
  "这个 App 已经关联了其他方案，请先解除原方案的关联。":
    "This app is already linked to another profile. Clear that link first.",
  "已清除 App 关联": "App link cleared",
  "已关联「{name}」": "Linked “{name}”",
  "添加 App...": "Add App...",
  "还没有关联 App": "No app linked",
  "还没有关联 App，先从上方选择一个可用的 App。":
    "No app linked yet. Choose an available app above first.",
  "不可用，请重新选择": "Unavailable — choose again",
  "关联的 App 不可用，请重新选择。":
    "The linked app is unavailable. Choose it again.",
  "打开键位方案市场失败：{reason}": "Could not open the button profile market: {reason}",
  "由「按键」页面管理": "Managed on the Buttons page",
  "没有匹配的方案": "No matching profiles",
  "新建方案": "New profile",
  "专用方案": "App profile",
  "已设置的按键": "Configured buttons",
  "{count} 个按键设置": "{count} button mappings",
  "{count} 个手势设置": "{count} gesture mappings",
  "本方案": "This profile",
  "本方案禁用": "Disabled in this profile",
  "继承": "Inherited",
  "系统动作": "System action",
  "按住": "Hold",
  "只执行一次": "Runs once",
  "未命中时使用默认方案": "Uses the default profile when no app matches",
  "确认删除": "Confirm delete",
  "删除方案「{name}」？": "Delete profile “{name}”?",
  "删除键位方案 “{name}”？": "Delete button profile “{name}”?",
  "删除后无法恢复。": "This cannot be undone.",
  "删除后，此方案的按键配置和 App 关联会一并移除。":
    "This removes the profile's button mappings and app associations.",
  "删除前会自动保存一份快照。": "A snapshot is saved before deleting.",
  "这套方案正在使用；删除后会回到默认方案。":
    "This profile is in use. Deleting it returns to the default profile.",
  "关联的 App": "Linked apps",
  "删除方案": "Delete profile",
  "编辑方案": "Edit profile",
  "还没有专用方案": "No app profiles yet",
  "新建一套方案，为不同 App 配置 10 个按键。默认方案继续使用基础映射。":
    "Create a profile to set 10 buttons per app. The default profile keeps using the basic mappings.",
  "导入…": "Import…",
  "导出…": "Export…",
  "前往按键页面": "Go to the Buttons page",
  "默认方案是「按键」页面的基础映射。":
    "The default profile is the basic mapping from the Buttons page.",
  "这里显示 10 个按键的单击、双击、长按；其他未显示的按键动作仍然有效。":
    "The 10 buttons' single-click, double-click, and long-press actions are shown here. Other button actions still work.",
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
  "不执行": "No action",
  "不可用": "Unavailable",
  "不可用：{reason}": "Unavailable: {reason}",
  "已设置": "Set",
  "键位方案 {index}": "Button profile {index}",
  "{name} 副本": "{name} copy",
  "复制为新的键位方案": "Copy as new button profile",
  "创建新的键位方案": "Create new button profile",
  "开通后可创建新方案": "Membership unlocks new profiles",
  "开通后可复制当前方案": "Membership unlocks profile copies",
  "会把当前的动作与按住方式复制成一份独立方案；之后修改基础映射，这份方案不会跟着变。": "Copies the current actions and hold settings into a separate profile. Later changes to the basic mapping will not affect this profile.",
  "会复制「{name}」的动作与按住方式；关联 App 需要在副本里重新选择。": "Copies “{name}” actions and hold modes. Choose associated Apps again for the copy.",
  "复制方案": "Copy profile",
  "键位方案已复制": "Button profile copied",
  "键位方案已创建，已切换到新方案": "Button profile created and selected",
  "来源 {name}": "Source {name}",
  "来源：{name}": "Source: {name}",
  "覆盖范围": "Coverage",
  "按键方案 {index}": "Button profile {index}",
  "复制键位方案失败，请重试。": "Could not copy the button profile. Try again.",
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
  "保存为键位方案": "Save as button profile",
  "会把当前 10 个按键的单击、双击、长按和按住复制成一份独立方案。":
    "Copies the 10 buttons' single-click, double-click, long-press, and hold settings into a separate profile.",
  "创建方案": "Create profile",
  "按键方案": "Button profile",
  "请填写方案名称。": "Enter a profile name.",
  "键位方案已创建": "Button profile created",
  "创建键位方案失败，请重试。": "Could not create the button profile. Try again.",
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
  "已复制默认方案为「{name}」": 'Copied the default profile as "{name}"',
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
  "解除原关联并保存": "Unlink and save",
  "已关联「{name}」，并解除「{owners}」的原关联":
    "Linked “{name}” and removed the link from {owners}",
  "已启用「{name}」": "Enabled “{name}”",
  "已启用「{name}」，并解除「{owners}」的原关联":
    "Enabled “{name}” and removed the link from {owners}",
  "已创建「{name}」": "Created “{name}”",
  "新建失败：{reason}": "Could not create the profile: {reason}",
  "已导入「{name}」，请选择关联 App":
    "Imported “{name}”. Choose an app to link.",
  "已导入「{name}」": "Imported “{name}”",
  "这个方案关联的 App 在本机没有对应项，导入后请重新选择。":
    "The app linked to this profile has no local match. Choose an app after importing.",
  "这个方案包含多个 App，Windows 只保留第一个。":
    "This profile contains multiple apps. Windows keeps the first one.",
  "来源是 macOS，部分快捷键已转换。":
    "The source is macOS. Some shortcuts were converted.",

  // 动作摘要（actions.ts）
  "聚焦当前输入框": "Focus the current input field",
  "组合动作 {id}": "Routine {id}",
  "执行{name}": "Run {name}",
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
