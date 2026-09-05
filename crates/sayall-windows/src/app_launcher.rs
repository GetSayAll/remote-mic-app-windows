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

/// 预设应用表（对齐 Mac 预设 + Windows 常见项）。
pub const PRESET_APPS: &[PresetApp] = &[
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
#[cfg(windows)]
pub fn probe_preset_apps() -> Vec<PresetAppInfo> {
    PRESET_APPS
        .iter()
        .map(|app| PresetAppInfo {
            id: app.id.to_owned(),
            name: app.name.to_owned(),
            installed: app.exe_names.iter().any(|exe| exe_resolvable(exe)),
        })
        .collect()
}

#[cfg(not(windows))]
pub fn probe_preset_apps() -> Vec<PresetAppInfo> {
    Vec::new()
}

/// 激活已运行的预设应用窗口；未运行则启动。
#[cfg(windows)]
pub fn activate_or_launch(id: &str) -> Result<(), String> {
    let app = preset_app(id).ok_or_else(|| format!("未知预设应用：{id}"))?;
    if activate_running(app.exe_names) {
        return Ok(());
    }
    launch_new(app.exe_names)
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

    // 枚举顶层可见窗口：第一个属于目标进程的窗口 → 恢复 + 前置。
    let mut context = EnumContext {
        pids: &pids,
        activated: false,
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_proc),
            LPARAM(&mut context as *mut EnumContext as isize),
        );
    }
    context.activated
}

/// EnumWindows 回调（extern "system" ABI，无捕获）。
#[cfg(windows)]
mod win_impl {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, ShowWindow,
        SW_RESTORE,
    };

    pub(super) struct EnumContext<'a> {
        pub pids: &'a std::collections::HashSet<u32>,
        pub activated: bool,
    }

    pub(super) unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let context = &mut *(lparam.0 as *mut EnumContext);
        if context.activated || !IsWindowVisible(hwnd).as_bool() {
            return BOOL::from(true);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if context.pids.contains(&pid) {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            context.activated = true;
            let _ = SetForegroundWindow(hwnd);
            return BOOL::from(false);
        }
        BOOL::from(true)
    }
}

#[cfg(windows)]
use win_impl::{enum_windows_proc, EnumContext};

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
}
