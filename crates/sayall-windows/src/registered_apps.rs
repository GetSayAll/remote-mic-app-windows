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

/// Known AppsFolder entries whose process has no AUMID.
///
/// WPS Office launches a shared `wps.exe` host and distinguishes modules by
/// window contents; matching only the executable would give the WPP profile to
/// Writer as well. A blank WPP window is `PP12FrameClass`; an opened `.pptx`
/// keeps the generic `OpusApp` top-level class but contains `PPTFrameClass`.
pub fn known_window_class_prefix(app_user_model_id: &str) -> Option<&'static str> {
    match app_user_model_id.to_ascii_lowercase().as_str() {
        "kingsoft.office.wpp" => Some("PP12"),
        _ => None,
    }
}

/// Descendant window class that identifies a WPS module whose top-level class
/// is shared with another module (WPP editor vs Writer both use `OpusApp`).
pub fn known_descendant_window_class_prefix(app_user_model_id: &str) -> Option<&'static str> {
    match app_user_model_id.to_ascii_lowercase().as_str() {
        "kingsoft.office.wpp" => Some("PPT"),
        "kingsoft.office.wps" => Some("_Ww"),
        _ => None,
    }
}

/// 只读判断某个 AppsFolder AUMID 是否已安装（2026-10-22：ChatGPT 等
/// Microsoft Store 包没有 App Paths / 开始菜单 .lnk，必须走 AppsFolder）。
#[cfg(windows)]
pub fn registered_target_is_installed(app_user_model_id: &str) -> bool {
    let target = format!("{REGISTERED_PREFIX}{app_user_model_id}");
    if !is_registered_target(&target) {
        return false;
    }
    // 命令线程可能已经以 MTA 初始化 COM；Shell 解析放到独立 STA 线程，
    // 避免 CoInitializeEx(COINIT_APARTMENTTHREADED) 返回 RPC_E_CHANGED_MODE
    // 被误判成“未安装”。
    std::thread::Builder::new()
        .name("sayall-appsfolder-probe".to_owned())
        .spawn(move || resolve_registered_identity(&target).is_ok())
        .ok()
        .and_then(|handle| handle.join().ok())
        .unwrap_or(false)
}

pub fn normalize_library(apps: Vec<CustomAppPick>) -> Result<Vec<CustomAppPick>, String> {
    if apps.len() > 2000 {
        return Err("应用列表最多支持 2000 项".into());
    }
    let mut unique = std::collections::BTreeMap::new();
    for mut app in apps {
        app.name = app.name.trim().to_owned();
        let lower = app.path.to_ascii_lowercase();
        let file_target = is_file_target(&app.path);
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

/// 文件型启动目标的既有语法：`.exe/.lnk` 且含路径分隔符。
/// 保持旧配置的兼容口径，不在这里追加绝对路径要求。
fn is_file_target(path: &str) -> bool {
    let path = path.trim();
    let lower = path.to_ascii_lowercase();
    (lower.ends_with(".exe") || lower.ends_with(".lnk"))
        && (path.contains('\\') || path.contains('/'))
        && path.len() <= 8192
        && !path.chars().any(|c| c.is_control() || c == '"')
}

/// 扫描到的新文件 target 必须是绝对路径，避免把 `{GUID}\cmd.exe`
/// 这类 shell 相对解析名当成可直接启动的文件。
fn is_absolute_file_target(path: &str) -> bool {
    is_file_target(path) && std::path::Path::new(path).is_absolute()
}

/// 打包应用内部目录受 ACL 保护，按 exe 路径启动不可靠；这类条目应保留
/// AUMID，而不是改成文件 target。
fn is_packaged_internal_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("\\windowsapps\\") || lower.contains("\\windows\\systemapps\\")
}

/// 解析 `{已知文件夹 GUID}\相对路径` 形式的 AppsFolder 解析名。
///
/// `cmd.exe`、`Windows PowerShell`、`系统信息` 等传统系统工具在 AppsFolder
/// 里不是 `shell:AppsFolder\<AUMID>`，而是已知文件夹下的相对路径。
/// `IShellItem::GetDisplayName(SIGDN_FILESYSPATH)` 对这些条目常返回失败；
/// 用公开的 `SHGetKnownFolderPath` 解析前缀 GUID 后再拼接。
#[cfg(windows)]
fn known_folder_relative(relative: &str) -> Option<String> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};

    let (guid_text, remainder) = relative.split_once('\\')?;
    if remainder.is_empty() {
        return None;
    }
    // 只接受已知文件夹下的普通相对组件，拒绝 ..、绝对路径和盘符前缀，
    // 避免 AppsFolder 解析名把目标带出已知文件夹。
    let relative_path = std::path::Path::new(remainder);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }
    let guid = parse_guid(guid_text)?;
    let base = unsafe { SHGetKnownFolderPath(&guid, KF_FLAG_DEFAULT, None) }.ok()?;
    let base_text = unsafe { base.to_string() }.unwrap_or_default();
    unsafe {
        CoTaskMemFree(Some(base.0.cast()));
    }
    if base_text.is_empty() {
        return None;
    }
    std::path::Path::new(&base_text)
        .join(relative_path)
        .to_str()
        .map(str::to_owned)
}

#[cfg(windows)]
fn parse_guid(text: &str) -> Option<windows::core::GUID> {
    let text = text.trim().strip_prefix('{')?.strip_suffix('}')?;
    let parts: Vec<&str> = text.split('-').collect();
    if parts.len() != 5
        || parts[0].len() != 8
        || parts[1].len() != 4
        || parts[2].len() != 4
        || parts[3].len() != 4
        || parts[4].len() != 12
    {
        return None;
    }
    let data1 = u32::from_str_radix(parts[0], 16).ok()?;
    let data2 = u16::from_str_radix(parts[1], 16).ok()?;
    let data3 = u16::from_str_radix(parts[2], 16).ok()?;
    let tail = format!("{}{}", parts[3], parts[4]);
    let mut data4 = [0u8; 8];
    for (index, chunk) in tail.as_bytes().chunks(2).enumerate() {
        data4[index] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(windows::core::GUID {
        data1,
        data2,
        data3,
        data4,
    })
}

/// 明显的卸载程序不作为「打开应用」候选。只按展示名与文件名判断，
/// 不读取程序内容；误判代价只是不在扫描列表里出现，用户仍可手动选择。
fn looks_like_uninstaller(name: &str, target: &str) -> bool {
    let name = name.trim().to_lowercase();
    let file_stem = std::path::Path::new(target)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_lowercase();
    [name.as_str(), file_stem.as_str()]
        .iter()
        .any(|text| text.contains("unins") || text.contains("卸载"))
}

/// 把一条 AppsFolder 条目转换成可持久化的启动目标。
///
/// 优先保留 `shell:AppsFolder\<AUMID>`：打包应用只能按 AUMID 激活。
/// 解析名含路径分隔符时，改用 Shell 解析出的真实文件路径；这样
/// `{GUID}\cmd.exe` 与传统桌面应用的绝对路径都能进入应用库。
fn appsfolder_launch_target(
    name: &str,
    relative: &str,
    filesystem_path: Option<&str>,
) -> Option<String> {
    if relative.is_empty() || name.trim().is_empty() || name.len() > 1024 {
        return None;
    }
    let registered = format!("{REGISTERED_PREFIX}{relative}");
    let target = if is_registered_target(&registered) {
        registered
    } else {
        filesystem_path
            .filter(|path| is_absolute_file_target(path) && !is_packaged_internal_path(path))
            .map(str::to_owned)
            .or_else(|| {
                (is_absolute_file_target(relative) && !is_packaged_internal_path(relative))
                    .then(|| relative.to_owned())
            })?
    };
    (!looks_like_uninstaller(name, &target)).then_some(target)
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
        SHCreateItemFromParsingName, SHCreateItemInKnownFolder, KF_FLAG_DEFAULT,
        SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
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
            let mut registered_count = 0usize;
            let mut file_count = 0usize;
            let mut skipped_count = 0usize;
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
                // AUMID 条目保持原样；解析名是路径的条目（{GUID}\cmd.exe、传统
                // 桌面应用的绝对路径）改用 Shell 的真实文件路径，否则
                // is_registered_target 会因反斜杠把它们整条丢掉。
                let registered_target = format!("{REGISTERED_PREFIX}{id}");
                let filesystem_path = if is_registered_target(&registered_target) {
                    None
                } else {
                    // FILESYSPATH 对 AppsFolder 里的 {GUID}\... 条目常拿不到；
                    // 再试桌面绝对解析名，最后按完整 AppsFolder 目标重新解析一次。
                    let direct = item
                        .GetDisplayName(SIGDN_FILESYSPATH)
                        .ok()
                        .map(|value| text(value));
                    let desktop = item
                        .GetDisplayName(SIGDN_DESKTOPABSOLUTEPARSING)
                        .ok()
                        .map(|value| text(value));
                    let reparsed = {
                        let wide: Vec<u16> =
                            registered_target.encode_utf16().chain(Some(0)).collect();
                        let resolved: windows::core::Result<IShellItem> =
                            SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None);
                        resolved
                            .ok()
                            .and_then(|resolved| resolved.GetDisplayName(SIGDN_FILESYSPATH).ok())
                            .map(|value| text(value))
                    };
                    let known = known_folder_relative(&id);
                    [direct, desktop, reparsed, known]
                        .into_iter()
                        .flatten()
                        .find(|path| {
                            is_absolute_file_target(path) && !is_packaged_internal_path(path)
                        })
                };
                let Some(path) = appsfolder_launch_target(&name, &id, filesystem_path.as_deref())
                else {
                    skipped_count += 1;
                    continue;
                };
                if path.starts_with(REGISTERED_PREFIX) {
                    registered_count += 1;
                } else {
                    file_count += 1;
                }
                apps.push(CustomAppPick { name, path });
            }
            crate::gatt_note(format!(
                "registered_apps phase=filtered registered={registered_count} files={file_count} skipped={skipped_count} kept={}",
                registered_count + file_count
            ));
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
pub fn resolve_registered_executable_path(target: &str) -> Option<String> {
    if !is_registered_target(target) {
        return None;
    }
    resolve_registered_identity(target)
        .ok()
        .and_then(|identity| identity.executable_path)
}

/// 诊断用：返回注册项解析出的 exe 路径（`None` = 解析不到；外层 `None` = 目标非法）。
#[cfg(windows)]
pub fn describe_registered_identity(target: &str) -> Option<Option<String>> {
    if !is_registered_target(target) {
        return None;
    }
    Some(
        resolve_registered_identity(target)
            .ok()
            .and_then(|identity| identity.executable_path),
    )
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

    #[cfg(windows)]
    #[test]
    fn missing_appsfolder_aumid_is_not_installed() {
        assert!(!registered_target_is_installed(
            "SayAll.Definitely.Missing_0000000000000!App"
        ));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires an installed ChatGPT Store package on this machine"]
    fn chatgpt_store_package_is_detected_on_this_machine() {
        assert!(registered_target_is_installed(
            "OpenAI.Codex_2p2nqsd0c76g0!App"
        ));
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

    #[test]
    fn appsfolder_entries_keep_aumids_and_reject_documents_and_uninstallers() {
        assert_eq!(
            appsfolder_launch_target(
                "设置",
                "Microsoft.AutoGenerated.{B775B473-2C80-E64C-6B77-9634BE4A48A2}",
                None,
            ),
            Some(format!(
                "{REGISTERED_PREFIX}Microsoft.AutoGenerated.{{B775B473-2C80-E64C-6B77-9634BE4A48A2}}"
            ))
        );
        assert_eq!(
            appsfolder_launch_target("此电脑", "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}", None),
            Some(format!(
                "{REGISTERED_PREFIX}::{{20D04FE0-3AEA-1069-A2D8-08002B30309D}}"
            ))
        );
        assert_eq!(
            appsfolder_launch_target("Python Manuals", "C:\\Python\\Doc\\index.html", None),
            None
        );
        assert_eq!(
            appsfolder_launch_target("笔记本使用手册", "https://pc.mi.com/sysc.html", None),
            None
        );
        assert_eq!(
            appsfolder_launch_target("卸载百度网盘", "卸载百度网盘", None),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn appsfolder_path_entries_become_absolute_file_targets() {
        assert_eq!(
            appsfolder_launch_target(
                "命令提示符",
                "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\cmd.exe",
                Some("C:\\Windows\\System32\\cmd.exe"),
            ),
            Some("C:\\Windows\\System32\\cmd.exe".into())
        );
        assert_eq!(
            appsfolder_launch_target("微信", "D:\\Apps\\Weixin\\Weixin.exe", None),
            Some("D:\\Apps\\Weixin\\Weixin.exe".into())
        );
        assert_eq!(
            appsfolder_launch_target(
                "Store App",
                "Vendor.App_123!App",
                Some("C:\\Program Files\\WindowsApps\\Vendor.App\\app.exe"),
            ),
            Some(format!("{REGISTERED_PREFIX}Vendor.App_123!App"))
        );
        assert_eq!(
            appsfolder_launch_target(
                "Store Internal",
                "{GUID}\\app.exe",
                Some("C:\\Program Files\\WindowsApps\\Vendor.App\\app.exe"),
            ),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn known_folder_relative_resolves_system_tools_and_rejects_escapes() {
        let resolved = known_folder_relative("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\cmd.exe")
            .expect("System32 known folder should resolve cmd.exe");
        let expected =
            std::path::Path::new(&std::env::var("SystemRoot").expect("SystemRoot should be set"))
                .join("System32")
                .join("cmd.exe");
        assert!(
            resolved.eq_ignore_ascii_case(&expected.to_string_lossy()),
            "unexpected cmd.exe path: {resolved}"
        );
        assert_eq!(
            known_folder_relative("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\..\\evil.exe"),
            None
        );
        assert_eq!(known_folder_relative("{not-a-guid}\\cmd.exe"), None);
    }

    #[cfg(windows)]
    #[test]
    fn library_accepts_file_targets_and_dedupes_case_insensitively() {
        let apps = normalize_library(vec![
            CustomAppPick {
                name: "Command Prompt".into(),
                path: "C:\\Windows\\System32\\cmd.exe".into(),
            },
            CustomAppPick {
                name: "Command Prompt".into(),
                path: "c:\\windows\\system32\\CMD.EXE".into(),
            },
            CustomAppPick {
                name: "WeChat".into(),
                path: "D:\\Apps\\Weixin\\Weixin.exe".into(),
            },
        ])
        .unwrap();
        assert_eq!(apps.len(), 2);
        assert!(apps.iter().any(|app| app.path.ends_with("cmd.exe")));
        assert!(apps.iter().any(|app| app.path.ends_with("Weixin.exe")));
    }
}
