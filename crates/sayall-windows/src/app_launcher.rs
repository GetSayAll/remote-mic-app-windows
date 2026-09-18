//! 预设应用动作（对齐 Mac `PresetApplication`）：按键映射可打开常用应用。
//!
//! 语义与 Mac 一致：**已运行 → 恢复窗口并前置；未运行 → 启动**。
//! 只用公开 API：注册表 App Paths 探测安装、工具帮助进程快照找已运行
//! 实例、窗口枚举前置、ShellExecuteW 启动（短命线程内做 COM 初始化，
//! 避免引擎线程套间约束）。
//!
//! 预设表仅列出常见应用；未安装项在 UI 中不展示（Mac
//! `installedBundleIdentifiers` 同款过滤）。

use serde::{Deserialize, Serialize};

/// UI 侧预设应用条目（`list_preset_apps` 返回）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetAppInfo {
    pub id: String,
    pub name: String,
    pub installed: bool,
}

/// 预设应用定义。
pub struct PresetApp {
    pub id: &'static str,
    pub name: &'static str,
    /// 进程/窗口匹配用可执行文件名（大小写不敏感；含不同版本命名）。
    pub exe_names: &'static [&'static str],
}

/// 预设应用表（对齐 Mac 预设 + Windows 常见项）。无线麦自身排首位
/// （对齐 Mac `PresetApplication.remoteMic`，恒为已安装）。
pub const PRESET_APPS: &[PresetApp] = &[
    PresetApp {
        id: "sayall",
        name: "无线麦",
        exe_names: &["sayall-windows-app.exe"],
    },
    PresetApp {
        id: "wechat",
        name: "微信",
        exe_names: &["WeChat.exe", "Weixin.exe"],
    },
    PresetApp {
        id: "edge",
        name: "Edge 浏览器",
        exe_names: &["msedge.exe"],
    },
    PresetApp {
        id: "chrome",
        name: "Chrome 浏览器",
        exe_names: &["chrome.exe"],
    },
    PresetApp {
        id: "notepad",
        name: "记事本",
        exe_names: &["notepad.exe"],
    },
    PresetApp {
        id: "calc",
        name: "计算器",
        exe_names: &["calc.exe", "CalculatorApp.exe"],
    },
    PresetApp {
        id: "explorer",
        name: "文件资源管理器",
        exe_names: &["explorer.exe"],
    },
    PresetApp {
        id: "netease_music",
        name: "网易云音乐",
        exe_names: &["cloudmusic.exe"],
    },
];

pub fn preset_app(id: &str) -> Option<&'static PresetApp> {
    PRESET_APPS.iter().find(|app| app.id == id)
}

/// 探测预设应用安装状态（System32 直存或 App Paths 注册表命中）。
/// 无线麦自身恒为已安装（映射运行时它必然在运行）。
#[cfg(windows)]
pub fn probe_preset_apps() -> Vec<PresetAppInfo> {
    PRESET_APPS
        .iter()
        .map(|app| PresetAppInfo {
            id: app.id.to_owned(),
            name: app.name.to_owned(),
            installed: app.id == "sayall" || app.exe_names.iter().any(|exe| exe_resolvable(exe)),
        })
        .collect()
}

#[cfg(not(windows))]
pub fn probe_preset_apps() -> Vec<PresetAppInfo> {
    Vec::new()
}

/// 自定义应用选择结果（`pick_custom_app` 命令返回）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomAppPick {
    /// 展示名（文件名去扩展名）。
    pub name: String,
    /// 完整路径（.exe/.lnk）。作为 `OpenApp.target` 持久化。
    pub path: String,
}

/// 判断 OpenApp 目标是否为自定义路径（非预设 id）。
#[cfg(windows)]
fn is_custom_path_target(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    (lower.contains('\\') || lower.contains('/'))
        && (lower.ends_with(".exe") || lower.ends_with(".lnk"))
}

/// 宿主注册的"同步自身窗口可见性缓存"回调（Tauri 层在启动时注入一次）。
///
/// 为什么必须有它：为修"托盘里的窗口唤不回"，`show_and_force_foreground` 会用
/// Win32 `ShowWindow(SW_SHOW)` 显示**本进程**的主窗口（同步生效，紧随其后的
/// `SetForegroundWindow` 才有意义）。但 Win32 直接改可见性绕过了 tao 的
/// `WindowFlags::VISIBLE` 缓存——tao 的 `set_window_flags` 只应用新旧 flag 的
/// **差异**。于是缓存停在 `false`，之后 `window.hide()`（点 X 关到托盘）的
/// `set_visible(false)` 被判为"无变化"而整个跳过，**窗口再也关不进托盘**
/// （2026-09-16 真机实测：连续 13 次 `window_close hide_result=Ok(())` 仍不隐藏）。
///
/// 宿主的这个回调只需 `window.show()`：tao 会把缓存置回 `true`（窗口已可见时
/// 判为无差异、无副作用），缓存与真实状态重新一致，`hide()` 恢复正常。
/// 隐藏与显示走同一事件队列，FIFO 顺序天然保证"同步在前、hide 在后"。
/// 外部应用的窗口不归本进程的 tao 管，无需同步。
static SELF_SHOW_SYNC: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> =
    std::sync::OnceLock::new();

/// 注册自身窗口可见性缓存同步回调（宿主在启动时调用一次）。
pub fn set_self_show_sync(sync: impl Fn() + Send + Sync + 'static) {
    let _ = SELF_SHOW_SYNC.set(Box::new(sync));
}

/// 激活已运行的应用窗口；未运行则启动。支持预设 id 与自定义路径。
/// 自定义 .exe：按文件名探测已运行实例后直接启动；
/// 自定义 .lnk：先解析快捷方式（目标 exe/参数/工作目录），按解析结果
/// 激活或直接启动——不经 shell 的 .lnk 异步链路（短命线程退出会中止
/// 该链路，2026-09-06 实证：ShellExecuteW 对 .lnk 返回成功但应用未启动）；
/// 解析失败退回原路径 ShellExecuteW。
#[cfg(windows)]
pub fn activate_or_launch(id: &str) -> Result<(), String> {
    if crate::registered_apps::is_registered_target(id) {
        return crate::registered_apps::launch_registered_app(id);
    }
    if let Some(app) = preset_app(id) {
        if app.id == "sayall" {
            // 自身：恒已运行；激活失败（窗口隐藏等）时用自身 exe 路径重启拉起。
            if activate_running(app.exe_names) {
                // `activate_running` 对隐藏的自身窗口用 Win32 `ShowWindow` 显示它
                // （同步生效，其后抢前台才有意义），这会让 tao 的
                // `WindowFlags::VISIBLE` 缓存停在 false。必须立刻请宿主同步一次
                // （tao `window.show()`，幂等），否则随后点 X 的 `window.hide()`
                // 会因"无差异"被整个跳过 —— 窗口关不进托盘（2026-09-16 实测）。
                if let Some(sync) = SELF_SHOW_SYNC.get() {
                    sync();
                    crate::ble::gatt_note(
                        "app_launcher self_show_sync via=host_callback terminal_result=passed"
                            .to_owned(),
                    );
                }
                return Ok(());
            }
            let exe =
                std::env::current_exe().map_err(|error| format!("获取自身路径失败：{error}"))?;
            return launch_explicit(&exe.to_string_lossy(), None, None);
        }
        if activate_running(app.exe_names) {
            return Ok(());
        }
        return launch_new(app.exe_names);
    }
    if is_custom_path_target(id) {
        let path = std::path::Path::new(id);
        if !path.exists() {
            return Err(format!("应用不存在：{id}"));
        }
        if id.to_ascii_lowercase().ends_with(".exe") {
            let exe_name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            if activate_running(&[&exe_name]) {
                return Ok(());
            }
            return launch_explicit(id, None, None);
        }
        if let Some(resolved) = resolve_lnk(id) {
            if !resolved.exe_path.is_empty() {
                let exe_name = std::path::Path::new(&resolved.exe_path)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                if activate_running(&[&exe_name]) {
                    return Ok(());
                }
                let arguments =
                    (!resolved.arguments.is_empty()).then_some(resolved.arguments.clone());
                let dir =
                    (!resolved.working_dir.is_empty()).then_some(resolved.working_dir.clone());
                return launch_explicit(&resolved.exe_path, arguments.as_deref(), dir.as_deref());
            }
        }
        return launch_path(id);
    }
    Err(format!("未知预设应用：{id}"))
}

/// 快捷方式解析结果（.lnk → 目标 exe/参数/工作目录）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedShortcut {
    exe_path: String,
    arguments: String,
    working_dir: String,
}

/// 解析 .lnk 快捷方式（STA COM 线程内 IShellLinkW + IPersistFile）。
#[cfg(windows)]
fn resolve_lnk(lnk_path: &str) -> Option<ResolvedShortcut> {
    let path = lnk_path.to_owned();
    let handle = std::thread::Builder::new()
        .name("sayall-resolve-lnk".to_owned())
        .spawn(move || {
            use windows::core::{Interface, PCWSTR};
            use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
            use windows::Win32::System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED,
            };
            use windows::Win32::System::Com::{IPersistFile, STGM_READ};
            use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

            unsafe {
                if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                    return None;
                }
            }
            let result = (|| unsafe {
                let shell_link: IShellLinkW =
                    CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
                let persist: IPersistFile = shell_link.cast().ok()?;
                let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
                persist.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
                let mut file_buf = [0u16; 1040];
                let mut find_data = WIN32_FIND_DATAW::default();
                shell_link.GetPath(&mut file_buf, &mut find_data, 0).ok()?;
                let mut args_buf = [0u16; 1040];
                shell_link.GetArguments(&mut args_buf).ok()?;
                let mut dir_buf = [0u16; 1040];
                shell_link.GetWorkingDirectory(&mut dir_buf).ok()?;
                let take = |buf: &[u16]| -> String {
                    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                    String::from_utf16_lossy(&buf[..len])
                };
                Some(ResolvedShortcut {
                    exe_path: take(&file_buf),
                    arguments: take(&args_buf),
                    working_dir: take(&dir_buf),
                })
            })();
            unsafe {
                CoUninitialize();
            }
            result
        })
        .ok()?;
    handle.join().ok().flatten()
}

#[cfg(not(windows))]
pub fn activate_or_launch(_id: &str) -> Result<(), String> {
    Err("打开应用仅在 Windows 上可用".to_owned())
}

#[cfg(windows)]
fn exe_resolvable(exe: &str) -> bool {
    if system32_path(exe).exists() {
        return true;
    }
    app_paths_key_exists(exe)
}

#[cfg(windows)]
fn system32_path(exe: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
    root.join("System32").join(exe)
}

#[cfg(windows)]
fn app_paths_key_exists(exe: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
    };

    let subkey: String = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
    let wide: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let mut key = HKEY::default();
        let opened =
            unsafe { RegOpenKeyExW(root, PCWSTR(wide.as_ptr()), None, KEY_READ, &mut key) };
        if opened.is_ok() {
            unsafe {
                let _ = RegCloseKey(key);
            }
            return true;
        }
    }
    false
}

/// 已运行 → 恢复窗口并前置。返回是否找到并激活了窗口。
#[cfg(windows)]
fn activate_running(exe_names: &[&str]) -> bool {
    use std::collections::HashSet;

    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    let wanted: HashSet<String> = exe_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();

    // 进程快照：exe 名 → pid 集合。
    let mut pids: HashSet<u32> = HashSet::new();
    unsafe {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
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
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
            if wanted.contains(&name) {
                pids.insert(entry.th32ProcessID);
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
    }
    if pids.is_empty() {
        return false;
    }

    // 枚举顶层窗口：找到目标进程的主窗口（无所有者、非工具窗口）→ 恢复/显示
    // 并强制置前。先优先可见窗口；若只在托盘隐藏，则退而激活隐藏主窗口。
    let mut context = EnumContext {
        pids: &pids,
        activated: false,
        hidden_candidate: None,
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_proc),
            LPARAM(&mut context as *mut EnumContext as isize),
        );
    }
    if !context.activated {
        if let Some(hwnd) = context.hidden_candidate {
            unsafe {
                show_and_force_foreground(hwnd);
            }
            context.activated = true;
        }
    }
    context.activated
}

/// EnumWindows 回调（extern "system" ABI，无捕获）。
#[cfg(windows)]
mod win_impl {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_RESTORE,
        SW_SHOW, WS_EX_TOOLWINDOW,
    };

    pub(super) struct EnumContext<'a> {
        pub pids: &'a std::collections::HashSet<u32>,
        pub activated: bool,
        /// 目标进程的隐藏（如收进托盘）主窗口候选；仅在无可见窗口时回退激活。
        pub hidden_candidate: Option<HWND>,
    }

    pub(super) unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let context = &mut *(lparam.0 as *mut EnumContext);
        if context.activated {
            return BOOL::from(true);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if !context.pids.contains(&pid) {
            return BOOL::from(true);
        }
        // 只认主窗口：无所有者且非工具窗口（排除托盘/弹层/辅助隐藏窗口）。
        if GetWindow(hwnd, GW_OWNER).unwrap_or(HWND::default()).0 != std::ptr::null_mut() {
            return BOOL::from(true);
        }
        if (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32) & (WS_EX_TOOLWINDOW.0 as u32) != 0 {
            return BOOL::from(true);
        }
        if IsWindowVisible(hwnd).as_bool() {
            // 可见但被其它窗口遮挡：直接恢复并强制置前。
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            show_and_force_foreground(hwnd);
            context.activated = true;
            return BOOL::from(false);
        }
        // 隐藏（如收进托盘）的主窗口：记录为候选，循环结束后再激活。
        if context.hidden_candidate.is_none() {
            context.hidden_candidate = Some(hwnd);
        }
        BOOL::from(true)
    }

    /// 显示（若隐藏）/还原（若最小化）目标窗口并强制置前。
    /// 绕过 Windows 前台锁定（foreground lock）：菜单键的"打开应用"事件来自后台
    /// 进程的引擎线程，直接 `SetForegroundWindow` 会静默失败（仅任务栏闪烁、不置前）。
    /// 用 `AttachThreadInput` 把本线程输入挂到当前前台线程，使置前调用被视为前台线程
    /// 发起而获准。不使用 TOPMOST 置顶：短暂的 topmost 状态会污染窗口常驻 Z 序，
    /// 且与 `window.hide()`（关到托盘）交互时会造成窗口无法正常隐藏；仅依赖
    /// attach + SetForegroundWindow（与仓库 wetype_dormancy_probe 已验证的前台切换同款）。
    pub(super) unsafe fn show_and_force_foreground(hwnd: HWND) {
        let mut target_pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut target_pid));
        let self_window = target_pid == GetCurrentProcessId();
        let visible_before = IsWindowVisible(hwnd).as_bool();
        // 恢复可见性走 Win32（同步生效，确保紧随其后的抢前台有效）。
        // 对**本进程**窗口，这会让 tao 的 `WindowFlags::VISIBLE` 缓存与真实状态
        // 脱节，因此调用方（`activate_or_launch` 的 sayall 分支）必须随后调用
        // `SELF_SHOW_SYNC` 把缓存同步回来，否则 `window.hide()`（点 X 关到托盘）
        // 会被判为"无差异"而跳过（2026-09-16 真机实测）。
        let took_show_path = !visible_before;
        if took_show_path {
            let _ = ShowWindow(hwnd, SW_SHOW);
        } else if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let foreground = GetForegroundWindow();
        let foreground_thread = GetWindowThreadProcessId(foreground, None);
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0 && foreground_thread != current_thread;
        crate::ble::gatt_note(format!(
            "app_launcher action=show_and_force_foreground self_window={self_window} visible_before={visible_before} took_show_path={took_show_path} foreground_thread={foreground_thread} current_thread={current_thread} attached={attached}"
        ));
        if attached {
            let _ = AttachThreadInput(foreground_thread, current_thread, true);
        }
        let ok = SetForegroundWindow(hwnd).as_bool();
        if attached {
            let _ = AttachThreadInput(foreground_thread, current_thread, false);
        }
        crate::ble::gatt_note(format!(
            "app_launcher action=show_and_force_foreground terminal_result={} set_foreground_ok={}",
            if ok { "passed" } else { "failed" },
            ok
        ));
    }
}

#[cfg(windows)]
use win_impl::{enum_windows_proc, show_and_force_foreground, EnumContext};

/// 启动新实例（短命线程内 COM 初始化后 ShellExecuteW，避免引擎线程套间约束）。
#[cfg(windows)]
fn launch_new(exe_names: &[&str]) -> Result<(), String> {
    let exes: Vec<String> = exe_names.iter().map(|exe| (*exe).to_owned()).collect();
    let handle = std::thread::Builder::new()
        .name("sayall-app-launch".to_owned())
        .spawn(move || {
            use windows::core::PCWSTR;
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

            // ShellExecuteW 依赖 OLE 初始化：短命线程内初始化并配对释放。
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
                );
            }
            let mut last_error = String::new();
            for exe in &exes {
                let wide: Vec<u16> = exe.encode_utf16().chain(Some(0)).collect();
                let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
                let result = unsafe {
                    ShellExecuteW(
                        None,
                        PCWSTR(verb.as_ptr()),
                        PCWSTR(wide.as_ptr()),
                        None,
                        None,
                        SW_SHOWNORMAL,
                    )
                };
                // 返回值 > 32 表示成功（ShellExecuteW 旧式约定）。
                if result.0 as usize > 32 {
                    unsafe {
                        windows::Win32::System::Com::CoUninitialize();
                    }
                    return Ok(());
                }
                last_error = format!("ShellExecuteW 返回 {result:?}");
            }
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
            Err(last_error)
        })
        .map_err(|error| format!("启动线程失败：{error}"))?;
    handle
        .join()
        .unwrap_or_else(|_| Err("启动线程异常退出".to_owned()))
}

#[cfg(not(windows))]
fn launch_path(_path: &str) -> Result<(), String> {
    Err("打开应用仅在 Windows 上可用".to_owned())
}

/// 按完整路径启动（短命 COM 线程内 ShellExecuteW，支持 .exe/.lnk）。
#[cfg(windows)]
fn launch_path(path: &str) -> Result<(), String> {
    launch_explicit(path, None, None)
}

/// 按完整路径启动（可带参数与工作目录；短命 COM 线程内 ShellExecuteW）。
/// 启动后短暂保活线程：覆盖 shell 异步派生链路（.lnk 场景），避免
/// 线程退出中止挂起的启动（2026-09-06 实证）。
#[cfg(windows)]
fn launch_explicit(
    target: &str,
    arguments: Option<&str>,
    working_dir: Option<&str>,
) -> Result<(), String> {
    let target = target.to_owned();
    let arguments = arguments.map(str::to_owned);
    let working_dir = working_dir.map(str::to_owned);
    let handle = std::thread::Builder::new()
        .name("sayall-app-launch-path".to_owned())
        .spawn(move || {
            use windows::core::PCWSTR;
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
                );
            }
            let to_wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
            let wide = to_wide(&target);
            let verb = to_wide("open");
            let args = arguments.as_deref().map(to_wide);
            let dir = working_dir.as_deref().map(to_wide);
            let args_ptr = args
                .as_ref()
                .map(|v| PCWSTR(v.as_ptr()))
                .unwrap_or(PCWSTR::null());
            let dir_ptr = dir
                .as_ref()
                .map(|v| PCWSTR(v.as_ptr()))
                .unwrap_or(PCWSTR::null());
            let result = unsafe {
                ShellExecuteW(
                    None,
                    PCWSTR(verb.as_ptr()),
                    PCWSTR(wide.as_ptr()),
                    args_ptr,
                    dir_ptr,
                    SW_SHOWNORMAL,
                )
            };
            // 保活：给 shell 的异步派生留出完成窗口。
            std::thread::sleep(std::time::Duration::from_millis(80));
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
            if result.0 as usize > 32 {
                Ok(())
            } else {
                Err(format!("ShellExecuteW 返回 {result:?}"))
            }
        })
        .map_err(|error| format!("启动线程失败：{error}"))?;
    handle
        .join()
        .unwrap_or_else(|_| Err("启动线程异常退出".to_owned()))
}

/// 在系统文件资源管理器里打开目录（"打开日志目录"入口）。
///
/// 复用 `launch_explicit` 的 ShellExecuteW 链路而不是另起 `explorer.exe` 子进程：
/// 目录的 "open" 动词本来就由 shell 处理，两者等价，但复用能继承已验证的
/// COM 套间初始化与 80ms 保活（2026-09-06 实证：线程过早退出会中止挂起的启动）。
///
/// 目录不存在时不在这里创建——资源管理器会弹出系统"找不到"对话框，那是误导性的
/// 用户可见错误。调用方应先确保目录存在（见 src-tauri 的 `open_log_directory`）。
#[cfg(windows)]
pub fn open_directory(path: &std::path::Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err("目录不存在".to_owned());
    }
    launch_explicit(&path.to_string_lossy(), None, None)
}

#[cfg(not(windows))]
pub fn open_directory(_path: &std::path::Path) -> Result<(), String> {
    Err("打开目录仅在 Windows 上可用".to_owned())
}

/// 原生文件选择器：选择自定义应用（.exe/.lnk）。
/// 在短命 STA COM 线程内运行 IFileOpenDialog，避免占用调用方套间。
#[cfg(windows)]
pub fn pick_custom_app() -> Option<CustomAppPick> {
    let handle = std::thread::Builder::new()
        .name("sayall-pick-app".to_owned())
        .spawn(|| {
            use windows::core::PCWSTR;
            use windows::Win32::System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED,
            };
            use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
            use windows::Win32::UI::Shell::{
                FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
                SIGDN_FILESYSPATH,
            };

            unsafe {
                let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                if hr.is_err() {
                    return None;
                }
            }
            let result = (|| -> Option<CustomAppPick> {
                unsafe {
                    let dialog: IFileOpenDialog =
                        match CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) {
                            Ok(dialog) => dialog,
                            Err(_) => return None,
                        };
                    let title: Vec<u16> = "选择应用".encode_utf16().chain(Some(0)).collect();
                    let _ = dialog.SetTitle(PCWSTR(title.as_ptr()));
                    let filter_spec: Vec<u16> =
                        "*.exe;*.lnk".encode_utf16().chain(Some(0)).collect();
                    let filter_name: Vec<u16> = "应用程序 (.exe, .lnk)"
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                    let filters = [COMDLG_FILTERSPEC {
                        pszName: PCWSTR(filter_name.as_ptr()),
                        pszSpec: PCWSTR(filter_spec.as_ptr()),
                    }];
                    let _ = dialog.SetFileTypes(&filters);
                    let options = dialog.GetOptions().ok()?;
                    let _ = dialog.SetOptions(options | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST);
                    if dialog.Show(None).is_err() {
                        return None; // 用户取消
                    }
                    let item = dialog.GetResult().ok()?;
                    let path_pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
                    let path = path_pwstr.to_string().ok()?;
                    CoTaskMemFree(Some(path_pwstr.0 as _));
                    let name = std::path::Path::new(&path)
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.clone());
                    Some(CustomAppPick { name, path })
                }
            })();
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
            result
        })
        .ok()?;
    handle.join().ok().flatten()
}

#[cfg(not(windows))]
pub fn pick_custom_app() -> Option<CustomAppPick> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_ids_are_unique_and_nonempty() {
        let mut ids: Vec<&str> = PRESET_APPS.iter().map(|app| app.id).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "预设应用 id 必须唯一");
        assert!(!ids.is_empty());
        for app in PRESET_APPS {
            assert!(!app.name.is_empty());
            assert!(!app.exe_names.is_empty());
        }
    }

    #[test]
    fn preset_app_lookup_rejects_unknown() {
        assert!(preset_app("wechat").is_some());
        assert!(preset_app("nonexistent-app").is_none());
    }

    #[test]
    #[cfg(windows)]
    fn custom_path_targets_are_recognized() {
        assert!(is_custom_path_target(r"C:\Apps\Tool.exe"));
        assert!(is_custom_path_target(r"C:\Apps\快捷方式.lnk"));
        assert!(is_custom_path_target("D:/dir/app.exe"));
        assert!(!is_custom_path_target("wechat"), "预设 id 不是路径");
        assert!(
            !is_custom_path_target(r"C:\Apps\readme.txt"),
            "仅支持 exe/lnk"
        );
        assert!(!is_custom_path_target("CAppsapp.exe"), "不含路径分隔符");
    }

    #[test]
    #[cfg(windows)]
    fn activate_or_launch_rejects_unknown_non_path() {
        let result = activate_or_launch("nonexistent-app");
        assert!(result.is_err(), "未知预设 id 应报错");
    }

    /// .lnk 解析往返：COM 创建临时快捷方式（指向记事本，带参数与工作
    /// 目录）→ resolve_lnk 解析 → 断言三元组一致。
    #[test]
    #[cfg(windows)]
    fn lnk_resolution_round_trips() {
        let notepad = {
            let root = std::env::var_os("SystemRoot")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
            root.join("System32").join("notepad.exe")
        };
        if !notepad.exists() {
            // 裁剪系统可能无记事本：跳过而非失败（探测行为=按机器如实报告）。
            return;
        }
        let lnk_path = std::env::temp_dir().join("sayall-lnk-roundtrip-test.lnk");
        let lnk = lnk_path.to_string_lossy().to_string();
        let created = std::thread::Builder::new()
            .name("sayall-lnk-create".to_owned())
            .spawn(move || {
                use windows::core::{Interface, PCWSTR};
                use windows::Win32::System::Com::IPersistFile;
                use windows::Win32::System::Com::{
                    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                    COINIT_APARTMENTTHREADED,
                };
                use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
                unsafe {
                    if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                        return false;
                    }
                }
                let ok = (|| unsafe {
                    let link: IShellLinkW =
                        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
                    let target: Vec<u16> = notepad
                        .to_string_lossy()
                        .encode_utf16()
                        .chain(Some(0))
                        .collect();
                    link.SetPath(PCWSTR(target.as_ptr())).ok()?;
                    let args: Vec<u16> = "/k test".encode_utf16().chain(Some(0)).collect();
                    link.SetArguments(PCWSTR(args.as_ptr())).ok()?;
                    let dir: Vec<u16> = r"C:\Windows".encode_utf16().chain(Some(0)).collect();
                    link.SetWorkingDirectory(PCWSTR(dir.as_ptr())).ok()?;
                    let persist: IPersistFile = link.cast().ok()?;
                    let lnk_wide: Vec<u16> = lnk.encode_utf16().chain(Some(0)).collect();
                    persist.Save(PCWSTR(lnk_wide.as_ptr()), true).ok()?;
                    Some(())
                })()
                .is_some();
                unsafe {
                    CoUninitialize();
                }
                ok
            })
            .expect("spawn create thread failed")
            .join()
            .expect("create thread panicked");
        assert!(created, "创建测试快捷方式失败");

        let resolved = resolve_lnk(&lnk_path.to_string_lossy());
        let _ = std::fs::remove_file(&lnk_path);
        let resolved = resolved.expect("解析测试快捷方式失败");
        assert!(
            resolved
                .exe_path
                .to_ascii_lowercase()
                .contains("notepad.exe"),
            "解析出的目标应为记事本，实际：{}",
            resolved.exe_path
        );
        assert_eq!(resolved.arguments, "/k test");
        assert!(
            resolved
                .working_dir
                .to_ascii_lowercase()
                .contains("windows"),
            "解析出的工作目录应包含 Windows，实际：{}",
            resolved.working_dir
        );
    }

    #[test]
    #[cfg(windows)]
    fn sayall_preset_is_always_installed() {
        let apps = probe_preset_apps();
        let sayall = apps
            .iter()
            .find(|app| app.id == "sayall")
            .expect("无线麦自身应在预设表首位");
        assert!(sayall.installed, "无线麦自身恒为已安装");
        assert_eq!(apps[0].id, "sayall", "对齐 Mac：自身排首位");
    }

    /// COM 文件对话框管线（创建+标题+过滤器+选项）可用性探针；
    /// Show 的交互行为由真机 UI 验证，此处验证 COM 对象链路本身。
    #[test]
    #[cfg(windows)]
    fn file_dialog_com_pipeline_is_usable() {
        let ok = std::thread::Builder::new()
            .name("sayall-dialog-probe".to_owned())
            .spawn(|| {
                use windows::core::PCWSTR;
                use windows::Win32::System::Com::{
                    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                    COINIT_APARTMENTTHREADED,
                };
                use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
                use windows::Win32::UI::Shell::{
                    FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST,
                };

                unsafe {
                    if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                        return false;
                    }
                }
                let usable = (|| unsafe {
                    let dialog: IFileOpenDialog =
                        CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
                    let title: Vec<u16> = "测试".encode_utf16().chain(Some(0)).collect();
                    dialog.SetTitle(PCWSTR(title.as_ptr())).ok()?;
                    let spec: Vec<u16> = "*.exe;*.lnk".encode_utf16().chain(Some(0)).collect();
                    let name: Vec<u16> = "应用".encode_utf16().chain(Some(0)).collect();
                    let filters = [COMDLG_FILTERSPEC {
                        pszName: PCWSTR(name.as_ptr()),
                        pszSpec: PCWSTR(spec.as_ptr()),
                    }];
                    dialog.SetFileTypes(&filters).ok()?;
                    dialog
                        .SetOptions(
                            dialog.GetOptions().ok()? | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST,
                        )
                        .ok()?;
                    Some(())
                })()
                .is_some();
                unsafe {
                    CoUninitialize();
                }
                usable
            })
            .expect("spawn probe thread failed")
            .join()
            .expect("probe thread panicked");
        assert!(ok, "IFileOpenDialog COM 管线应可用");
    }

    #[test]
    #[cfg(windows)]
    fn system_apps_report_installed() {
        let apps = probe_preset_apps();
        // 只断言跨桌面/服务器 SKU 都保证存在于 System32 的记事本；
        // explorer 等在裁剪系统上可能缺失（探测行为=按机器如实报告）。
        let notepad = apps.iter().find(|app| app.id == "notepad");
        assert!(
            notepad.is_some_and(|app| app.installed),
            "记事本应视为已安装"
        );
    }

    /// 不存在的目录必须直接拒绝：否则资源管理器会弹出系统"找不到"对话框，
    /// 把"日志目录没建好"伪装成用户在系统里的操作失误。
    #[test]
    fn open_directory_rejects_missing_directory() {
        let missing = std::env::temp_dir().join("sayall-open-directory-missing-probe");
        let _ = std::fs::remove_dir(&missing);
        assert!(open_directory(&missing).is_err());
    }

    /// 真机取证（默认 `#[ignore]`，CI 不跑）：实际调用 ShellExecuteW "open"
    /// 打开一个临时目录，确认 shell 链路返回成功。会弹出资源管理器窗口。
    ///
    /// 运行：`cargo test -p sayall-windows --lib -- --ignored open_directory_opens_explorer`
    #[test]
    #[cfg(windows)]
    #[ignore = "会真实打开资源管理器窗口，仅在需要取证时手动运行"]
    fn open_directory_opens_explorer() {
        let directory = std::env::temp_dir().join("sayall-open-directory-probe");
        std::fs::create_dir_all(&directory).expect("创建取证目录失败");
        open_directory(&directory).expect("ShellExecuteW 打开目录应返回成功");
    }
}
