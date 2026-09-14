//! Public Windows AppsFolder discovery and launch targets.
use crate::app_launcher::CustomAppPick;
pub const REGISTERED_PREFIX: &str = "shell:AppsFolder\\";

pub fn is_registered_target(target: &str) -> bool {
    target.strip_prefix(REGISTERED_PREFIX).is_some_and(|id| {
        !id.is_empty()
            && id.len() <= 4096
            && !id
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | '"'))
    })
}

pub fn normalize_library(apps: Vec<CustomAppPick>) -> Result<Vec<CustomAppPick>, String> {
    if apps.len() > 2000 {
        return Err("应用列表最多支持 2000 项".into());
    }
    let mut unique = std::collections::BTreeMap::new();
    for mut app in apps {
        app.name = app.name.trim().to_owned();
        let lower = app.path.to_ascii_lowercase();
        let file_target = (lower.ends_with(".exe") || lower.ends_with(".lnk"))
            && (app.path.contains('\\') || app.path.contains('/'));
        if app.name.is_empty()
            || app.name.len() > 1024
            || app.path.len() > 8192
            || app.path.chars().any(|c| c.is_control() || c == '"')
            || !(is_registered_target(&app.path) || file_target)
        {
            return Err("应用列表包含无效的名称或启动目标".into());
        }
        unique.entry(lower).or_insert(app);
    }
    let mut apps: Vec<_> = unique.into_values().collect();
    apps.sort_by_key(|app| (app.name.to_lowercase(), app.path.to_lowercase()));
    Ok(apps)
}

#[cfg(windows)]
pub fn scan_registered_apps() -> Result<Vec<CustomAppPick>, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    static SCANNING: AtomicBool = AtomicBool::new(false);
    if SCANNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("应用扫描仍在进行，请稍后重试".into());
    }
    let started = Instant::now();
    crate::gatt_note("registered_apps phase=requested source=windows_appsfolder".to_owned());
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("sayall-apps-scan".into())
        .spawn(move || {
            struct Reset;
            impl Drop for Reset {
                fn drop(&mut self) {
                    SCANNING.store(false, Ordering::Release);
                }
            }
            let _reset = Reset;
            let _ = tx.send(scan_sta(started));
        });
    if let Err(error) = spawned {
        SCANNING.store(false, Ordering::Release);
        return Err(format!("无法启动应用扫描：{error}"));
    }
    let result = rx
        .recv_timeout(Duration::from_secs(15))
        .map_err(|_| "应用扫描超时，请稍后重试".to_owned())
        .and_then(|result| result);
    crate::gatt_note(format!(
        "registered_apps phase=completed terminal_result={} count={} elapsed_ms={}",
        if result.is_ok() { "passed" } else { "failed" },
        result.as_ref().map_or(0, Vec::len),
        started.elapsed().as_millis()
    ));
    result
}

#[cfg(windows)]
fn scan_sta(started: std::time::Instant) -> Result<Vec<CustomAppPick>, String> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem,
        SHCreateItemInKnownFolder, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY,
        SIGDN_PARENTRELATIVEPARSING,
    };
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
    }
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    let _com = Com;
    unsafe fn text(value: PWSTR) -> String {
        let result = unsafe { value.to_string() }.unwrap_or_default();
        unsafe {
            CoTaskMemFree(Some(value.0.cast()));
        }
        result
    }
    let result = (|| -> windows::core::Result<Vec<CustomAppPick>> {
        unsafe {
            let folder: IShellItem =
                SHCreateItemInKnownFolder(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, PCWSTR::null())?;
            let enumeration: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
            let mut apps = Vec::new();
            loop {
                if started.elapsed().as_secs() >= 14 || apps.len() >= 2000 {
                    return Err(windows::core::Error::new(
                        windows::core::HRESULT(0x800705B4u32 as i32),
                        "Application scan limit reached",
                    ));
                }
                let mut items = [None];
                let mut fetched = 0;
                enumeration.Next(&mut items, Some(&mut fetched))?;
                if fetched == 0 {
                    break;
                }
                let Some(item) = items[0].take() else {
                    continue;
                };
                let name = match item.GetDisplayName(SIGDN_NORMALDISPLAY) {
                    Ok(value) => text(value),
                    Err(_) => continue,
                };
                let id = match item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING) {
                    Ok(value) => text(value),
                    Err(_) => continue,
                };
                let path = format!("{REGISTERED_PREFIX}{id}");
                if !name.is_empty() && is_registered_target(&path) {
                    apps.push(CustomAppPick { name, path });
                }
            }
            Ok(apps)
        }
    })();
    normalize_library(result.map_err(|e| format!("读取 Windows 应用列表失败：{e}"))?)
}

#[cfg(not(windows))]
pub fn scan_registered_apps() -> Result<Vec<CustomAppPick>, String> {
    Err("仅 Windows 支持应用扫描".into())
}

#[cfg(windows)]
pub fn launch_registered_app(target: &str) -> Result<(), String> {
    if !is_registered_target(target) {
        return Err("无效的 Windows 应用目标".into());
    }
    let target = target.to_owned();
    crate::gatt_note("registered_app_launch phase=requested".to_owned());
    let started = std::time::Instant::now();
    let result = std::thread::Builder::new()
        .name("sayall-registered-launch".into())
        .spawn(move || {
            use windows::core::{w, PCWSTR};
            use windows::Win32::System::Com::{
                CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
            };
            use windows::Win32::UI::Shell::{
                ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
            };
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                    .ok()
                    .map_err(|e| e.to_string())?;
            }
            let wide: Vec<_> = target.encode_utf16().chain(Some(0)).collect();
            let mut info = SHELLEXECUTEINFOW {
                cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
                lpVerb: w!("open"),
                lpFile: PCWSTR(wide.as_ptr()),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            let result = unsafe { ShellExecuteExW(&mut info) }.map_err(|e| e.to_string());
            unsafe {
                CoUninitialize();
            }
            result
        })
        .map_err(|e| e.to_string())?
        .join()
        .unwrap_or_else(|_| Err("启动线程异常退出".into()));
    crate::gatt_note(format!("registered_app_launch phase=completed terminal_result={} target_result=unknown elapsed_ms={}", if result.is_ok() { "submitted" } else { "failed" }, started.elapsed().as_millis()));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_deduplicates_targets_and_rejects_commands() {
        let app = CustomAppPick {
            name: " Example ".into(),
            path: format!("{REGISTERED_PREFIX}Example.App!Main"),
        };
        let apps = normalize_library(vec![app.clone(), app]).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Example");
        for target in [
            "cmd /c calc",
            "shell:AppsFolder\\",
            "shell:AppsFolder\\bad\\path",
            "shell:AppsFolder\\bad\nvalue",
        ] {
            assert!(normalize_library(vec![CustomAppPick {
                name: "Invalid".into(),
                path: target.into()
            }])
            .is_err());
        }
    }
}
