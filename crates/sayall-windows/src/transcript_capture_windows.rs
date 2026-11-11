//! Windows UIA 回眸基线/轮询读取。
//!
//! 只读 UI Automation 公开接口：取系统焦点元素、检查安全输入区域、读取 Value/Text
//! 模式和完整文字。文字只在内存中交给捕获状态机做差异，不写日志、不进 IPC。
//! 每次 UIA 调用都有有界等待；目标 provider 卡住时本次捕获放弃，不阻塞语音线程。

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use sayall_core::transcripts::application_key;
use windows::core::{Interface, BSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationValuePattern,
    UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_GroupControlTypeId, UIA_TextPatternId,
    UIA_ValuePatternId,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_DISPLAYNAME};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetForegroundWindow, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO,
};

use crate::focus_windows::with_automation_timeout;
use crate::transcript_capture::{CaptureSnapshot, CaptureText, SnapshotError, SnapshotProvider};

const MAX_TEXT_CHARS: usize = 1_000_000;
const MAX_FOCUS_ID_CHARS: usize = 256;

/// 图标缓存与回眸界面统一使用的像素边长。
const APPLICATION_ICON_PIXELS: i32 = 64;

/// 不应记录文字的语义词。比「聚焦输入框」的排除词更严格：搜索框、地址栏、
/// 设置类输入区域一律不记录，避免把用户搜索词/密码/配置写进回眸。
const UNSAFE_TERMS: &[&str] = &[
    "password",
    "passcode",
    "secret",
    "api key",
    "apikey",
    "token",
    "credit card",
    "search",
    "find",
    "filter",
    "address bar",
    "omnibox",
    "settings",
    "preferences",
    "command palette",
    "rename",
    "密码",
    "口令",
    "密钥",
    "令牌",
    "信用卡",
    "搜索",
    "查找",
    "筛选",
    "地址",
    "设置",
    "偏好",
    "命令面板",
    "重命名",
];

#[derive(Debug, Clone)]
pub struct WindowsSnapshotProvider {
    timeout: Duration,
}

impl Default for WindowsSnapshotProvider {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(200),
        }
    }
}

impl WindowsSnapshotProvider {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl SnapshotProvider for WindowsSnapshotProvider {
    fn snapshot(&self) -> Result<CaptureSnapshot, SnapshotError> {
        let foreground = unsafe { GetForegroundWindow() };
        if foreground.0.is_null() {
            return Err(SnapshotError::NoForeground);
        }
        let mut foreground_pid = 0u32;
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                foreground,
                Some(&mut foreground_pid),
            )
        };
        if foreground_pid == 0 {
            return Err(SnapshotError::NoForeground);
        }
        if foreground_pid == std::process::id() {
            return Err(SnapshotError::SelfForegroundWindow);
        }

        let result = with_automation_timeout(self.timeout, |automation| {
            let element =
                unsafe { automation.GetFocusedElement() }.map_err(|_| SnapshotError::NoFocus)?;
            let pid =
                unsafe { element.CurrentProcessId() }.map_err(|_| SnapshotError::NoFocus)? as u32;
            if pid == std::process::id() {
                return Err(SnapshotError::SelfFocusedElement);
            }
            let focus = read_focus(&element)?;
            let text = read_text(&element)?;
            let application = application_identity(pid);
            Ok(CaptureSnapshot {
                focus_id: focus.focus_id,
                application_key: application.0,
                application_name: application.1,
                text: text.0,
                selection_text: text.1,
                icon_png: application.2,
            })
        });
        result.unwrap_or(Err(SnapshotError::UiaTimeout))
    }

    fn read(&self, focus_id: &str) -> Result<CaptureText, SnapshotError> {
        let expected_focus = focus_id.to_owned();
        let result = with_automation_timeout(self.timeout, move |automation| {
            let element = unsafe { automation.GetFocusedElement() }
                .map_err(|_| SnapshotError::FocusChanged)?;
            let focus = read_focus(&element)?;
            if focus.focus_id != expected_focus {
                return Err(SnapshotError::FocusChanged);
            }
            let text = read_text(&element)?;
            Ok(CaptureText {
                focus_id: focus.focus_id,
                text: text.0,
                selection_text: text.1,
                safe: true,
            })
        });
        result.unwrap_or(Err(SnapshotError::UiaTimeout))
    }
}

struct FocusInfo {
    focus_id: String,
}

fn read_focus(element: &IUIAutomationElement) -> Result<FocusInfo, SnapshotError> {
    let pid = unsafe { element.CurrentProcessId() }.map_err(|_| SnapshotError::NoFocus)? as u32;
    let control_type =
        unsafe { element.CurrentControlType() }.map_err(|_| SnapshotError::NoFocus)?;
    let automation_id = bstr(unsafe { element.CurrentAutomationId() });
    let class_name = bstr(unsafe { element.CurrentClassName() });
    let name = bstr(unsafe { element.CurrentName() });
    let is_password = unsafe { element.CurrentIsPassword() }
        .map(|value| value.as_bool())
        .unwrap_or(false);
    let enabled = unsafe { element.CurrentIsEnabled() }
        .map(|value| value.as_bool())
        .unwrap_or(true);
    let focused = unsafe { element.CurrentHasKeyboardFocus() }
        .map(|value| value.as_bool())
        .unwrap_or(true);

    if is_password {
        return Err(SnapshotError::PasswordField);
    }
    if !enabled {
        return Err(SnapshotError::TargetDisabled);
    }
    if !focused {
        return Err(SnapshotError::NoFocus);
    }
    if control_type != UIA_EditControlTypeId
        && control_type != UIA_DocumentControlTypeId
        && control_type != UIA_GroupControlTypeId
    {
        return Err(SnapshotError::UnsupportedControlType);
    }
    let semantics = format!("{automation_id} {class_name} {name}").to_lowercase();
    if UNSAFE_TERMS.iter().any(|term| semantics.contains(term)) {
        return Err(SnapshotError::UnsafeSemantics);
    }
    let focus_id = format!("{pid}:{}:{automation_id}:{class_name}", control_type.0)
        .chars()
        .take(MAX_FOCUS_ID_CHARS)
        .collect();
    Ok(FocusInfo { focus_id })
}

/// 返回 `(全文, 选区文字)`。选区文字只用于诊断，不落盘。
fn read_text(element: &IUIAutomationElement) -> Result<(String, Option<String>), SnapshotError> {
    let value_pattern: Option<IUIAutomationValuePattern> = unsafe {
        element
            .GetCurrentPattern(UIA_ValuePatternId)
            .ok()
            .and_then(|pattern| pattern.cast().ok())
    };
    let text_pattern: Option<IUIAutomationTextPattern> = unsafe {
        element
            .GetCurrentPattern(UIA_TextPatternId)
            .ok()
            .and_then(|pattern| pattern.cast().ok())
    };
    if value_pattern.is_none() && text_pattern.is_none() {
        return Err(SnapshotError::NoTextPattern);
    }

    if let Some(pattern) = value_pattern.as_ref() {
        let read_only = unsafe { pattern.CurrentIsReadOnly() }
            .map(|value| value.as_bool())
            .unwrap_or(false);
        if read_only {
            return Err(SnapshotError::ReadOnly);
        }
    }

    let value = value_pattern
        .as_ref()
        .and_then(|pattern| unsafe { pattern.CurrentValue() }.ok())
        .map(|value| value.to_string());
    let mut selection_text = None;
    let mut text_value = None;
    if let Some(pattern) = text_pattern.as_ref() {
        if let Ok(range) = unsafe { pattern.DocumentRange() } {
            if let Ok(text) = unsafe { range.GetText(-1) } {
                text_value = Some(text.to_string());
            }
        }
        if let Ok(selection) = unsafe { pattern.GetSelection() } {
            let count = unsafe { selection.Length() }.unwrap_or(0);
            if count > 0 {
                if let Ok(range) = unsafe { selection.GetElement(0) } {
                    selection_text = unsafe { range.GetText(-1) }
                        .ok()
                        .map(|text| text.to_string());
                }
            } else {
                selection_text = Some(String::new());
            }
        }
    }

    let text = match (value, text_value) {
        (Some(value), Some(text)) if value.is_empty() && !text.is_empty() => text,
        (Some(value), _) => value,
        (None, Some(text)) => text,
        (None, None) => return Err(SnapshotError::NoTextPattern),
    };
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(SnapshotError::TooLarge);
    }
    Ok((text, selection_text))
}

/// 语音开始时按前台窗口取应用身份，不依赖 UIA。WorkBuddy / 微信等窗口没有可读
/// 文本 provider 时，文字捕获会跳过，但录音仍需要应用名与稳定键来生成“仅录音”条目。
pub fn foreground_application_identity() -> Option<(String, String)> {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.0.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
            foreground,
            Some(&mut pid),
        )
    };
    application_identity_for_pid(pid)
}

/// 录音 manifest 的应用身份。
///
/// 前台是本应用时也要给出身份，不能返回 `None`：首次设置向导的语音测试与
/// 设置页试音都发生在无线麦自己的窗口里，缺身份会让 manifest 没有应用名，
/// 回眸只能显示「未知应用」（2026-11-06 用户报告）。个人中心统计已按
/// 2026-10-25 的产品规则把「前台是本应用」显示为 `SayAll`，这里保持一致。
fn application_identity_for_pid(pid: u32) -> Option<(String, String)> {
    if pid == 0 {
        return None;
    }
    if pid == std::process::id() {
        return Some((
            SELF_APPLICATION_NAME.to_owned(),
            SELF_APPLICATION_NAME.to_owned(),
        ));
    }
    // 录音只需要应用键与显示名；不要在 BLE 线程上顺带提取图标。
    let (key, name, _) = application_identity_parts(pid);
    Some((key, name))
}

const SELF_APPLICATION_NAME: &str = "SayAll";

fn application_identity(pid: u32) -> (String, String, Option<Vec<u8>>) {
    let (key, name, executable) = application_identity_parts(pid);
    let icon = executable.as_deref().and_then(application_icon_png);
    (key, name, icon)
}

fn application_identity_parts(pid: u32) -> (String, String, Option<String>) {
    let executable = process_executable_path(pid);
    let display_name = executable
        .as_deref()
        .and_then(file_display_name)
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            executable.as_deref().and_then(|path| {
                Path::new(path)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_owned)
            })
        })
        .unwrap_or_else(|| "未知应用".to_owned());
    let identity = crate::app_launcher::process_app_user_model_id(pid)
        .filter(|id| !id.trim().is_empty())
        .or_else(|| executable.clone())
        .unwrap_or_else(|| display_name.clone());
    // 稳定键继续用原始文件名（去扩展名），避免已有 Weixin/WeChat 记录被拆成新应用；
    // 界面名走产品名映射：chrome → Chrome 浏览器、notepad → 记事本、Weixin → 微信。
    let key_name = display_name.clone();
    let localized_name = executable
        .as_deref()
        .and_then(crate::foreground::application_display_name)
        .map(|value| localized_application_name(&value))
        .unwrap_or_else(|| localized_application_name(&display_name));
    (
        application_key(&identity, &key_name),
        localized_name,
        executable,
    )
}

fn localized_application_name(raw: &str) -> String {
    if raw == "微信" || raw.eq_ignore_ascii_case("weixin") || raw.eq_ignore_ascii_case("wechat") {
        "微信".to_owned()
    } else {
        raw.to_owned()
    }
}

/// 判断缓存里的图标 PNG 是否还值得用。
///
/// 用于升级迁移：旧版本（`LR_MONOCHROME`）写下的坏图是「2 种颜色 + 全不透明」的
/// 黑白块，不能直接沿用；同时也不能把缓存一律作废——应用没在运行时就提取不到
/// 图标，那样会让没开着的应用全部退回通用图标（2026-11-04 实测）。
pub fn icon_png_is_usable(bytes: &[u8]) -> bool {
    let decoder = png::Decoder::new(bytes);
    let Ok(mut reader) = decoder.read_info() else {
        return false;
    };
    let mut pixels = vec![0u8; reader.output_buffer_size()];
    let Ok(info) = reader.next_frame(&mut pixels) else {
        return false;
    };
    let pixels = &pixels[..info.buffer_size()];
    let mut colors = std::collections::HashSet::new();
    let mut transparent = false;
    for pixel in pixels.chunks_exact(4) {
        colors.insert([pixel[0], pixel[1], pixel[2], pixel[3]]);
        transparent |= pixel[3] != 255;
        if colors.len() > 2 && transparent {
            return true;
        }
    }
    colors.len() > 2 || transparent
}

/// 从可执行文件提取 64×64 图标并编码为 PNG。失败只返回 `None`，由前端使用通用图标。
///
/// 捕获轮询会反复读同一应用，按可执行路径缓存，避免每轮都做 Shell 提取与 PNG 编码。
fn application_icon_png(path: &str) -> Option<Vec<u8>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Vec<u8>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(path) {
            return cached.clone();
        }
    }
    let icon = extract_icon_png(path);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(path.to_owned(), icon.clone());
    }
    icon
}

/// 从可执行文件路径提取图标（PNG 字节）。给「回眸 → 本地 Agent 访问」用：
/// 卡片要显示用户已安装的 AI 工具的真实图标，与「回眸」记录的应用图标同一条链路。
pub fn icon_png_for_executable(path: &str) -> Option<Vec<u8>> {
    if path.trim().is_empty() {
        return None;
    }
    application_icon_png(path)
}

/// 只读扫描当前进程，按显示名找到应用并提取图标。
///
/// 用于「只有录音、没有文字记录」的应用：文字捕获失败时不会走 `write_icon`，
/// 但录音仍需要图标。只读进程路径与公开 Shell 图标，不读取应用私有数据。
pub fn application_icon_for_display_name(name: &str) -> Option<Vec<u8>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    let target = localized_application_name(name);
    if target == "未知应用" {
        return None;
    }
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.get(&target) {
            return Some(cached.clone());
        }
    }
    let mut found = None;
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return None;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(entry.szExeFile.len());
            let executable = String::from_utf16_lossy(&entry.szExeFile[..len]);
            let stem = Path::new(&executable)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            let display = crate::foreground::application_display_name(&executable)
                .map(|value| localized_application_name(&value));
            if stem.eq_ignore_ascii_case(&target)
                || display
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(&target))
            {
                if let Some(path) = process_executable_path(entry.th32ProcessID) {
                    if let Some(png) = application_icon_png(&path) {
                        found = Some(png);
                        break;
                    }
                }
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    if let Some(png) = found.as_ref() {
        if let Ok(mut guard) = cache.lock() {
            guard.insert(target, png.clone());
        }
    }
    found
}

fn extract_icon_png(path: &str) -> Option<Vec<u8>> {
    if path.encode_utf16().count() >= 259 {
        return None;
    }
    let mut file = [0u16; 260];
    for (index, value) in path.encode_utf16().enumerate() {
        file[index] = value;
    }
    let mut icons = [HICON::default()];
    // `windows` crate 的 `PrivateExtractIconsW` 把第 7 个参数暴露成 `flags`
    // （`nicons` 由 `phicon` 切片长度推出，这里恒为 1）。必须传 `LR_DEFAULTCOLOR`
    // 即 0：传 1 等于 `LR_MONOCHROME`，Windows 会把经典位图帧转成单色位图
    // （黑/白、alpha 全 0），Chrome 这类只用位图存小尺寸帧的应用会变成灰度块
    // （2026-11-03 实测：flags=1 时 64px 只剩 2 种颜色）。
    let count = unsafe {
        PrivateExtractIconsW(
            &file,
            0,
            APPLICATION_ICON_PIXELS,
            APPLICATION_ICON_PIXELS,
            Some(&mut icons),
            None,
            0,
        )
    };
    if count == 0 || icons[0].is_invalid() {
        return None;
    }
    let encoded = icon_to_png(icons[0]);
    unsafe {
        let _ = DestroyIcon(icons[0]);
    }
    encoded
}

fn icon_to_png(icon: HICON) -> Option<Vec<u8>> {
    let mut info: ICONINFO = unsafe { std::mem::zeroed() };
    if unsafe { GetIconInfo(icon, &mut info) }.is_err() {
        return None;
    }
    bitmap_to_png(info.hbmColor, Some(info.hbmMask))
}

/// 从 Shell 应用项（例如 `shell:AppsFolder\...`）提取图标。
///
/// Store 应用的快捷方式目标不一定能解析成 exe，此时直接向 Shell 要 64×64 图标。
pub fn icon_png_for_shell_item(target: &str) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY,
    };

    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let result = (|| -> windows::core::Result<Vec<u8>> {
        let wide: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
        let factory: IShellItemImageFactory =
            unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }?;
        let bitmap = unsafe {
            factory.GetImage(
                SIZE {
                    cx: APPLICATION_ICON_PIXELS,
                    cy: APPLICATION_ICON_PIXELS,
                },
                SIIGBF_ICONONLY,
            )
        }?;
        bitmap_to_png(bitmap, None)
            .ok_or_else(|| windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))
    })();
    if initialized {
        unsafe {
            CoUninitialize();
        }
    }
    result.ok()
}

fn bitmap_to_png(color: HBITMAP, mask: Option<HBITMAP>) -> Option<Vec<u8>> {
    let encoded = (|| {
        let mut bitmap: BITMAP = unsafe { std::mem::zeroed() };
        let copied = unsafe {
            GetObjectW(
                color.into(),
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bitmap as *mut BITMAP as *mut c_void),
            )
        };
        if copied == 0 {
            return None;
        }
        let width = bitmap.bmWidth;
        let height = bitmap.bmHeight;
        if !(1..=256).contains(&width) || !(1..=256).contains(&height) {
            return None;
        }
        let mut info_header: BITMAPINFO = unsafe { std::mem::zeroed() };
        info_header.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            return None;
        }
        let lines = unsafe {
            GetDIBits(
                dc,
                color,
                0,
                height as u32,
                Some(pixels.as_mut_ptr() as *mut c_void),
                &mut info_header,
                DIB_RGB_COLORS,
            )
        };
        if lines == 0 {
            unsafe {
                let _ = DeleteDC(dc);
            }
            return None;
        }
        let any_alpha = pixels.chunks_exact(4).any(|pixel| pixel[3] != 0);
        // 没有 alpha 通道的旧式图标：透明区域只体现在 AND 掩码里。此时把 alpha
        // 一律补 255 会把图标背景画成不透明色块，必须改用掩码。
        let mask_alpha = if any_alpha {
            None
        } else {
            mask.and_then(|mask| mask_alpha_bits(dc, mask, width, height))
        };
        unsafe {
            let _ = DeleteDC(dc);
        }
        let mut rgba = Vec::with_capacity(pixels.len());
        for pixel in pixels.chunks_exact(4) {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
        if !any_alpha {
            match mask_alpha {
                Some(alpha) => {
                    for (pixel, value) in rgba.chunks_exact_mut(4).zip(alpha) {
                        pixel[3] = value;
                    }
                }
                None => {
                    for pixel in rgba.chunks_exact_mut(4) {
                        pixel[3] = 255;
                    }
                }
            }
        }
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().ok()?;
            writer.write_image_data(&rgba).ok()?;
        }
        Some(png)
    })();
    unsafe {
        if !color.is_invalid() {
            let _ = DeleteObject(color.into());
        }
        if let Some(mask) = mask {
            if !mask.is_invalid() {
                let _ = DeleteObject(mask.into());
            }
        }
    }
    encoded
}

fn bstr(value: windows::core::Result<BSTR>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

/// 1bpp 掩码的 `GetDIBits` 输出需要 2 个调色板项，比 `BITMAPINFO` 自带的多一项，
/// 单独定义避免 GDI 写出结构体边界。
#[repr(C)]
struct MaskBitmapInfo {
    header: BITMAPINFOHEADER,
    colors: [u32; 2],
}

/// 从 AND 掩码取每像素透明度：位为 1 表示该像素透明（0），否则不透明（255）。
///
/// 掩码是 1bpp 位图，行宽按 DWORD 对齐，不能按宽度直接推。取不到或形状不符时
/// 返回 `None`，由调用方退回「整块不透明」。
fn mask_alpha_bits(dc: HDC, mask: HBITMAP, width: i32, height: i32) -> Option<Vec<u8>> {
    if mask.is_invalid() {
        return None;
    }
    let mut bitmap: BITMAP = unsafe { std::mem::zeroed() };
    let copied = unsafe {
        GetObjectW(
            mask.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut BITMAP as *mut c_void),
        )
    };
    // 单色图标里掩码可能是双倍高度（AND + OR），这里只处理与彩色位图同高的情况。
    if copied == 0 || bitmap.bmBitsPixel != 1 || bitmap.bmHeight != height || width <= 0 {
        return None;
    }
    let stride = ((width as usize) + 31) / 32 * 4;
    if stride * height as usize > 1 << 20 {
        return None;
    }
    let mut bits = vec![0u8; stride * height as usize];
    let mut info: MaskBitmapInfo = unsafe { std::mem::zeroed() };
    info.header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 1,
        biCompression: BI_RGB.0,
        biClrUsed: 2,
        ..Default::default()
    };
    let lines = unsafe {
        GetDIBits(
            dc,
            mask,
            0,
            height as u32,
            Some(bits.as_mut_ptr() as *mut c_void),
            &mut info as *mut MaskBitmapInfo as *mut BITMAPINFO,
            DIB_RGB_COLORS,
        )
    };
    if lines != height {
        return None;
    }
    Some(compose_mask_alpha(
        &bits,
        stride,
        width as usize,
        height as usize,
    ))
}

/// 把自上而下的 1bpp 掩码行转成每像素 alpha（1 = 透明）。
fn compose_mask_alpha(bits: &[u8], stride: usize, width: usize, height: usize) -> Vec<u8> {
    let mut alpha = vec![255u8; width * height];
    for y in 0..height {
        let row = match bits.get(y * stride..(y + 1) * stride) {
            Some(row) => row,
            None => break,
        };
        for (x, slot) in alpha[y * width..(y + 1) * width].iter_mut().enumerate() {
            match row.get(x / 8) {
                Some(byte) if byte & (0x80 >> (x % 8)) != 0 => *slot = 0,
                Some(_) => {}
                None => break,
            }
        }
    }
    alpha
}

fn process_executable_path(pid: u32) -> Option<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let result = (|| {
        let mut buffer = vec![0u16; 32768];
        let mut length = buffer.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut length,
            )
        }
        .ok()?;
        let used = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(length as usize);
        String::from_utf16(&buffer[..used]).ok()
    })();
    unsafe {
        let _ = CloseHandle(process);
    }
    result
}

fn file_display_name(path: &str) -> Option<String> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = SHFILEINFOW::default();
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_DISPLAYNAME,
        )
    };
    if result == 0 {
        return None;
    }
    let end = info
        .szDisplayName
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(info.szDisplayName.len());
    Some(String::from_utf16_lossy(&info.szDisplayName[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_terms_cover_password_and_search() {
        let terms = UNSAFE_TERMS.join(" ");
        assert!(terms.contains("密码"));
        assert!(terms.contains("search"));
        assert!(terms.contains("地址"));
    }

    #[test]
    fn extracts_a_png_icon_from_a_system_executable() {
        let icon = application_icon_png(r"C:\Windows\System32\notepad.exe").expect("notepad icon");
        assert!(icon.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(icon.len() > 64);
    }

    /// 回归：`PrivateExtractIconsW` 的最后一个参数是 `flags`，传 1 等于
    /// `LR_MONOCHROME`，图标会被转成只有黑白的单色位图。系统记事本的 64px 帧
    /// 在修复前只有 2 种颜色且 alpha 全 0，修复后是多彩且带透明。
    #[test]
    fn extracted_icons_keep_their_color_and_alpha() {
        let png = application_icon_png(r"C:\Windows\System32\notepad.exe").expect("notepad icon");
        let decoder = png::Decoder::new(png.as_slice());
        let mut reader = decoder.read_info().expect("png info");
        let mut pixels = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut pixels).expect("png frame");
        assert_eq!(info.color_type, png::ColorType::Rgba);
        let pixels = &pixels[..info.buffer_size()];
        let colors = pixels
            .chunks_exact(4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
            .collect::<std::collections::HashSet<_>>();
        assert!(
            colors.len() > 16,
            "单色图标说明 LR_MONOCHROME 又被传进来了：{} 种颜色",
            colors.len()
        );
        assert!(
            pixels.chunks_exact(4).any(|pixel| pixel[3] == 0),
            "图标缺少透明像素，掩码或 alpha 处理可能退化"
        );
    }

    #[test]
    fn composes_alpha_from_a_one_bit_mask() {
        // 8×2，行宽 4 字节（DWORD 对齐）：第 0 行最左像素透明位为 1。
        let bits = [0b1000_0000u8, 0, 0, 0, 0, 0, 0, 0];
        let alpha = compose_mask_alpha(&bits, 4, 8, 2);
        assert_eq!(alpha[0], 0);
        assert_eq!(alpha[1], 255);
        assert_eq!(alpha[8], 255);
        assert_eq!(alpha.len(), 16);
    }

    #[test]
    fn mask_alpha_tolerates_short_rows() {
        // 只读到第一行时，后续行保持不透明而不是越界或 panic。
        let alpha = compose_mask_alpha(&[0xff], 1, 8, 2);
        assert_eq!(alpha.len(), 16);
        assert_eq!(alpha[0], 0);
        assert_eq!(alpha[7], 0);
        assert_eq!(alpha[8], 255);
    }

    /// 4×4 RGBA，16 种颜色、带透明 —— 一张能用的图标缓存。
    const COLORFUL_ICON: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 4, 8, 6,
        0, 0, 0, 169, 241, 158, 126, 0, 0, 0, 44, 73, 68, 65, 84, 120, 218, 109, 202, 177, 9, 0,
        64, 12, 195, 64, 25, 50, 135, 39, 241, 254, 107, 57, 197, 23, 223, 164, 18, 7, 18, 208, 96,
        5, 55, 88, 67, 44, 112, 225, 117, 200, 7, 215, 177, 125, 93, 11, 56, 16, 21, 52, 176, 0, 0,
        0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    /// 4×4 RGBA，只有黑/白且全不透明 —— `LR_MONOCHROME` 时期写坏的缓存形状。
    const MONOCHROME_ICON: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 4, 8, 6,
        0, 0, 0, 169, 241, 158, 126, 0, 0, 0, 27, 73, 68, 65, 84, 120, 218, 99, 96, 96, 96, 248,
        255, 255, 255, 255, 255, 112, 26, 133, 195, 192, 240, 159, 129, 160, 10, 0, 63, 75, 39,
        217, 8, 22, 13, 44, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    #[test]
    fn cached_icons_are_reused_only_when_they_can_still_be_real_icons() {
        assert!(icon_png_is_usable(COLORFUL_ICON));
        assert!(!icon_png_is_usable(MONOCHROME_ICON));
        assert!(!icon_png_is_usable(b"not-a-png"));
        assert!(!icon_png_is_usable(&[]));
    }

    #[test]
    fn extracted_icons_pass_the_cache_check() {
        let png = application_icon_png(r"C:\Windows\System32\notepad.exe").expect("notepad icon");
        assert!(icon_png_is_usable(&png));
    }

    #[test]
    fn self_foreground_recordings_are_labelled_sayall() {
        // 首次设置向导的语音测试前台是无线麦自己；必须显示 SayAll，
        // 而不是因为缺身份落到「未知应用」（2026-11-06 用户报告）。
        assert_eq!(
            application_identity_for_pid(std::process::id()),
            Some((
                SELF_APPLICATION_NAME.to_owned(),
                SELF_APPLICATION_NAME.to_owned()
            ))
        );
    }

    #[test]
    fn missing_foreground_pid_has_no_identity() {
        assert_eq!(application_identity_for_pid(0), None);
    }

    #[test]
    fn localizes_wechat_names_without_changing_other_apps() {
        assert_eq!(localized_application_name("Weixin"), "微信");
        assert_eq!(localized_application_name("WeChat"), "微信");
        assert_eq!(localized_application_name("WorkBuddy"), "WorkBuddy");
    }
}
