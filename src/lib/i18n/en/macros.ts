// English strings for macros. Key = exact Chinese source passed to t().
export default {
  // ===== 页面骨架 / 动作库 =====
  "组合动作": "Routine",
  "组合动作保存后，请到按键页面的按键操作中选择它。":
    "After saving, open the Buttons page and select this routine for a button action.",
  "我的组合动作": "My Routines",
  "搜索组合动作": "Search routines",
  "组合动作列表": "Routine list",
  "{count} 个步骤": "{count} steps",
  "还没有组合动作。创建一个，把多个操作合并成一次执行。":
    "No routines yet. Create one to run several operations together.",
  "新建": "New",
  "导入": "Import",
  "导出": "Export",
  "测试动作": "Test Action",
  "编辑": "Edit",
  "复制": "Copy",
  "删除": "Delete",
  "取消": "Cancel",
  "使用位置": "Used by",
  "还没有按键或组合动作使用它。": "No button or routine uses it.",
  "没有按键或组合动作引用它。": "No button or routine uses it.",
  "按键：{button} · {trigger}": "Button: {button} · {trigger}",
  "{button}·{trigger}": "{button} · {trigger}",
  "、": ", ",
  "；": "; ",
  "嵌套于：{name}": "Nested in: {name}",
  "删除后不可恢复。": "This cannot be undone.",
  "这些按键会改为": "These buttons will be changed to ",
  "未设置": "No action",
  "：{buttons}": ": {buttons}",
  "这些组合动作嵌套引用了它，删除后会把这些嵌套步骤一并移除：{routines}":
    "These routines nest it, so their nested steps will be removed as well: {routines}",
  "创建第一个组合动作": "Create Your First Routine",
  "把打开 App、按键或输入框聚焦组合起来，再设到遥控器按键上。":
    "Combine opening an app, pressing keys, or focusing an input field, then assign it to a remote button.",
  "创建组合动作": "Create Routine",
  "创建后可继续添加步骤并测试执行。":
    "After creating it, add more steps and test the result.",

  // ===== 状态 / 提示（脚本生成） =====
  "组合动作读取失败：{error}": "Could not load routines.",
  "已取消编辑": "Editing cancelled",
  "组合动作已删除": "Routine deleted",
  "已从 {count} 个组合动作中移除它的嵌套步骤":
    "Removed its nested step from {count} routines",
  "一并删除了 {count} 个因此变空的组合动作":
    "Also deleted {count} routines that became empty",
  "{count} 个引用它的按键已改为未设置":
    "{count} buttons that used it were set to No action",
  "删除失败：{error}": "Could not delete the routine.",
  "{name}（副本）": "{name} (copy)",
  "组合动作副本已创建": "Routine copy created",
  "复制失败：{error}": "Could not duplicate the routine.",
  "组合动作已导出": "Routines exported",
  "已取消导出": "Export cancelled",
  "导出失败：{error}": "Could not export routines.",
  "已取消导入": "Import cancelled",
  "组合动作已导入": "Routines imported",
  "导入失败：{error}": "Could not import routines.",
  "最多 {count} 个步骤": "Up to {count} steps",
  "先填写组合动作名称": "Enter a routine name",
  "请先填写组合动作名称": "Enter a routine name",
  "先添加第一个步骤": "Add the first step first",
  "第 {step} 步还没设置完：{issue}": "Step {step} is not set up yet: {issue}",
  "请补全参数": "complete the settings",
  "已删除步骤": "Step deleted",
  "组合键需要至少一个非修饰键；仅按住 Alt/Ctrl 等修饰键暂不支持":
    "A shortcut needs at least one non-modifier key; holding only Alt/Ctrl is not supported",
  "窗口失去焦点，已取消录入":
    "The window lost focus. Recording cancelled.",
  "请先选择一个步骤": "Select a step first",
  "正在录入：请按下一次按键（按 Esc 取消）":
    "Recording: press one key now (press Esc to cancel)",
  "录入已超时，请重新录入": "Recording timed out. Record again.",
  "无法启动快捷键录入：{error}": "Could not start shortcut recording.",
  "已取消录入": "Recording cancelled.",
  "聚焦快捷键已录入：{chord}": "Focus shortcut recorded: {chord}",
  "这个按键暂不支持写入组合动作": "This key cannot be used in a routine yet",
  "快捷键录入成功：{chord}": "Shortcut recorded: {chord}",
  "已选择：{chord}": "Selected: {chord}",
  "已添加应用：{name}": "App added: {name}",
  "添加应用失败：{error}": "Could not add the app.",
  "已添加 {count} 个应用": "Added {count} apps",
  "还没有选择要打开的 App": "No app to open is selected",
  "还没有选择要等待的 App": "No app to wait for is selected",
  "还没有选择或录入按键": "No key recorded",
  "还没有选择系统动作": "No system action selected",
  "还没有选择目标 App": "No target app selected",
  "这个 App 还没有聚焦方式，请选择一个":
    "This app has no focus method yet; choose one",
  "这个 App 的聚焦方式是「只打开 APP」，请选择其他聚焦方式":
    'This app\'s focus method is "Open app only"; choose another focus method',
  "这个 App 的聚焦方式还没配置完成（缺少快捷键或输入框记录）":
    "This app's focus method is not complete yet (missing a shortcut or learned input field)",
  "网址需要以 http:// 或 https:// 开头":
    "The URL must start with http:// or https://",
  "还没有选择要执行的组合动作": "No routine to run is selected",
  "更新聚焦方式失败：{error}": "Could not update the focus method.",
  "聚焦方式已更新": "Focus method updated",
  "已恢复默认聚焦方式": "Default focus method restored",
  "正在打开目标应用…": "Opening the target app…",
  "没能打开目标应用，请确认它已安装后再试。":
    "Could not open the target app. Check that it is installed and try again.",
  "正在等待你把鼠标停在目标输入框上……（6 秒后自动记录）":
    "Move the pointer over the target input field… (recorded automatically after 6 seconds)",
  "已记录位置（窗口大小变化超过 25% 时会拒绝点击，需要重新记录）":
    "Position recorded (if the window size changes by more than 25%, clicking is refused and you must record it again)",
  "记录位置失败：{reason}": "Could not record the position: {reason}",
  "聚焦快捷键已录入": "Focus shortcut recorded",
  "正在等待你点击目标输入框……":
    "Waiting for you to click the target input field…",
  "输入框学习成功。": "Input field learned.",
  "组合动作已开始执行": "Routine started",
  "已取消运行": "Run cancelled",
  "取消失败：{error}": "Could not cancel the run.",
  "至少添加一个步骤": "Add at least one step",
  "组合动作已保存": "Routine saved",
  "保存失败：{error}": "Could not save the routine.",
  "测试失败：{reason}": "Test failed: {reason}",
  "测试失败": "Test failed.",

  // ===== 编辑向导 =====
  "新建组合动作": "New Routine",
  "编辑组合动作": "Edit Routine",
  "基本设置": "Basic settings",
  "编排步骤": "Arrange steps",
  "测试与保存": "Test and save",
  "组合动作名称": "Routine Name",
  "例如：打开 Codex 并聚焦输入框":
    "For example: Open Codex and focus the input field",
  "最多 {count} 个字符。": "Up to {count} characters.",
  "选择第一个动作": "Choose the first action",
  "已添加：": "Added:",
  "已有 {count} 个步骤，可在下一步修改顺序和参数。":
    "{count} steps. Edit their order and settings in the next step.",
  "选择动作后，点击“下一步”配置；名称和步骤在最后一起保存。":
    "Select an action, then click Next to configure it. Save the name and steps together at the end.",
  "未命名": "Untitled",
  "上移步骤": "Move step up",
  "下移步骤": "Move step down",
  "请从下方添加第一个步骤。": "Add the first step below.",
  "添加步骤": "Add Step",
  "步骤 {step}": "Step {step}",
  "测试这步": "Test this step",
  "要使用的 App": "App to Use",
  "扫描本机应用": "Scan Installed Apps",
  "添加应用": "Add App",
  "未运行时会先启动；已运行时直接切换到前台。":
    "Starts the app if it is not running; switches to it if it is.",
  "当前：{name}": "Current: {name}",
  "要等待的 App": "App to Wait For",
  "等待到什么时候": "Wait Until",
  "App 已打开": "App Is Open",
  "App 窗口已出现": "App Window Appears",
  "最多 5 秒；超时后停止执行并提示。":
    "Up to 5 seconds; if it times out, the run stops and you are notified.",
  "已录入": "Recorded",
  "尚未录入按键": "No key recorded",
  "录入中…（点击取消）": "Recording… (click to cancel)",
  "开始录入": "Start Recording",
  "请按一次单键或组合键，按 Esc 取消；15 秒无操作会自动结束。":
    "Press one key or shortcut; press Esc to cancel. Recording ends automatically after 15 seconds of inactivity.",
  "点击“开始录入”后，只捕获下一次完整的单键或组合键，然后立即停止。":
    "After starting, only the next complete key or shortcut is captured, then recording stops.",
  "已保存的快捷键": "Saved Shortcuts",
  "常用快捷键": "Common Shortcuts",
  "把键盘焦点放到": "Move keyboard focus to the ",
  "当前前台应用": "frontmost app",
  "的输入框；不切换应用、不输入文字。需要指定某个 App 时请改用「聚焦学习过的输入框」。":
    "'s input field; it does not switch apps or type text. To target a specific app, use \"Focus Learned Input\" instead.",
  "系统与媒体动作": "System and Media Action",
  "尚未选择系统动作": "No system action selected",
  "该步骤来自旧版本，仍可执行或删除；新步骤请使用「单键或快捷键」。":
    'This step comes from an older version. It can still run or be deleted; use "Key or Shortcut" for new steps.',
  "打开应用": "Open App",
  "聚焦方式": "Focus method",
  "恢复默认": "Restore Default",
  "先点击“录入快捷键”，再按下该 APP 用于聚焦输入框的单个按键或组合键。执行时会先激活 APP，再发送此按键。":
    "Click Record Focus Shortcut first, then press the single key or key combination this app uses to focus its input field. SayAll activates the app before sending it.",
  "尚未录入": "Not recorded yet",
  "录入中…（按 Esc 取消）": "Recording… (Esc to cancel)",
  "点击录入快捷键": "Record Focus Shortcut",
  "清除": "Clear",
  "点击后会立即打开目标 APP。请在 6 秒内点击需要聚焦的输入框；无线麦 SayAll 只记录辅助功能特征，不读取输入内容。":
    "This opens the target app right away. Click the input field to focus within 6 seconds; SayAll records accessibility traits only and never reads what you type.",
  "正在打开应用…": "Opening the app…",
  "等待点击输入框…": "Waiting for you to click the input field…",
  "开始学习输入框": "Learn Input Field",
  "已记录目标输入框": "Input field learned",
  "尚未记录目标输入框": "No input field learned",
  "「记录点击位置」会先打开目标应用；把鼠标停在要点击的位置上，6 秒后自动记录，之后按这个位置点击聚焦。":
    '"Record click position" opens the target app first. Move the pointer over the spot to click; it is recorded automatically after 6 seconds and used to click and focus.',
  "等待鼠标停留…": "Waiting for the pointer…",
  "开始记录位置": "Record Position",
  "清除记录": "Clear Record",
  "已记录点击位置": "Position recorded",
  "尚未记录点击位置": "No position recorded",
  "网址": "Website",
  "输入完整网址，例如 https://www.example.com":
    "Enter a full URL, for example https://www.example.com",
  "会用默认浏览器打开。只能打开普通网页，不能打开电脑里的文件或 App 内的特殊页面。":
    "Opens in your default browser. It can open normal web pages, but not files on this PC or special pages inside apps.",
  "选择组合动作": "Choose Routine",
  "还没有可执行的组合动作；先创建并保存一个。":
    "Create and save another routine first.",
  "这里会使用所选组合动作最后一次保存的内容。组合动作不能直接或间接执行自己，最多嵌套 8 层。":
    "This uses the last saved version of the selected routine. A routine cannot run itself, directly or through another routine; nesting is limited to 8 levels.",
  "请从左侧添加第一个步骤。": "Add the first step from the left.",
  "{count} 个步骤 · 可先测试，再保存。测试不是保存的必要条件。":
    "{count} steps · You can test before saving. Testing is optional.",
  "未测试": "Not tested",
  "取消编辑": "Cancel editing",
  "上一步": "Previous",
  "下一步": "Next",
  "保存修改": "Save Changes",

} as Record<string, string>;
