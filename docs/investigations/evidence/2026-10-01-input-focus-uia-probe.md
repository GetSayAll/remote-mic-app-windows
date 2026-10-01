# 聚焦输入框 UIA 探针原始证据（2026-10-01）

工具：`Testing/probe-uia-focus.ps1`（一次性诊断脚本，只读；只输出控件语义与几何，不输出任何用户文本内容）。
环境：Windows 11 本机，Windows PowerShell 5.1 + .NET UIA（`System.Windows.Automation`），探针进程 Per-Monitor V2 DPI 感知。

> 说明：以下 JSON 为脚本原始输出，仅删去与本功能无关的长样式类字符串；`name_len` 是名称长度而非名称本身，属有意设计（隐私）。

## 1. 记事本（WinUI，窗口最小化/非前台）

```json
{
  "process_name": "Notepad", "process_id": 23888,
  "dpi_awareness": "system_aware_fallback", "was_foreground": false, "foreground": false,
  "scan_attempts": [
    { "attempt": 1, "count": 1, "elapsed_ms": 203, "error": null },
    { "attempt": 2, "count": 1, "elapsed_ms": 34,  "error": null },
    { "attempt": 3, "count": 1, "elapsed_ms": 37,  "error": null }
  ],
  "candidates": [
    { "index": 0, "control_type": "ControlType.Document", "class_name": "RichEditD2DPT",
      "automation_id": "", "is_password": false, "is_enabled": true, "focusable": true,
      "has_focus": false, "value_readonly": false, "name_len": 5,
      "rect_text": "-31991,-31888 1050x646" }
  ],
  "focused": { "control_type": "ControlType.Button", "process_id": 31208, "belongs_to_target": false, "has_focus": true }
}
```

观察：最小化窗口的 `BoundingRectangle` 是离屏值（-31991,-31888），不是有效坐标；候选本身可读。

## 2. Chrome（运行中，非前台窗口）

```json
{
  "process_name": "chrome", "process_id": 31824,
  "dpi_awareness": "per_monitor_v2", "was_foreground": false, "foreground": false,
  "scan_attempts": [
    { "attempt": 1, "count": 2, "elapsed_ms": 81, "error": null },
    { "attempt": 2, "count": 2, "elapsed_ms": 53, "error": null },
    { "attempt": 3, "count": 2, "elapsed_ms": 47, "error": null }
  ],
  "candidates": [
    { "index": 0, "control_type": "ControlType.Edit", "class_name": "OmniboxViewViews",
      "automation_id": "view_1012", "is_password": false, "is_enabled": true, "focusable": true,
      "has_focus": false, "value_readonly": false, "name_len": 6,
      "rect_text": "531,276 1601x36" },
    { "index": 1, "control_type": "ControlType.Document", "class_name": "",
      "automation_id": "RootWebArea", "is_password": false, "is_enabled": true, "focusable": true,
      "has_focus": false, "value_readonly": true, "name_len": 48,
      "rect_text": "240,381 2478x1475" }
  ],
  "focused": { "control_type": "ControlType.Button", "process_id": 31208, "belongs_to_target": false, "has_focus": true }
}
```

观察：地址栏是 `Edit` 且 `value_readonly=false`（**必须靠语义排除**，不能靠只读位）；网页根是 `Document` / `RootWebArea` 且 `value_readonly=true`（Document 的只读位不可作为排除条件）。

## 3. Edge（冷启动新实例，file:// 页面含 `<textarea id="ta" autofocus>`）

```json
{
  "process_name": "msedge", "process_id": 16876,
  "dpi_awareness": "per_monitor_v2", "was_foreground": true, "foreground": true,
  "scan_attempts": [
    { "attempt": 1, "count": 4, "elapsed_ms": 70, "error": null },
    { "attempt": 2, "count": 4, "elapsed_ms": 37, "error": null },
    { "attempt": 3, "count": 4, "elapsed_ms": 29, "error": null }
  ],
  "candidates": [
    { "index": 0, "control_type": "ControlType.Edit", "class_name": "OmniboxViewViews",
      "automation_id": "view_1017", "is_password": false, "is_enabled": true, "focusable": true,
      "has_focus": false, "value_readonly": false, "name_len": 6, "rect_text": "204,72 3420x36" },
    { "index": 1, "control_type": "ControlType.Document", "class_name": "",
      "automation_id": "RootWebArea", "is_password": false, "is_enabled": true, "focusable": true,
      "has_focus": false, "value_readonly": true, "name_len": 18, "rect_text": "6,120 3828x1962" },
    { "index": 2, "control_type": "ControlType.Edit", "class_name": "",
      "automation_id": "ta", "is_password": false, "is_enabled": true, "focusable": true,
      "has_focus": true, "value_readonly": false, "name_len": 0, "rect_text": "18,132 431x146" },
    { "index": 3, "control_type": "ControlType.Edit", "class_name": "Textfield",
      "automation_id": "", "is_password": false, "is_enabled": true, "focusable": false,
      "has_focus": false, "value_readonly": true, "name_len": 0, "rect_text": "nonfinite" }
  ],
  "focused": { "control_type": "ControlType.Edit", "automation_id": "ta", "process_id": 16876,
               "belongs_to_target": true, "has_focus": true }
}
```

观察：HTML `id` 会作为 `AutomationId` 暴露（`ta`）；存在合成元素（`Textfield`，不可聚焦、非有限矩形）必须过滤；冷启动首次 `FindAll` 即拿到完整候选。

## 4. 微信 4.0（Weixin.exe，登录窗口，宽口径扫描）

```json
{
  "process_name": "Weixin", "process_id": 32536, "window_title_len": 2,
  "dpi_awareness": "per_monitor_v2", "was_foreground": true, "foreground": true,
  "scan_attempts": [ { "attempt": 1, "count": 0, "elapsed_ms": 20, "error": null } ],
  "candidates": [],
  "focused": { "control_type": "ControlType.Button", "class_name": "mmui::XOutlineButton",
               "process_id": 32536, "belongs_to_target": true, "has_focus": true },
  "broad": {
    "total_found": 28, "scanned": 28, "truncated": false, "elapsed_ms": 12, "error": null,
    "control_types": { "ControlType.Button": 8, "ControlType.Text": 5, "ControlType.Group": 10,
                       "ControlType.Pane": 1, "ControlType.ToolBar": 1, "ControlType.Custom": 3 },
    "text_pattern_count": 0,
    "text_pattern_elements": [],
    "focusable_count": 6,
    "focusable_elements": [
      { "index": 10, "control_type": "ControlType.Text", "class_name": "mmui::XTextView",
        "automation_id": "login_layout_.login_step_layout.auto_login_step_layout.current_login_nick_name",
        "focusable": true, "rect_text": "1785,1023 270x45" },
      { "index": 11, "control_type": "ControlType.Button", "class_name": "mmui::XOutlineButton",
        "focusable": true, "has_focus": true, "rect_text": "1794,1173 252x54" }
    ]
  }
}
```

观察：微信 4.0 **有** UIA provider（可读到 `mmui::XButton` / `XTextView` 与点分路径的 AutomationId），但登录窗口整棵树 28 个元素里 **没有任何 Edit/Document，也没有任何元素带 TextPattern**。聊天输入框是否暴露需登录后复测。

## 5. SayAll 自身（Tauri / WebView2，非前台）

```json
{
  "process_name": "sayall-windows-app", "process_id": 40980,
  "dpi_awareness": "system_aware_fallback", "was_foreground": false, "foreground": false,
  "scan_attempts": [
    { "attempt": 1, "count": 1, "elapsed_ms": 45, "error": null },
    { "attempt": 2, "count": 1, "elapsed_ms": 19, "error": null },
    { "attempt": 3, "count": 1, "elapsed_ms": 22, "error": null }
  ],
  "candidates": [
    { "index": 0, "control_type": "ControlType.Document", "class_name": "",
      "automation_id": "RootWebArea", "is_password": false, "is_enabled": true, "focusable": true,
      "has_focus": true, "value_readonly": true, "name_len": 10, "rect_text": "2105,242 1650x1080" }
  ]
}
```

观察：WebView2 与 Chrome/Edge 行为一致（`Document` / `RootWebArea`），读数耗时 19–45 ms。

## 6. 微信 4.0（已登录，聊天主窗口）

```json
{
  "process_name": "Weixin", "process_id": 13748,
  "minimized": false, "cloaked": false, "foreground": false,
  "scan_attempts": [
    { "attempt": 1, "count": 0, "elapsed_ms": 12, "error": null },
    { "attempt": 2, "count": 0, "elapsed_ms": 1,  "error": null },
    { "attempt": 3, "count": 0, "elapsed_ms": 1,  "error": null }
  ],
  "candidates": [],
  "broad": { "total_found": 2, "control_types": { "ControlType.Pane": 2 },
             "text_pattern_count": 0, "focusable_count": 0 },
  "windows": [ "hwnd=0x7117E|pid=13748|visible=True|class=Qt51514QWindowIcon|title=微信",
               "child=0x71148|visible=True|class=MMUIRenderSubWindowHW" ]
}
```

补充：直接探子窗口 `MMUIRenderSubWindowHW` 得到 0 个元素；`powershell -Mta` 结果不变；`WM_NULL` 有响应。
对照登录窗口（第一轮）：28 个元素、`mmui::XButton` / `XTextView`、点分 AutomationId、0 TextPattern。

## 7. WorkBuddy（Electron 37.10.3，腾讯）与 DimAgent（Electron，正例）

```json
// WorkBuddy: 树为空
{ "process_name": "WorkBuddy", "process_id": 27992, "minimized": false, "cloaked": false,
  "scan_attempts": [ { "attempt": 1, "count": 0, "elapsed_ms": 15 }, { "attempt": 8, "count": 0, "elapsed_ms": 3 } ],
  "candidates": [],
  "broad": { "total_found": 2, "control_types": { "ControlType.Pane": 2 },
             "text_pattern_count": 0, "focusable_count": 0 },
  "wake": { "attempted": 4, "answered": 4 },
  "windows": [ "hwnd=0x2CE0730|class=Chrome_WidgetWin_1|visible=True",
               "child=0x99A0F6A|class=Chrome_RenderWidgetHostHWND|visible=True" ] }

// DimAgent: 树可用（对照）
{ "process_name": "DimAgent", "process_id": 10736,
  "scan_attempts": [ { "attempt": 1, "count": 1, "elapsed_ms": 38 } ],
  "candidates": [ { "control_type": "ControlType.Document", "automation_id": "RootWebArea" } ],
  "broad": { "total_found": 685, "text_pattern_count": 120, "focusable_count": 288 } }
```

无效尝试记录（WorkBuddy）：探子窗口 `Chrome_RenderWidgetHostHWND`（0 元素）、`WM_GETOBJECT(UiaRootObjectId)` 唤醒（4/4 送达但树不建立）、置前台、`powershell -Mta`、`WM_NULL` 响应正常（未卡死）。

## 复现命令

```powershell
# 只读探测（不激活目标窗口）
powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-uia-focus.ps1 -ProcessName notepad
# 宽口径扫描（控制类型直方图 + TextPattern / 可聚焦元素）
powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-uia-focus.ps1 -ProcessName Weixin -Broad
# 指定窗口/枚举窗口/子窗口（MainWindowHandle 不是真实 UI 窗口时）
powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-uia-focus.ps1 -ProcessName WorkBuddy -ListWindows
powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-uia-focus.ps1 -WindowHandle 0x2CE0730 -ListChildren
# 唤醒按需构建的无障碍树（WM_GETOBJECT(UiaRootObjectId)）后再扫描
powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-uia-focus.ps1 -WindowHandle 0x2CE0730 -Wake -Broad
# 用 MTA 线程复核（产品路径是 MTA；PowerShell 默认 STA）
powershell -Mta -NoProfile -ExecutionPolicy Bypass -File Testing\probe-uia-focus.ps1 -WindowHandle 0x2CE0730 -Broad
# 激活并验证 SetFocus 读回（会抢前台，慎用）
powershell -NoProfile -ExecutionPolicy Bypass -File Testing\probe-uia-focus.ps1 -ProcessName msedge -Activate -SetFocus
```
