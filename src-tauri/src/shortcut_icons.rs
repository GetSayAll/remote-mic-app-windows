//! 快捷方式图标同步（2026-10-03 用户现场要求）。
//!
//! 为什么单独一段：快捷方式的图标**不来自窗口句柄**——`IconLocation` 为 `,0` 时取的
//! 是 exe 内嵌图标，运行期改不了；窗口图标（`ICON_BIG`/`ICON_SMALL`）只覆盖**运行中、
//! 未固定**的任务栏按钮。2026-10-03 现场：切换应用图标后，任务栏按钮与托盘已跟随
//! （窗口图标路径，见 `app_icon`），但**开始菜单磁贴与固定到任务栏的按钮不变**，
//! 因为它们读的是快捷方式图标。
//!
//! 做法：把所选风格的 .ico 落到 `%LOCALAPPDATA%\SayAll\icons\`，改写目标指向本程序
//! 的快捷方式（开始菜单 / 桌面 / 「固定到任务栏」）的 `IconLocation`，再
//! `SHChangeNotify` 让 shell 立刻刷新图标缓存。
//!
//! 边界：只动**目标 exe 等于当前进程 exe** 的 .lnk；卸载类场景由安装器负责（卸载器
//! 会删掉自己创建的快捷方式）。日志只记数量，不记路径（隐私规则）。

use std::path::{Path, PathBuf};

use sayall_core::AppIconIdentifier;

/// 主程序产品名（安装器创建快捷方式用的同一名字）。
const PRODUCT_NAME: &str = "无线麦 SayAll";

/// 嵌入的多尺寸 .ico：`standard`（水彩鸭）由 `scripts/generate-standard-icon.py`
/// 派生；`faceted-duck`（几何鸭，2026-10-04 起同时是 exe / 安装包 / 卸载器的图标，
/// 见 `tauri.conf.json` 的 `bundle.icon`）由 `scripts/generate-app-icons.py` 派生。
const STANDARD_ICO: &[u8] = include_bytes!("../icons/icon.ico");
const FACETED_DUCK_ICO: &[u8] = include_bytes!("../icons/app-icons/faceted-duck.ico");

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShortcutSyncReport {
    pub considered: usize,
    pub updated: usize,
    pub failed: usize,
}

fn icon_file_name(style: AppIconIdentifier) -> &'static str {
    match style {
        AppIconIdentifier::Standard => "sayall-standard.ico",
        AppIconIdentifier::FacetedDuck => "sayall-faceted-duck.ico",
    }
}

fn icon_bytes(style: AppIconIdentifier) -> &'static [u8] {
    match style {
        AppIconIdentifier::Standard => STANDARD_ICO,
        AppIconIdentifier::FacetedDuck => FACETED_DUCK_ICO,
    }
}

/// 目标路径是否就是当前程序（大小写、正反斜杠、`\\?\` 前缀不敏感）。
///
/// 快捷方式里存的可能是短名（8.3）/长名等不同写法——CI 上 `%TEMP%` 就是短名形式
/// （`RUNNER~1`），而 `IShellLink::GetPath` 会把它解析成长名，只做字符串比较会漏改
/// （2026-10-03 CI 实证：`retarget_lnk` 返回 `Ok(false)`）。因此字符串规范化之后仍不
/// 相等时，再用 `canonicalize` 解析两边各自的规范路径比较。
fn matches_target(target: &str, exe: &Path) -> bool {
    fn normalize(value: &str) -> String {
        let trimmed = value.trim();
        trimmed
            .strip_prefix(r"\\?\")
            .unwrap_or(trimmed)
            .replace('/', "\\")
            .to_ascii_lowercase()
    }
    if target.trim().is_empty() {
        return false;
    }
    if normalize(target) == normalize(&exe.to_string_lossy()) {
        return true;
    }
    let canonical = |value: &str| {
        std::fs::canonicalize(value)
            .ok()
            .map(|path| normalize(&path.to_string_lossy()))
    };
    matches!(
        (canonical(target), canonical(&exe.to_string_lossy())),
        (Some(left), Some(right)) if left == right
    )
}

/// 快捷方式候选：三个固定位置的 .lnk（存在才算）+「固定到任务栏」目录下的全部 .lnk。
///
/// 固定位置用安装器的产品名；固定到任务栏的目录里可能有别的应用，改写前还会按目标
/// exe 过滤一次（`matches_target`），这里只负责把候选列全。
fn candidate_lnks(
    start_menu_programs: &Path,
    user_desktop: &Path,
    public_desktop: &Path,
    pinned_taskbar: &Path,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for lnk in [
        start_menu_programs
            .join(PRODUCT_NAME)
            .join(format!("{PRODUCT_NAME}.lnk")),
        user_desktop.join(format!("{PRODUCT_NAME}.lnk")),
        public_desktop.join(format!("{PRODUCT_NAME}.lnk")),
    ] {
        if lnk.is_file() {
            candidates.push(lnk);
        }
    }
    if let Ok(entries) = std::fs::read_dir(pinned_taskbar) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_lnk = path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"));
            if is_lnk && path.is_file() {
                candidates.push(path);
            }
        }
    }
    candidates
}

/// 图标落盘目录：`%LOCALAPPDATA%\SayAll\icons`（与 rc003 停用信号、日志同根）。
fn icon_directory() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("SayAll").join("icons"))
}

fn write_icon_files(directory: &Path) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| format!("创建图标目录失败：{error}"))?;
    for style in [AppIconIdentifier::Standard, AppIconIdentifier::FacetedDuck] {
        let target = directory.join(icon_file_name(style));
        let bytes = icon_bytes(style);
        let up_to_date = std::fs::read(&target)
            .map(|existing| existing == bytes)
            .unwrap_or(false);
        if !up_to_date {
            std::fs::write(&target, bytes).map_err(|error| format!("写入图标文件失败：{error}"))?;
        }
    }
    Ok(())
}

/// 把所选风格的图标同步到快捷方式；返回统计，出错只降级为 failed 计数。
pub fn sync(style: AppIconIdentifier) -> Result<ShortcutSyncReport, String> {
    let exe = std::env::current_exe().map_err(|error| format!("读取当前程序路径失败：{error}"))?;
    let directory = icon_directory().ok_or_else(|| "缺少 LOCALAPPDATA".to_owned())?;
    write_icon_files(&directory)?;
    let icon = directory.join(icon_file_name(style));

    let roaming = std::env::var_os("APPDATA").map(PathBuf::from);
    let user_profile = std::env::var_os("USERPROFILE").map(PathBuf::from);
    let public = std::env::var_os("PUBLIC").map(PathBuf::from);
    let (Some(roaming), Some(user_profile), Some(public)) = (roaming, user_profile, public) else {
        return Err("缺少 shell 目录环境变量".to_owned());
    };
    let candidates = candidate_lnks(
        &roaming
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs"),
        &user_profile.join("Desktop"),
        &public.join("Desktop"),
        &roaming
            .join("Microsoft")
            .join("Internet Explorer")
            .join("Quick Launch")
            .join("User Pinned")
            .join("TaskBar"),
    );
    let mut report = ShortcutSyncReport::default();
    let mut updated_paths = Vec::new();
    for lnk in candidates {
        match retarget_lnk(&lnk, &exe, &icon) {
            Ok(true) => {
                report.updated += 1;
                updated_paths.push(lnk);
            }
            Ok(false) => {}
            Err(_) => report.failed += 1,
        }
        report.considered += 1;
    }
    if !updated_paths.is_empty() {
        notify_shell(&updated_paths);
    }
    Ok(report)
}

#[cfg(windows)]
mod windows_impl {
    use super::matches_target;
    use std::path::Path;

    /// 在 STA 线程里改写一个 .lnk 的 IconLocation；目标不是本程序时返回 `Ok(false)`。
    pub fn retarget_lnk(lnk: &Path, exe: &Path, icon: &Path) -> Result<bool, String> {
        let lnk = lnk.to_owned();
        let exe = exe.to_owned();
        let icon = icon.to_owned();
        let handle = std::thread::Builder::new()
            .name("sayall-shortcut-icons".to_owned())
            .spawn(move || unsafe {
                use windows::core::{Interface, PCWSTR};
                use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
                use windows::Win32::System::Com::{
                    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
                    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READWRITE,
                };
                use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

                let wide = |value: &Path| -> Vec<u16> {
                    value
                        .to_string_lossy()
                        .encode_utf16()
                        .chain(Some(0))
                        .collect()
                };

                if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                    return Err("COM 初始化失败".to_owned());
                }
                let result = (|| -> Result<bool, String> {
                    let shell_link: IShellLinkW =
                        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                            .map_err(|error| format!("创建 IShellLink 失败：{error}"))?;
                    let persist: IPersistFile = shell_link
                        .cast()
                        .map_err(|error| format!("取 IPersistFile 失败：{error}"))?;
                    let lnk_wide = wide(&lnk);
                    persist
                        .Load(PCWSTR(lnk_wide.as_ptr()), STGM_READWRITE)
                        .map_err(|error| format!("加载快捷方式失败：{error}"))?;
                    let mut file_buf = [0u16; 1040];
                    let mut find_data = WIN32_FIND_DATAW::default();
                    shell_link
                        .GetPath(&mut file_buf, &mut find_data, 0)
                        .map_err(|error| format!("读取快捷方式目标失败：{error}"))?;
                    let length = file_buf
                        .iter()
                        .position(|value| *value == 0)
                        .unwrap_or(file_buf.len());
                    let target = String::from_utf16_lossy(&file_buf[..length]);
                    if !matches_target(&target, &exe) {
                        return Ok(false);
                    }
                    let icon_wide = wide(&icon);
                    shell_link
                        .SetIconLocation(PCWSTR(icon_wide.as_ptr()), 0)
                        .map_err(|error| format!("设置快捷方式图标失败：{error}"))?;
                    persist
                        .Save(PCWSTR::null(), true)
                        .map_err(|error| format!("保存快捷方式失败：{error}"))?;
                    Ok(true)
                })();
                CoUninitialize();
                result
            })
            .map_err(|error| format!("启动快捷方式线程失败：{error}"))?;
        handle
            .join()
            .map_err(|_| "快捷方式线程异常退出".to_owned())?
    }

    /// shell 图标缓存通知：逐条 UPDATEITEM + 一次 ASSOCCHANGED（后者覆盖开始菜单磁贴缓存）。
    pub fn notify_shell(paths: &[std::path::PathBuf]) {
        use windows::Win32::UI::Shell::{
            SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNE_UPDATEITEM, SHCNF_IDLIST, SHCNF_PATHW,
        };
        for path in paths {
            let wide: Vec<u16> = path
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect();
            unsafe {
                SHChangeNotify(
                    SHCNE_UPDATEITEM,
                    SHCNF_PATHW,
                    Some(wide.as_ptr() as *const core::ffi::c_void),
                    None,
                );
            }
        }
        unsafe {
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
        }
    }
}

#[cfg(windows)]
use windows_impl::{notify_shell, retarget_lnk};

#[cfg(not(windows))]
fn retarget_lnk(_lnk: &Path, _exe: &Path, _icon: &Path) -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(windows))]
fn notify_shell(_paths: &[PathBuf]) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_current_executable_is_retargeted() {
        let exe = Path::new(r"C:\Users\demo\AppData\Local\SayAll\sayall-windows-app.exe");
        assert!(matches_target(
            r"C:\Users\demo\AppData\Local\SayAll\sayall-windows-app.exe",
            exe
        ));
        assert!(matches_target(
            "c:/users/demo/appdata/local/sayall/SAYALL-WINDOWS-APP.EXE",
            exe
        ));
        assert!(!matches_target(r"C:\Windows\notepad.exe", exe));
        assert!(!matches_target("", exe));
        // `\\?\` 前缀（canonicalize 的输出形态）不能因为前缀就把自己漏掉。
        assert!(matches_target(
            r"\\?\C:\Users\demo\AppData\Local\SayAll\sayall-windows-app.exe",
            exe
        ));
        // 路径形态差异（这里用 `..` 段模拟短名/长名这类写法差异）：由 canonicalize 兜底。
        let real = std::env::temp_dir().join("sayall-shortcut-match-test.exe");
        std::fs::write(&real, b"stub").expect("create stub exe");
        let detoured = std::env::temp_dir()
            .join("..")
            .join(
                std::env::temp_dir()
                    .file_name()
                    .unwrap_or_default()
                    .to_owned(),
            )
            .join("sayall-shortcut-match-test.exe");
        assert_ne!(
            detoured.to_string_lossy(),
            real.to_string_lossy(),
            "前提：两种写法字符串不同"
        );
        assert!(matches_target(&detoured.to_string_lossy(), &real));
        assert!(!matches_target(
            r"C:\definitely\not\sayall-shortcut-match-test.exe",
            &real
        ));
        let _ = std::fs::remove_file(&real);
    }

    #[test]
    fn candidates_cover_start_menu_desktop_and_pinned_directory() {
        let root = std::env::temp_dir().join("sayall-shortcut-candidates-test");
        let start_menu = root.join("start-menu");
        let user_desktop = root.join("desktop");
        let public_desktop = root.join("public-desktop");
        let pinned = root.join("pinned");
        for directory in [&start_menu, &user_desktop, &public_desktop, &pinned] {
            let _ = std::fs::remove_dir_all(directory);
            std::fs::create_dir_all(directory).expect("create test directory");
        }
        let product_dir = start_menu.join(PRODUCT_NAME);
        std::fs::create_dir_all(&product_dir).expect("create product folder");
        let start_lnk = product_dir.join(format!("{PRODUCT_NAME}.lnk"));
        let desktop_lnk = user_desktop.join(format!("{PRODUCT_NAME}.lnk"));
        let pinned_lnk = pinned.join("Other App.lnk");
        for path in [&start_lnk, &desktop_lnk, &pinned_lnk] {
            std::fs::write(path, b"stub").expect("create stub lnk");
        }

        let candidates = candidate_lnks(&start_menu, &user_desktop, &public_desktop, &pinned);
        assert!(candidates.contains(&start_lnk));
        assert!(candidates.contains(&desktop_lnk));
        assert!(candidates.contains(&pinned_lnk));
        assert_eq!(candidates.len(), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn both_styles_have_their_own_icon_file() {
        assert_ne!(
            icon_file_name(AppIconIdentifier::Standard),
            icon_file_name(AppIconIdentifier::FacetedDuck)
        );
        assert!(icon_bytes(AppIconIdentifier::Standard).len() > 1024);
        assert!(icon_bytes(AppIconIdentifier::FacetedDuck).len() > 1024);
    }

    #[cfg(windows)]
    #[test]
    fn retarget_writes_the_icon_location_only_for_our_target() {
        let root = std::env::temp_dir().join("sayall-shortcut-retarget-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create test directory");
        let dummy_exe = root.join("sayall-windows-app.exe");
        std::fs::write(&dummy_exe, b"stub").expect("create dummy exe");
        let icon = root.join("sayall-faceted-duck.ico");
        std::fs::write(&icon, b"stub").expect("create dummy icon");
        let ours = root.join("ours.lnk");
        let other = root.join("other.lnk");
        create_shortcut(&ours, &dummy_exe);
        create_shortcut(&other, &root.join("notepad.exe"));

        assert_eq!(retarget_lnk(&ours, &dummy_exe, &icon), Ok(true));
        assert_eq!(
            read_icon_location(&ours),
            icon.to_string_lossy().to_string()
        );
        // 别的应用（目标不同）必须原样不动。
        assert_eq!(retarget_lnk(&other, &dummy_exe, &icon), Ok(false));
        assert!(read_icon_location(&other).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 测试用：用 COM 造一个指向 `target` 的 .lnk。
    #[cfg(windows)]
    fn create_shortcut(lnk: &Path, target: &Path) {
        let lnk = lnk.to_owned();
        let target = target.to_owned();
        std::thread::Builder::new()
            .name("sayall-shortcut-create".to_owned())
            .spawn(move || unsafe {
                use windows::core::{Interface, PCWSTR};
                use windows::Win32::System::Com::{
                    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
                    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
                };
                use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
                if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                    return;
                }
                let shell_link: IShellLinkW =
                    CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
                let target_wide: Vec<u16> = target
                    .to_string_lossy()
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                shell_link
                    .SetPath(PCWSTR(target_wide.as_ptr()))
                    .expect("set shortcut target");
                let persist: IPersistFile = shell_link.cast().unwrap();
                let lnk_wide: Vec<u16> = lnk
                    .to_string_lossy()
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                persist
                    .Save(PCWSTR(lnk_wide.as_ptr()), true)
                    .expect("save shortcut");
                CoUninitialize();
            })
            .expect("spawn shortcut creator")
            .join()
            .expect("join shortcut creator");
    }

    /// 测试用：读回 .lnk 的 IconLocation 原始字符串（空 = 未设置）。
    #[cfg(windows)]
    fn read_icon_location(lnk: &Path) -> String {
        let lnk = lnk.to_owned();
        std::thread::Builder::new()
            .name("sayall-shortcut-read".to_owned())
            .spawn(move || unsafe {
                use windows::core::{Interface, PCWSTR};
                use windows::Win32::System::Com::{
                    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
                    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
                };
                use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
                if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                    return String::new();
                }
                let shell_link: IShellLinkW =
                    CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
                let persist: IPersistFile = shell_link.cast().unwrap();
                let lnk_wide: Vec<u16> = lnk
                    .to_string_lossy()
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                persist
                    .Load(PCWSTR(lnk_wide.as_ptr()), STGM_READ)
                    .expect("load shortcut");
                let mut icon_buf = [0u16; 1040];
                let mut index = 0i32;
                shell_link
                    .GetIconLocation(&mut icon_buf, &mut index)
                    .expect("read icon location");
                let length = icon_buf
                    .iter()
                    .position(|value| *value == 0)
                    .unwrap_or(icon_buf.len());
                let value = String::from_utf16_lossy(&icon_buf[..length]);
                CoUninitialize();
                value
            })
            .expect("spawn icon reader")
            .join()
            .expect("join icon reader")
    }
}
