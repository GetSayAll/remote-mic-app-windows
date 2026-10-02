//! Public Windows AppsFolder discovery and launch targets.
use crate::app_launcher::CustomAppPick;
pub const REGISTERED_PREFIX: &str = "shell:AppsFolder\\";

#[derive(Debug, Clone)]
struct RegisteredIdentity {
    app_user_model_id: String,
    executable_path: Option<String>,
}

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

/// 启动结果的分类（2026-09-28 Andy 定稿）：**应用已启动即算成功**——前台
/// 读回失败（Windows 前台锁、冷启动窗口创建慢等）不再构成用户可见错误
/// （此前会拼成「打开应用失败：应用已启动，但 Windows 未将其窗口切换到
/// 前台」弹出提示条，用户明确不需要）；前台观察结果只进结构化日志。
/// `(false, _)` = 启动请求本身未被接受，仍是失败。
fn classify_registered_launch(submitted: bool, foreground_observed: bool) -> Result<(), String> {
    match (submitted, foreground_observed) {
        (true, _) => Ok(()),
        (false, _) => Err("Windows 未接受应用启动请求".into()),
    }
}

#[cfg(windows)]
fn resolve_registered_identity(target: &str) -> Result<RegisteredIdentity, String> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::{
        CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellItem2, SHCreateItemFromParsingName};

    if !is_registered_target(target) {
        return Err("无效的 Windows 应用目标".into());
    }
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|error| error.to_string())?;
    }
    let result = (|| -> windows::core::Result<Option<String>> {
        let wide: Vec<_> = target.encode_utf16().chain(Some(0)).collect();
        let item: IShellItem2 =
            unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }?;
        const PKEY_LINK_TARGET_PARSING_PATH: PROPERTYKEY = PROPERTYKEY {
            fmtid: windows::core::GUID::from_u128(0xb9b4b3fc_2b51_4a42_b5d8_324146afcf25),
            pid: 2,
        };
        let value: PWSTR = match unsafe { item.GetString(&PKEY_LINK_TARGET_PARSING_PATH) } {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let text = unsafe { value.to_string() }.unwrap_or_default();
        unsafe {
            CoTaskMemFree(Some(value.0.cast()));
        }
        let path = std::path::Path::new(&text);
        Ok((text.len() <= 8192
            && !text.chars().any(char::is_control)
            && path.is_absolute()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe")))
        .then_some(text))
    })();
    unsafe {
        CoUninitialize();
    }
    Ok(RegisteredIdentity {
        app_user_model_id: target
            .strip_prefix(REGISTERED_PREFIX)
            .expect("registered target was validated")
            .to_owned(),
        executable_path: result.map_err(|error| error.to_string())?,
    })
}

#[cfg(windows)]
pub fn launch_registered_app(target: &str) -> Result<(), String> {
    if !is_registered_target(target) {
        return Err("无效的 Windows 应用目标".into());
    }
    let shell_target = target.to_owned();
    crate::gatt_note("registered_app_launch phase=requested".to_owned());
    let started = std::time::Instant::now();
    let result = std::thread::Builder::new()
        .name("sayall-registered-launch".into())
        // 闭包返回 (结果, 前台是否观察到)：收尾日志必须如实区分「已启动但
        // 未抢到前台」与「启动失败」——(true, false) 自 2026-09-28 起算成功，
        // 日志若仍按 result.is_ok() 推导前台字段就会说谎。
        .spawn(move || -> (Result<(), String>, bool) {
            use windows::core::{w, PCWSTR};
            use windows::Win32::Foundation::CloseHandle;
            use windows::Win32::System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED,
            };
            use windows::Win32::UI::Shell::{
                ApplicationActivationManager, IApplicationActivationManager, ShellExecuteExW,
                AO_NONE, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NOASYNC,
                SHELLEXECUTEINFOW,
            };
            use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
            if let Err(error) = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() } {
                return (Err(error.to_string()), false);
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
            let identity = match resolve_registered_identity(&shell_target) {
                Ok(identity) => identity,
                Err(error) => return (Err(error), false),
            };
            crate::gatt_note(format!(
                "registered_app_launch phase=identity_resolved aumid_available=true executable_path_available={}",
                identity.executable_path.is_some()
            ));
            let app_user_model_id = identity.app_user_model_id;
            let executable_path = identity.executable_path;
            // 收在托盘里的主窗口：契约/ShellExecute 之后先看应用是否自己把它显示出来，
            // 没显示才走旧的显示路径（见下方 2026-10-03 说明）。
            let mut deferred_hidden_window: Option<isize> = None;

            // 「已运行 → 切回已有窗口」必须先于激活契约：Word / PowerPoint / WPS
            // 这类应用只要走到 ActivateApplication 就会新开实例或文档/首页窗口，
            // 之后的前台读回只能把那个新窗口置前（2026-10-02 用户实测）。
            // 能力本就在 app_launcher 里（窗口 AUMID → 进程 AUMID → exe 路径），
            // 这里只是在启动前先试一次；都失败才认为确实没在运行。
            //
            // 例外（2026-10-03 真机）：目标应用**只有隐藏窗口**（收在托盘，如 WorkBuddy）
            // 时不能直接显示它——应用内部仍认为窗口是隐藏的，客户端区点击不进入应用
            // （"打开后点不动，双击标题栏才活"）。这种情形交给应用自己的激活契约。
            {
                // AUMID 与 exe 路径两个口径都查：WorkBuddy 这类进程没有 AUMID，
                // 只按 AUMID 查会得到 (0,0)，于是永远走不到"应用自己打开"的分支
                // （2026-10-03 用户实测：关闭到托盘后打开不可点击）。
                let by_identity = crate::app_launcher::window_visibility(
                    crate::app_launcher::WindowSelector::AppUserModelId(&app_user_model_id),
                );
                let by_path = executable_path.as_deref().map_or(
                    crate::app_launcher::WindowVisibility::EMPTY,
                    |path| {
                        crate::app_launcher::window_visibility(
                            crate::app_launcher::WindowSelector::ExecutablePath(path),
                        )
                    },
                );
                let visibility = by_identity.merge(by_path);
                let (visible, hidden) = (visibility.visible, visibility.hidden);
                let main_hidden = hidden > 0 && visibility.hidden_max_area > visibility.visible_max_area;
                if (visible == 0 || main_hidden) && hidden > 0 {
                    // 先走应用自己的托盘图标（用户平时把应用叫回来的方式；窗口由应用自己
                    // 恢复，可交互）。触发失败再落到激活契约。
                    let mut names: Vec<String> = Vec::new();
                    if let Some(stem) = executable_path
                        .as_deref()
                        .and_then(|path| path.rsplit(['\\', '/']).next())
                        .map(|name| name.to_string())
                    {
                        names.push(stem.clone());
                        if let Some(base) = stem
                            .strip_suffix(".exe")
                            .or(stem.strip_suffix(".EXE"))
                            .map(str::to_string)
                        {
                            names.push(base);
                        }
                    }
                    if let Some(segment) = app_user_model_id.rsplit('.').next() {
                        names.push(segment.to_string());
                    }
                    let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
                    let tray = crate::tray_icons::invoke_tray_icon(&borrowed);
                    let known_hidden = visibility.largest_hidden;
                    let tray_visible = tray.submitted
                        && crate::app_launcher::readback_hidden_window_shown(known_hidden);
                    crate::gatt_note(format!(
                        "registered_app_launch phase=tray_icon matched={} submitted={} method={} terminal_result={}",
                        tray.matched,
                        tray.submitted,
                        tray.method,
                        if tray_visible { "passed" } else { "failed" },
                    ));
                    if tray_visible {
                        return (Ok(()), true);
                    }
                    deferred_hidden_window = known_hidden;
                    crate::gatt_note(format!(
                        "registered_app_launch phase=prelaunch_activation result=deferred reason=windows_hidden_by_app visible=0 hidden={hidden}"
                    ));
                } else {
                let by_identity =
                    crate::app_launcher::activate_application_window(&app_user_model_id);
                let by_path = !by_identity
                    && executable_path
                        .as_deref()
                        .is_some_and(crate::app_launcher::activate_executable_path);
                // 启动器式目标（实测：WPS 注册项解析到 ksolaunch.exe，真正的文档
                // 进程在同目录的版本子目录里）→ 同安装目录同族进程兜底。
                let by_family = !by_identity
                    && !by_path
                    && executable_path.as_deref().is_some_and(|path| {
                        crate::app_launcher::activate_install_directory_family(
                            path,
                            &app_user_model_id,
                        )
                    });
                if by_identity || by_path || by_family {
                    let source = if by_identity {
                        "app_identity"
                    } else if by_path {
                        "executable_path"
                    } else {
                        "install_directory"
                    };
                    crate::gatt_note(format!(
                        "registered_app_launch phase=prelaunch_activation result=activated source={source}"
                    ));
                    return (Ok(()), true);
                }
                // 只记判定结果，不记路径（隐私规则）：用于区分「没匹配到运行中的
                // 进程」与「匹配到但抢前台被拒」——后者修法完全不同。
                crate::gatt_note(format!(
                    "registered_app_launch phase=prelaunch_activation result=not_running aumid_hit={} executable_path_available={}",
                    by_identity,
                    executable_path.is_some()
                ));
                }
            }

            let id_wide: Vec<_> = app_user_model_id.encode_utf16().chain(Some(0)).collect();
            let activation = (|| -> windows::core::Result<u32> {
                let manager: IApplicationActivationManager = unsafe {
                    CoCreateInstance(
                        &ApplicationActivationManager,
                        None,
                        CLSCTX_INPROC_SERVER,
                    )?
                };
                let (result, unlock) = crate::app_launcher::with_alt_foreground_unlock(|| unsafe {
                    manager.ActivateApplication(PCWSTR(id_wide.as_ptr()), PCWSTR::null(), AO_NONE)
                });
                crate::gatt_note(format!(
                    "registered_app_launch phase=activation_contract method=application_activation_manager terminal_result={} alt_unlock_submitted={} physical_alt_held={}",
                    if result.is_ok() { "submitted" } else { "failed" },
                    unlock.pair_submitted,
                    unlock.physical_alt_held
                ));
                result
            })();

            let (method, submitted, pid) = match activation {
                Ok(pid) => ("application_activation_manager", true, Some(pid)),
                Err(_) => {
                    // AppsFolder 也可能含传统桌面注册项；激活契约不支持时保留
                    // ShellExecuteEx 回退，并尽量从进程句柄取得 PID 做同样的读回。
                    let wide: Vec<_> = shell_target.encode_utf16().chain(Some(0)).collect();
                    let mut info = SHELLEXECUTEINFOW {
                        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI | SEE_MASK_NOCLOSEPROCESS,
                        lpVerb: w!("open"),
                        lpFile: PCWSTR(wide.as_ptr()),
                        nShow: SW_SHOWNORMAL.0,
                        ..Default::default()
                    };
                    let (shell_result, unlock) =
                        crate::app_launcher::with_alt_foreground_unlock(|| unsafe {
                            ShellExecuteExW(&mut info).map_err(|e| e.to_string())
                        });
                    let pid = if !info.hProcess.is_invalid() {
                        let pid = unsafe {
                            windows::Win32::System::Threading::GetProcessId(info.hProcess)
                        };
                        unsafe {
                            let _ = CloseHandle(info.hProcess);
                        }
                        (pid != 0).then_some(pid)
                    } else {
                        None
                    };
                    crate::gatt_note(format!(
                        "registered_app_launch phase=activation_contract method=shell_fallback terminal_result={} pid_available={} alt_unlock_submitted={} physical_alt_held={}",
                        if shell_result.is_ok() { "submitted" } else { "failed" },
                        pid.is_some(),
                        unlock.pair_submitted,
                        unlock.physical_alt_held
                    ));
                    ("shell_fallback", shell_result.is_ok(), pid)
                }
            };

            // 收在托盘的应用：契约/ShellExecute 已经按应用自己的方式请求过一次（Electron
            // 应用会由单实例处理把主窗口显示出来）。先有界复核应用是否自己把窗口显示出来，
            // 是则不再走会被判"点不动"的直接显示路径。
            if let Some(hwnd) = deferred_hidden_window {
                let self_shown = crate::app_launcher::readback_hidden_window_shown(Some(hwnd));
                crate::gatt_note(format!(
                    "registered_app_launch phase=app_self_shown terminal_result={}",
                    if self_shown { "passed" } else { "failed" },
                ));
                if self_shown {
                    return (Ok(()), true);
                }
            }

            let foreground_observed = pid.is_some_and(|pid| {
                observe_registered_foreground(
                    pid,
                    &app_user_model_id,
                    executable_path.as_deref(),
                    method == "shell_fallback",
                )
            });
            crate::gatt_note(format!(
                "registered_app_launch phase=foreground_readback method={method} pid_available={} target_result={}",
                pid.is_some(),
                if foreground_observed { "foreground_observed" } else { "foreground_denied" }
            ));
            (
                classify_registered_launch(submitted, foreground_observed),
                foreground_observed,
            )
        })
        .map_err(|e| e.to_string())?
        .join()
        .unwrap_or_else(|_| (Err("启动线程异常退出".into()), false));
    let (result, foreground_observed) = result;
    crate::gatt_note(format!(
        "registered_app_launch phase=completed terminal_result={} target_result={} elapsed_ms={}",
        if result.is_ok() { "passed" } else { "failed" },
        // 2026-09-28 起 (已启动, 未抢到前台) 也是 passed：字段必须如实区分，
        // 不能再由 result.is_ok() 反推前台状态。
        match (result.is_ok(), foreground_observed) {
            (true, true) => "foreground_observed",
            (true, false) => "launched_without_foreground",
            (false, _) => "launch_failed",
        },
        started.elapsed().as_millis()
    ));
    result
}

#[cfg(windows)]
fn observe_registered_foreground(
    pid: u32,
    app_user_model_id: &str,
    executable_path: Option<&str>,
    allow_pid_fallback: bool,
) -> bool {
    // 冷启动时窗口创建晚于激活契约返回。等待窗口出现并以有限次数尝试恢复/前置；
    // 每次都由 GetForegroundWindow 读回所选主窗口，而不是相信 API 返回值或 PID。
    for delay_ms in [0, 50, 100, 250, 500, 1000] {
        if delay_ms != 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        if crate::app_launcher::activate_application_window(app_user_model_id)
            || executable_path.is_some_and(crate::app_launcher::activate_executable_path)
            || (allow_pid_fallback && crate::app_launcher::activate_process_window(pid))
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_launch_treats_missing_foreground_as_success() {
        // 2026-09-28 Andy 定稿：应用已启动即算成功——前台读回失败不再构成
        // 用户可见错误（此前会拼成「打开应用失败：应用已启动，但 Windows
        // 未将其窗口切换到前台」弹提示条，用户明确不需要）；前台结果只进日志。
        assert!(classify_registered_launch(true, false).is_ok());
        assert!(classify_registered_launch(true, true).is_ok());
        assert!(
            classify_registered_launch(false, false).is_err(),
            "启动请求未被接受仍是失败"
        );
        assert!(
            classify_registered_launch(false, true).is_err(),
            "未提交却观察到前台属逻辑矛盾，按失败处理"
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires the configured AppsFolder desktop application"]
    fn configured_desktop_registered_app_resolves_executable_identity() {
        let target = std::env::var("SAYALL_TEST_REGISTERED_APP_TARGET")
            .expect("set SAYALL_TEST_REGISTERED_APP_TARGET to an AppsFolder target");
        let identity = resolve_registered_identity(&target).expect("resolve AppsFolder identity");
        assert!(identity.executable_path.is_some());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires an installed registered app and changes the desktop foreground"]
    fn configured_registered_app_reaches_observed_foreground() {
        let target = std::env::var("SAYALL_TEST_REGISTERED_APP_TARGET")
            .expect("set SAYALL_TEST_REGISTERED_APP_TARGET to an AppsFolder target");
        launch_registered_app(&target).expect("registered app should reach observed foreground");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "shows a foreground-lock probe and activates the configured registered app"]
    fn configured_registered_app_survives_foreground_lock() {
        let target = std::env::var("SAYALL_TEST_REGISTERED_APP_TARGET")
            .expect("set SAYALL_TEST_REGISTERED_APP_TARGET to an AppsFolder target");
        let activation =
            crate::app_launcher::tests::with_foreground_lock(|| launch_registered_app(&target));
        activation.expect("registered app should reach foreground after lock retry");
    }

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
