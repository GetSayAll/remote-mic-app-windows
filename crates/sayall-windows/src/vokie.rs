//! Vokie（第三方语音助手）安装检测。
//!
//! 只读：不启动、不修改、不读取它的私有配置；只回答"这台电脑上装没装 Vokie"，
//! 供连接页在未安装时给出官网入口（2026-10-01 Andy 需求）。
//!
//! 判据（任一命中即视为已安装；先命中先返回，便于日志定位误判）：
//! 1. 卸载表 `DisplayName` 含 "vokie"（HKCU / HKLM / WOW6432Node）；
//! 2. 开始菜单快捷方式名含 "vokie"（当前用户 + 所有用户）；
//! 3. `App Paths\Vokie.exe` 注册项。
//!
//! 刻意**不**按安装目录名判断：2026-10-01 本机实测 Vokie 可装在任意盘
//! （如 `D:\Apps\vokie`），而 `%APPDATA%\vokie` 这类数据目录在卸载后仍会残留，
//! 按目录名扫描既漏报又误报。日志只记来源标签，**不记路径**（隐私红线）。

/// 命中判据：用于日志定位误报，不对外展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VokieInstallSource {
    UninstallRegistry,
    StartMenu,
    AppPaths,
}

impl VokieInstallSource {
    pub fn label(self) -> &'static str {
        match self {
            VokieInstallSource::UninstallRegistry => "uninstall_registry",
            VokieInstallSource::StartMenu => "start_menu",
            VokieInstallSource::AppPaths => "app_paths",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VokieInstallation {
    pub installed: bool,
    pub source: Option<VokieInstallSource>,
    /// 进程是否在运行。快捷键冲突的判据是“在跑”而不是“装了”——
    /// 没运行就不会响应右 Alt（2026-10-01 Andy 提出的冲突点）。
    pub running: bool,
}

impl VokieInstallation {
    pub fn source_label(self) -> &'static str {
        self.source.map(VokieInstallSource::label).unwrap_or("none")
    }
}

/// 名称匹配：大小写不敏感地包含 "vokie"。
///
/// 用 `to_lowercase` 而不是 ASCII 小写：安装项名称可能混排中英文，非 ASCII
/// 字符保持不变即可，不会因为大小写转换丢字符。
pub fn matches_vokie(name: &str) -> bool {
    name.to_lowercase().contains("vokie")
}

/// 开始菜单目录扫描（递归、深度与条目数有界）：任一条目名含 "vokie" 即命中。
///
/// 拆成独立函数是为了能在临时目录上做确定性单元测试——真机上的开始菜单
/// 内容不可控，不能拿它当测试夹具。
pub fn directory_tree_contains_vokie(root: &std::path::Path, max_depth: u8) -> bool {
    directory_tree_find_vokie(root, max_depth).is_some()
}

/// 同上，但返回命中的条目路径（用于「打开 Vokie」：直接 ShellExecute 快捷方式）。
pub fn directory_tree_find_vokie(
    root: &std::path::Path,
    max_depth: u8,
) -> Option<std::path::PathBuf> {
    if max_depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(root).ok()?;
    let mut scanned = 0_u32;
    for entry in entries.flatten() {
        // 防御性上限：开始菜单正常规模远小于此，避免异常目录拖慢检测。
        scanned += 1;
        if scanned > 4096 {
            return None;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry.file_type().ok()?;
        if file_type.is_dir() {
            if let Some(found) = directory_tree_find_vokie(&entry.path(), max_depth - 1) {
                return Some(found);
            }
            continue;
        }
        if matches_vokie(&name) {
            return Some(entry.path());
        }
    }
    None
}

#[cfg(windows)]
pub fn detect() -> VokieInstallation {
    let running = vokie_process_running();
    for probe in [
        VokieInstallSource::UninstallRegistry,
        VokieInstallSource::StartMenu,
        VokieInstallSource::AppPaths,
    ] {
        if probe_matches(probe) {
            return VokieInstallation {
                installed: true,
                source: Some(probe),
                running,
            };
        }
    }
    VokieInstallation {
        installed: false,
        source: None,
        running,
    }
}

/// Vokie 进程是否在运行（只匹配进程名，不读路径、不记路径）。
#[cfg(windows)]
fn vokie_process_running() -> bool {
    any_process_name_matches(matches_vokie)
}

/// 进程名遍历（Toolhelp 快照）：任一进程名满足 `predicate` 即为真。
///
/// 抽成谓词形式是为了能做**阳性对照**测试——用"本测试进程自己的名字"验证遍历
/// 真的能看到进程（2026-09-23 教训：没有阳性对照的阴性结论不可信）。
#[cfg(windows)]
fn any_process_name_matches(predicate: impl Fn(&str) -> bool) -> bool {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return false;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut matched = false;
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        let len = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        if predicate(&String::from_utf16_lossy(&entry.szExeFile[..len])) {
            matched = true;
            break;
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };
    matched
}

#[cfg(not(windows))]
pub fn detect() -> VokieInstallation {
    VokieInstallation {
        installed: false,
        source: None,
        running: false,
    }
}

#[cfg(windows)]
fn probe_matches(source: VokieInstallSource) -> bool {
    match source {
        VokieInstallSource::UninstallRegistry => uninstall_registry_has_vokie(),
        VokieInstallSource::StartMenu => start_menu_has_vokie(),
        VokieInstallSource::AppPaths => app_paths_has_vokie(),
    }
}

#[cfg(windows)]
fn start_menu_has_vokie() -> bool {
    start_menu_vokie_shortcut().is_some()
}

/// 开始菜单里 Vokie 快捷方式的完整路径（用于「打开 Vokie」按钮）。
#[cfg(windows)]
fn start_menu_vokie_shortcut() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(programdata) = std::env::var_os("ProgramData") {
        roots.push(PathBuf::from(programdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    // 开始菜单层级通常 ≤ 3（程序 → 厂商 → 快捷方式），给 4 层余量。
    roots
        .iter()
        .find_map(|root| directory_tree_find_vokie(root, 4))
}

/// 启动 Vokie（连接页「打开 Vokie」按钮，2026-10-01 Andy 需求）。
///
/// 入口按可靠性排序：卸载表 `DisplayIcon`（本机实测 `…\Vokie.exe,0`）→
/// 开始菜单快捷方式（ShellExecute 可直接运行 .lnk）。只用于启动，
/// **路径不写日志**（隐私红线）；失败返回用户可读原因（调用方展示）。
#[cfg(windows)]
pub fn launch() -> Result<(), String> {
    if let Some(exe) = uninstall_registry_display_icon()
        .as_deref()
        .and_then(display_icon_exe)
    {
        return crate::app_launcher::launch_path(&exe);
    }
    if let Some(shortcut) = start_menu_vokie_shortcut() {
        return crate::app_launcher::launch_path(&shortcut.to_string_lossy());
    }
    Err("没有找到 Vokie 的启动入口，请从开始菜单手动打开".to_owned())
}

#[cfg(not(windows))]
pub fn launch() -> Result<(), String> {
    Err("打开应用仅在 Windows 上可用".to_owned())
}

/// 解析卸载表 `DisplayIcon` 里的可执行文件路径：去掉引号与 `,<图标索引>` 后缀，
/// 只接受 `.exe`（`@dll,-1` 这类资源引用返回 None）。纯函数，便于单测。
pub fn display_icon_exe(display_icon: &str) -> Option<String> {
    let first = display_icon
        .split(',')
        .next()?
        .trim()
        .trim_matches('"')
        .trim();
    if first.to_ascii_lowercase().ends_with(".exe") {
        Some(first.to_owned())
    } else {
        None
    }
}

#[cfg(windows)]
fn uninstall_registry_has_vokie() -> bool {
    use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    const SUBKEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    const SUBKEY_WOW: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
    [
        (windows::Win32::System::Registry::HKEY_CURRENT_USER, SUBKEY),
        (HKEY_LOCAL_MACHINE, SUBKEY),
        (HKEY_LOCAL_MACHINE, SUBKEY_WOW),
    ]
    .iter()
    .any(|(root, path)| uninstall_root_has_vokie(*root, path))
}

#[cfg(windows)]
fn uninstall_root_has_vokie(root: windows::Win32::System::Registry::HKEY, subkey: &str) -> bool {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, KEY_READ};

    let path = wide(subkey);
    let mut key = windows::Win32::System::Registry::HKEY::default();
    if unsafe { RegOpenKeyExW(root, PCWSTR(path.as_ptr()), None, KEY_READ, &mut key) }.0 != 0 {
        return false;
    }
    let mut index = 0_u32;
    loop {
        let mut name = [0u16; 260];
        let mut len = name.len() as u32;
        let result = unsafe {
            RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if result == ERROR_NO_MORE_ITEMS {
            break;
        }
        if result.0 != 0 {
            break;
        }
        index += 1;
        let entry = String::from_utf16_lossy(&name[..len as usize]);
        if entry_is_vokie_uninstall_entry(key, &entry) {
            unsafe {
                let _ = RegCloseKey(key);
            }
            return true;
        }
    }
    unsafe {
        let _ = RegCloseKey(key);
    }
    false
}

#[cfg(windows)]
fn entry_is_vokie_uninstall_entry(
    root: windows::Win32::System::Registry::HKEY,
    entry: &str,
) -> bool {
    uninstall_entry_string(root, entry, "DisplayName")
        .map(|name| matches_vokie(&name))
        .unwrap_or(false)
}

/// 读取某个卸载表项下的字符串值（REG_SZ）；读不到返回 None。
#[cfg(windows)]
fn uninstall_entry_string(
    root: windows::Win32::System::Registry::HKEY,
    entry: &str,
    value_name: &str,
) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, KEY_READ, REG_SZ,
    };

    let entry_wide = wide(entry);
    let mut key = HKEY::default();
    if unsafe { RegOpenKeyExW(root, PCWSTR(entry_wide.as_ptr()), None, KEY_READ, &mut key) }.0 != 0
    {
        return None;
    }
    let name = wide(value_name);
    let mut buffer = [0u16; 1024];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    let mut kind = windows::Win32::System::Registry::REG_VALUE_TYPE::default();
    let result = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    if result.0 != 0 || kind != REG_SZ || size < 2 {
        return None;
    }
    Some(String::from_utf16_lossy(
        &buffer[..(size as usize / 2).saturating_sub(1)],
    ))
}

/// 命中 Vokie 的卸载表项的 `DisplayIcon`（卸载入口里最可靠的 exe 路径来源；
/// 本机实测 `D:\Apps\vokie\Vokie.exe,0`）。
#[cfg(windows)]
fn uninstall_registry_display_icon() -> Option<String> {
    use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    const SUBKEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    const SUBKEY_WOW: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
    for (root, path) in [
        (windows::Win32::System::Registry::HKEY_CURRENT_USER, SUBKEY),
        (HKEY_LOCAL_MACHINE, SUBKEY),
        (HKEY_LOCAL_MACHINE, SUBKEY_WOW),
    ] {
        if let Some(icon) = uninstall_root_display_icon(root, path) {
            return Some(icon);
        }
    }
    None
}

#[cfg(windows)]
fn uninstall_root_display_icon(
    root: windows::Win32::System::Registry::HKEY,
    subkey: &str,
) -> Option<String> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, KEY_READ};

    let path = wide(subkey);
    let mut key = windows::Win32::System::Registry::HKEY::default();
    if unsafe { RegOpenKeyExW(root, PCWSTR(path.as_ptr()), None, KEY_READ, &mut key) }.0 != 0 {
        return None;
    }
    let mut index = 0_u32;
    let mut found = None;
    loop {
        let mut name = [0u16; 260];
        let mut len = name.len() as u32;
        let result = unsafe {
            RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if result == ERROR_NO_MORE_ITEMS {
            break;
        }
        if result.0 != 0 {
            break;
        }
        index += 1;
        let entry = String::from_utf16_lossy(&name[..len as usize]);
        let display_name = uninstall_entry_string(key, &entry, "DisplayName");
        if display_name.as_deref().map(matches_vokie).unwrap_or(false) {
            found = uninstall_entry_string(key, &entry, "DisplayIcon");
            break;
        }
    }
    unsafe {
        let _ = RegCloseKey(key);
    }
    found
}

#[cfg(windows)]
fn app_paths_has_vokie() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
    };

    let subkey = wide(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\Vokie.exe");
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        let mut key = HKEY::default();
        if unsafe { RegOpenKeyExW(root, PCWSTR(subkey.as_ptr()), None, KEY_READ, &mut key) }.is_ok()
        {
            unsafe {
                let _ = RegCloseKey(key);
            }
            return true;
        }
    }
    false
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_vokie_is_case_insensitive_and_partial() {
        assert!(matches_vokie("Vokie 1.5.25"));
        assert!(matches_vokie("Vokie.lnk"));
        assert!(matches_vokie("vokie-updater"));
        assert!(matches_vokie("我的 Vokie"));
        // 包含匹配：带后缀也命中（宁可多认，也不漏过真实安装项）。
        assert!(matches_vokie("Vokiex"));
        assert!(!matches_vokie("VSCode"));
        assert!(!matches_vokie(""));
    }

    #[test]
    fn directory_scan_finds_shortcut_and_respects_depth() {
        let base = std::env::temp_dir().join(format!("sayall-vokie-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let nested = base.join("厂商").join("工具");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("Vokie.lnk"), b"").unwrap();
        assert!(directory_tree_contains_vokie(&base, 4));
        // 深度不足：只在第 3 层命中，深度 2 看不到。
        assert!(!directory_tree_contains_vokie(&base, 2));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn directory_scan_ignores_unrelated_entries_and_missing_roots() {
        let base =
            std::env::temp_dir().join(format!("sayall-vokie-scan-miss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("微信输入法")).unwrap();
        assert!(!directory_tree_contains_vokie(&base, 4));
        assert!(!directory_tree_contains_vokie(
            &base.join("does-not-exist"),
            4
        ));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn display_icon_exe_parses_registry_forms() {
        // 本机实测形态：exe 路径 + ",0" 图标索引。
        assert_eq!(
            display_icon_exe(r"D:\Apps\vokie\Vokie.exe,0").as_deref(),
            Some(r"D:\Apps\vokie\Vokie.exe")
        );
        // 带引号 / 前后空格。
        assert_eq!(
            display_icon_exe(r#" "C:\Program Files\Vokie\Vokie.exe" , 1 "#).as_deref(),
            Some(r"C:\Program Files\Vokie\Vokie.exe")
        );
        // 无索引、大写扩展名。
        assert_eq!(
            display_icon_exe(r"C:\Vokie\VOKIE.EXE").as_deref(),
            Some(r"C:\Vokie\VOKIE.EXE")
        );
        // 资源引用（@dll,-1）与非 exe：不当作可执行入口。
        assert_eq!(display_icon_exe(r"@C:\Vokie\vokie.dll,-1"), None);
        assert_eq!(display_icon_exe(""), None);
    }

    #[test]
    fn directory_scan_returns_shortcut_path() {
        let base = std::env::temp_dir().join(format!("sayall-vokie-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let nested = base.join("厂商");
        std::fs::create_dir_all(&nested).unwrap();
        let shortcut = nested.join("Vokie.lnk");
        std::fs::write(&shortcut, b"").unwrap();
        assert_eq!(
            directory_tree_find_vokie(&base, 4).as_deref(),
            Some(shortcut.as_path())
        );
        assert_eq!(directory_tree_find_vokie(&base, 1), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 真机取证（默认 `#[ignore]`，CI 不跑）：本机装有 Vokie 时，「打开 Vokie」
    /// 必须能解析出启动入口（只打印"有没有"，不落路径）。
    #[cfg(windows)]
    #[test]
    #[ignore = "真机取证：依赖本机实际安装的 Vokie"]
    fn launch_entry_resolves_on_this_machine() {
        let icon = uninstall_registry_display_icon();
        let shortcut = start_menu_vokie_shortcut();
        println!(
            "vokie launch entry: display_icon={} start_menu={}",
            icon.is_some(),
            shortcut.is_some()
        );
        assert!(
            icon.is_some() || shortcut.is_some(),
            "应能解析出至少一个启动入口"
        );
        if let Some(icon) = icon.as_deref() {
            assert!(
                display_icon_exe(icon).is_some(),
                "DisplayIcon 应能解析出 exe"
            );
        }
    }

    #[test]
    fn detection_source_labels_are_stable() {
        assert_eq!(
            VokieInstallSource::UninstallRegistry.label(),
            "uninstall_registry"
        );
        assert_eq!(VokieInstallSource::StartMenu.label(), "start_menu");
        assert_eq!(VokieInstallSource::AppPaths.label(), "app_paths");
        assert_eq!(
            VokieInstallation {
                installed: false,
                source: None,
                running: false,
            }
            .source_label(),
            "none"
        );
    }

    /// 阳性对照：进程遍历必须能看见**本测试进程自己**（用自身 exe 名做谓词）。
    /// 没有这条，`running=false` 的阴性结论无法与"遍历根本没工作"区分。
    #[cfg(windows)]
    #[test]
    fn process_scan_sees_its_own_process() {
        let own = std::env::current_exe().expect("current exe");
        let stem = own
            .file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        assert!(!stem.is_empty(), "自身进程名不应为空");
        let stem_lower = stem.to_ascii_lowercase();
        assert!(
            any_process_name_matches(|name| {
                let lowered = name.to_ascii_lowercase();
                let trimmed = lowered.strip_suffix(".exe").unwrap_or(lowered.as_str());
                trimmed == stem_lower
            }),
            "进程遍历看不到自己（stem={stem}）"
        );
        assert!(!any_process_name_matches(|name| name.eq_ignore_ascii_case(
            "sayall-probe-definitely-not-running.exe"
        )));
    }

    /// 真机取证（默认 `#[ignore]`，CI 不跑）：本机装有 Vokie 时必须命中，
    /// 并打印命中的判据标签与运行状态（不含任何路径）。
    #[cfg(windows)]
    #[test]
    #[ignore = "真机取证：依赖本机实际安装的 Vokie"]
    fn detect_reports_installed_on_this_machine() {
        let result = detect();
        println!(
            "vokie installed={} source={} running={}",
            result.installed,
            result.source_label(),
            result.running
        );
        assert!(
            result.installed,
            "本机应检测到 Vokie（source={}）",
            result.source_label()
        );
    }
}
