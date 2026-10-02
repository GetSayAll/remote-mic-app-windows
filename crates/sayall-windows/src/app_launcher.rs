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
    /// 额外安装位置候选（`%ENV%` 模板；探测与启动共用，按顺序取首个存在项）。
    pub install_paths: &'static [&'static str],
    /// 开始菜单快捷方式名（不含 .lnk）：覆盖自定义安装目录的兜底探测，
    /// 命中且解析出的目标 exe 属于 `exe_names` 才算已安装。
    pub shortcut_names: &'static [&'static str],
}

/// 预设应用表（对齐 Mac 预设 + Windows 常见项）。无线麦自身排首位
/// （对齐 Mac `PresetApplication.remoteMic`，恒为已安装）。
///
/// 安装探测三级（任一命中即视为已安装）：System32 直存或 App Paths 注册表 →
/// `install_paths` 候选路径 → 开始菜单快捷方式（解析目标 exe 与 `exe_names`
/// 比对）。未安装的条目由 UI 过滤，不在"打开应用"列表展示。
pub const PRESET_APPS: &[PresetApp] = &[
    PresetApp {
        id: "sayall",
        name: "无线麦",
        exe_names: &["sayall-windows-app.exe"],
        install_paths: &[],
        shortcut_names: &[],
    },
    PresetApp {
        id: "wechat",
        name: "微信",
        exe_names: &["WeChat.exe", "Weixin.exe"],
        install_paths: &[
            "%ProgramFiles%\\Tencent\\WeChat\\WeChat.exe",
            "%ProgramFiles(x86)%\\Tencent\\WeChat\\WeChat.exe",
        ],
        shortcut_names: &["微信", "WeChat"],
    },
    PresetApp {
        id: "edge",
        name: "Edge 浏览器",
        exe_names: &["msedge.exe"],
        install_paths: &[],
        shortcut_names: &[],
    },
    PresetApp {
        id: "chrome",
        name: "Chrome 浏览器",
        exe_names: &["chrome.exe"],
        install_paths: &[],
        shortcut_names: &[],
    },
    PresetApp {
        id: "notepad",
        name: "记事本",
        exe_names: &["notepad.exe"],
        install_paths: &[],
        shortcut_names: &[],
    },
    PresetApp {
        id: "calc",
        name: "计算器",
        exe_names: &["calc.exe", "CalculatorApp.exe"],
        install_paths: &[],
        shortcut_names: &[],
    },
    PresetApp {
        id: "explorer",
        name: "文件资源管理器",
        exe_names: &["explorer.exe"],
        install_paths: &[],
        shortcut_names: &[],
    },
    PresetApp {
        id: "netease_music",
        name: "网易云音乐",
        exe_names: &["cloudmusic.exe"],
        install_paths: &[],
        shortcut_names: &[],
    },
    // 2026-10-02 扩充：这些应用常装在自定义目录（如 D:\Apps\vokie），
    // App Paths 与 System32 探测覆盖不到，必须走候选路径/开始菜单兜底。
    PresetApp {
        id: "vokie",
        name: "Vokie",
        exe_names: &["Vokie.exe"],
        install_paths: &["%LOCALAPPDATA%\\Programs\\Vokie\\Vokie.exe"],
        shortcut_names: &["Vokie"],
    },
    PresetApp {
        id: "vscode",
        name: "Visual Studio Code",
        exe_names: &["Code.exe"],
        install_paths: &[
            "%LOCALAPPDATA%\\Programs\\Microsoft VS Code\\Code.exe",
            "%ProgramFiles%\\Microsoft VS Code\\Code.exe",
        ],
        shortcut_names: &["Visual Studio Code", "VS Code"],
    },
    PresetApp {
        id: "cursor",
        name: "Cursor",
        exe_names: &["Cursor.exe"],
        install_paths: &[
            "%LOCALAPPDATA%\\Programs\\cursor\\Cursor.exe",
            "%ProgramFiles%\\cursor\\Cursor.exe",
        ],
        shortcut_names: &["Cursor"],
    },
    PresetApp {
        id: "dimagent",
        name: "DimAgent",
        exe_names: &["DimAgent.exe"],
        install_paths: &[
            "%LOCALAPPDATA%\\Programs\\DimAgent\\DimAgent.exe",
            "%ProgramFiles%\\DimAgent\\DimAgent.exe",
        ],
        shortcut_names: &["DimAgent"],
    },
    PresetApp {
        id: "qq",
        name: "QQ",
        exe_names: &["QQ.exe"],
        install_paths: &[
            "%ProgramFiles%\\Tencent\\QQNT\\QQ.exe",
            "%ProgramFiles(x86)%\\Tencent\\QQ\\Bin\\QQ.exe",
        ],
        shortcut_names: &["QQ"],
    },
    PresetApp {
        id: "feishu",
        name: "飞书",
        exe_names: &["Feishu.exe", "Lark.exe"],
        install_paths: &[
            "%LOCALAPPDATA%\\Feishu\\Feishu.exe",
            "%ProgramFiles%\\Feishu\\Feishu.exe",
            "%ProgramFiles%\\Lark\\Lark.exe",
        ],
        shortcut_names: &["飞书", "Lark"],
    },
    PresetApp {
        id: "hermes",
        name: "Hermes",
        exe_names: &["Hermes.exe"],
        install_paths: &[
            "%LOCALAPPDATA%\\Programs\\Hermes\\Hermes.exe",
            "%LOCALAPPDATA%\\hermes\\hermes-agent\\apps\\desktop\\release\\win-unpacked\\Hermes.exe",
        ],
        shortcut_names: &["Hermes"],
    },
];

/// 展开 `%VAR%` 形式的安装路径模板；任一变量缺失返回 None。
fn expand_install_path(template: &str) -> Option<std::path::PathBuf> {
    let mut expanded = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('%') {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        let name = &after[..end];
        if name.is_empty() {
            return None;
        }
        expanded.push_str(&std::env::var(name).ok()?);
        rest = &after[end + 1..];
    }
    expanded.push_str(rest);
    (!expanded.is_empty()).then(|| std::path::PathBuf::from(expanded))
}

pub fn preset_app(id: &str) -> Option<&'static PresetApp> {
    PRESET_APPS.iter().find(|app| app.id == id)
}

/// 预设应用解析出的启动位置（候选路径或开始菜单快捷方式）。
#[cfg(windows)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct PresetLaunchTarget {
    exe_path: String,
    arguments: Option<String>,
    working_dir: Option<String>,
    /// 诊断日志用来源：`install_path` | `start_menu_shortcut`。
    source: &'static str,
}

/// 开始菜单快捷方式索引：文件名去 `.lnk` 后小写 → 完整路径。
/// 覆盖自定义安装目录（App Paths/System32 探测不到的应用）。
#[cfg(windows)]
fn start_menu_shortcuts() -> Vec<(String, std::path::PathBuf)> {
    let mut out = Vec::new();
    for root in [
        std::env::var_os("APPDATA").map(|value| {
            std::path::PathBuf::from(value).join(r"Microsoft\Windows\Start Menu\Programs")
        }),
        std::env::var_os("ProgramData").map(|value| {
            std::path::PathBuf::from(value).join(r"Microsoft\Windows\Start Menu\Programs")
        }),
    ]
    .into_iter()
    .flatten()
    {
        collect_start_menu_shortcuts(&root, &mut out);
    }
    out
}

/// 递归收集目录下的 `.lnk`（索引构建的独立步骤，便于用临时目录测试）。
#[cfg(windows)]
fn collect_start_menu_shortcuts(
    dir: &std::path::Path,
    out: &mut Vec<(String, std::path::PathBuf)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_start_menu_shortcuts(&path, out);
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.len() <= 4 || !name.to_ascii_lowercase().ends_with(".lnk") {
            continue;
        }
        out.push((name[..name.len() - 4].to_lowercase(), path));
    }
}

/// 解析预设应用启动位置：先按 `install_paths` 候选路径（存在即用），
/// 再按开始菜单快捷方式（名称匹配 + 解析出的 exe 名与 `exe_names` 比对）。
#[cfg(windows)]
fn preset_launch_target(
    app: &PresetApp,
    shortcuts: &[(String, std::path::PathBuf)],
) -> Option<PresetLaunchTarget> {
    for template in app.install_paths {
        let Some(path) = expand_install_path(template) else {
            continue;
        };
        if path.is_file() {
            return Some(PresetLaunchTarget {
                exe_path: path.to_string_lossy().into_owned(),
                arguments: None,
                working_dir: None,
                source: "install_path",
            });
        }
    }
    if app.shortcut_names.is_empty() {
        return None;
    }
    let wanted: Vec<String> = app
        .shortcut_names
        .iter()
        .map(|name| name.to_lowercase())
        .collect();
    let exe_names: Vec<String> = app
        .exe_names
        .iter()
        .map(|name| name.to_lowercase())
        .collect();
    for (stem, path) in shortcuts {
        if !wanted.iter().any(|name| name == stem) {
            continue;
        }
        let Some(resolved) = resolve_lnk(&path.to_string_lossy()) else {
            continue;
        };
        if resolved.exe_path.is_empty() {
            continue;
        }
        let exe_name = std::path::Path::new(&resolved.exe_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !exe_names.iter().any(|name| *name == exe_name) {
            continue;
        }
        return Some(PresetLaunchTarget {
            exe_path: resolved.exe_path,
            arguments: (!resolved.arguments.is_empty()).then_some(resolved.arguments),
            working_dir: (!resolved.working_dir.is_empty()).then_some(resolved.working_dir),
            source: "start_menu_shortcut",
        });
    }
    None
}

/// 启动前解析预设应用的启动位置（自带开始菜单索引）。
#[cfg(windows)]
fn resolve_preset_launch_target(app: &PresetApp) -> Option<PresetLaunchTarget> {
    preset_launch_target(app, &start_menu_shortcuts())
}

/// 探测预设应用安装状态（System32 直存 / App Paths 注册表 / `install_paths`
/// 候选路径 / 开始菜单快捷方式，任一命中）。
/// 无线麦自身恒为已安装（映射运行时它必然在运行）。
#[cfg(windows)]
pub fn probe_preset_apps() -> Vec<PresetAppInfo> {
    let started = std::time::Instant::now();
    // 开始菜单索引只扫一次，供全部条目复用（每页加载调用一次，不能按条目重复遍历）。
    let shortcuts = start_menu_shortcuts();
    let apps: Vec<PresetAppInfo> = PRESET_APPS
        .iter()
        .map(|app| PresetAppInfo {
            id: app.id.to_owned(),
            name: app.name.to_owned(),
            installed: app.id == "sayall"
                || app.exe_names.iter().any(|exe| exe_resolvable(exe))
                || preset_launch_target(app, &shortcuts).is_some(),
        })
        .collect();
    crate::ble::gatt_note(format!(
        "app_launcher action=probe_preset_apps phase=completed total={} installed={} elapsed_ms={}",
        apps.len(),
        apps.iter().filter(|app| app.installed).count(),
        started.elapsed().as_millis()
    ));
    apps
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
            let activation = activate_running(app.exe_names);
            // `activate_running` 对隐藏的自身窗口用 Win32 `ShowWindow` 显示它
            // （同步生效，其后抢前台才有意义），这会让 tao 的
            // `WindowFlags::VISIBLE` 缓存停在 false。只要找到窗口就必须同步，
            // 即使 Windows 最终拒绝前台切换；否则窗口已显示但随后点 X 时
            // `window.hide()` 仍会因"无差异"被跳过（2026-09-16 实测）。
            if !matches!(activation, RunningActivation::NotFound) {
                if let Some(sync) = SELF_SHOW_SYNC.get() {
                    sync();
                    crate::ble::gatt_note(
                        "app_launcher self_show_sync via=host_callback terminal_result=passed"
                            .to_owned(),
                    );
                }
            }
            match activation {
                RunningActivation::Activated(_) => return Ok(()),
                RunningActivation::ForegroundDenied => {
                    return Err("Windows 拒绝将无线麦窗口切换到前台".to_owned());
                }
                RunningActivation::NotFound => {}
            }
            let exe =
                std::env::current_exe().map_err(|error| format!("获取自身路径失败：{error}"))?;
            return launch_explicit(&exe.to_string_lossy(), None, None, true);
        }
        let visibility = window_visibility(WindowSelector::ExecutableNames(app.exe_names));
        match running_window_disposition(
            visibility.visible,
            visibility.visible_max_area,
            visibility.hidden,
            visibility.hidden_max_area,
        ) {
            RunningWindowDisposition::ActivateVisible => match activate_running(app.exe_names) {
                RunningActivation::Activated(_) => return Ok(()),
                RunningActivation::ForegroundDenied => {
                    return Err("Windows 拒绝将目标应用切换到前台".to_owned());
                }
                RunningActivation::NotFound => {}
            },
            RunningWindowDisposition::DelegateToApp => {
                // 应用已运行，但窗口收在托盘里（如微信关闭到托盘）。**先走应用自己的托盘图标**
                // ——这是用户平时"把应用叫回来"的方式，窗口由应用自己恢复，状态一致、可交互
                // （2026-10-03 真机：直接 ShowWindow 出来的窗口点不动）。
                // 托盘触发失败时，再按应用入口重新拉起。
                let mut names: Vec<&str> = app.shortcut_names.to_vec();
                names.extend(app.exe_names.iter().map(|name| {
                    name.strip_suffix(".exe")
                        .or(name.strip_suffix(".EXE"))
                        .unwrap_or(name)
                }));
                let tray = crate::tray_icons::invoke_tray_icon(&names);
                let tray_visible = tray.submitted
                    && match visibility.largest_hidden {
                        Some(hwnd) => readback_with_recheck(
                            || {
                                unsafe {
                                    windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(
                                        windows::Win32::Foundation::HWND(
                                            hwnd as *mut core::ffi::c_void,
                                        ),
                                    )
                                }
                                .as_bool()
                            },
                            || std::thread::sleep(RELAUNCH_VISIBLE_WAIT),
                            RELAUNCH_VISIBLE_CHECKS,
                        ),
                        None => false,
                    };
                crate::ble::gatt_note(format!(
                    "app_launcher action=activate_or_launch preset={} phase=tray_icon matched={} submitted={} method={} terminal_result={}",
                    app.id,
                    tray.matched,
                    tray.submitted,
                    tray.method,
                    if tray_visible { "passed" } else { "failed" },
                ));
                if tray_visible {
                    return Ok(());
                }
                // 应用已运行，但窗口收在托盘里（如微信关闭到托盘）。不要直接把隐藏窗口
                // 摆出来：应用内部仍认为自己是隐藏的，客户端区点击不进入应用
                // （2026-10-03 真机"打开后点不动"）。改为按应用自己的入口重新拉起——
                // 单实例应用会自己把已有窗口正确显示出来，与开始菜单/桌面快捷方式一致。
                crate::ble::gatt_note(format!(
                    "app_launcher action=activate_or_launch preset={} phase=relaunch_entry reason=window_hidden_by_app visible=0 hidden={}",
                    app.id,
                    visibility.hidden
                ));
                let relaunched = if let Some(target) = resolve_preset_launch_target(app) {
                    crate::ble::gatt_note(format!(
                        "app_launcher action=activate_or_launch preset={} phase=launch_target source={} arguments_present={} working_dir_present={}",
                        app.id,
                        target.source,
                        target.arguments.is_some(),
                        target.working_dir.is_some()
                    ));
                    launch_explicit(
                        &target.exe_path,
                        target.arguments.as_deref(),
                        target.working_dir.as_deref(),
                        true,
                    )
                    .is_ok()
                } else {
                    launch_new(app.exe_names).is_ok()
                };
                // 有界等待：必须观察到**原先那个隐藏窗口**自己变可见才算成功。
                // 只数"有没有可见窗口"会被应用一闪而过的登录/提示窗口骗过（实测假阳性）。
                let known_hidden = visibility.largest_hidden;
                let became_visible = readback_with_recheck(
                    || match known_hidden {
                        Some(hwnd) => unsafe {
                            windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(
                                windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void),
                            )
                        }
                        .as_bool(),
                        None => {
                            window_visibility(WindowSelector::ExecutableNames(app.exe_names))
                                .visible
                                > 0
                        }
                    },
                    || std::thread::sleep(RELAUNCH_VISIBLE_WAIT),
                    RELAUNCH_VISIBLE_CHECKS,
                );
                crate::ble::gatt_note(format!(
                    "app_launcher action=activate_or_launch preset={} phase=relaunch_entry terminal_result={} reason={} submitted={relaunched} known_hidden={}",
                    app.id,
                    if became_visible { "passed" } else { "failed" },
                    if became_visible { "app_shown_known_window" } else { "app_window_not_observed" },
                    known_hidden.is_some(),
                ));
                if became_visible {
                    return Ok(());
                }
                // 回落：**定点**显示我们已知的那个隐藏主窗口（面积最大的隐藏候选）。
                // 不能重新枚举再挑"第一个可见窗口"——应用被重新拉起后往往先弹一个
                // 登录/入口小窗口（2026-10-03 实测：微信 295×387 的"进入微信"窗口
                // 正好会抢走回落目标，用户看到的就是那个小窗口）。
                if let Some(hwnd) = known_hidden {
                    let activated = unsafe { win_impl::activate_known_window(hwnd) };
                    crate::ble::gatt_note(format!(
                        "app_launcher action=activate_or_launch preset={} phase=relaunch_fallback target=largest_hidden_window terminal_result={}",
                        app.id,
                        if activated { "passed" } else { "failed" },
                    ));
                    if activated {
                        return Ok(());
                    }
                }
                // 最后兜底：仍按旧的"枚举 + 显示隐藏候选"路径（最坏等于修复前行为）。
                match activate_running(app.exe_names) {
                    RunningActivation::Activated(_) => return Ok(()),
                    RunningActivation::ForegroundDenied => {
                        return Err("Windows 拒绝将目标应用切换到前台".to_owned());
                    }
                    RunningActivation::NotFound => {}
                }
            }
            RunningWindowDisposition::StartFresh => {}
        }
        if let Some(target) = resolve_preset_launch_target(app) {
            // 候选路径/开始菜单解析出的完整路径：自定义安装目录的应用
            // （Vokie、DimAgent、Hermes 等）ShellExecuteW 只拿文件名会找不到。
            // 日志只记来源与副产品可用性，不落任何个人路径。
            crate::ble::gatt_note(format!(
                "app_launcher action=activate_or_launch preset={} phase=launch_target source={} arguments_present={} working_dir_present={}",
                app.id,
                target.source,
                target.arguments.is_some(),
                target.working_dir.is_some()
            ));
            return launch_explicit(
                &target.exe_path,
                target.arguments.as_deref(),
                target.working_dir.as_deref(),
                true,
            );
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
            match activate_running(&[&exe_name]) {
                RunningActivation::Activated(_) => return Ok(()),
                RunningActivation::ForegroundDenied => {
                    return Err("Windows 拒绝将目标应用切换到前台".to_owned());
                }
                RunningActivation::NotFound => {}
            }
            return launch_explicit(id, None, None, true);
        }
        if let Some(resolved) = resolve_lnk(id) {
            if !resolved.exe_path.is_empty() {
                let exe_name = std::path::Path::new(&resolved.exe_path)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                match activate_running(&[&exe_name]) {
                    RunningActivation::Activated(_) => return Ok(()),
                    RunningActivation::ForegroundDenied => {
                        return Err("Windows 拒绝将目标应用切换到前台".to_owned());
                    }
                    RunningActivation::NotFound => {}
                }
                let arguments =
                    (!resolved.arguments.is_empty()).then_some(resolved.arguments.clone());
                let dir =
                    (!resolved.working_dir.is_empty()).then_some(resolved.working_dir.clone());
                return launch_explicit(
                    &resolved.exe_path,
                    arguments.as_deref(),
                    dir.as_deref(),
                    true,
                );
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunningActivation {
    NotFound,
    Activated(ForegroundActivationOutcome),
    ForegroundDenied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ForegroundAttempt {
    set_foreground_ok: bool,
    target_is_foreground: bool,
    alt_unlock_submitted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ForegroundActivationOutcome {
    activated: bool,
    attempt_count: u8,
    alt_unlock_submitted: bool,
    last_set_foreground_ok: bool,
}

/// 已运行应用的窗口处置（纯逻辑，便于单测）。
///
/// 2026-10-03 真机：把收在托盘里的窗口直接 `ShowWindow` 出来，操作系统层面窗口可见、
/// 前台读回也通过，但**应用内部仍认为窗口是隐藏的**，客户端区点击不进入应用——
/// 微信 / WorkBuddy「打开后点不动，双击标题栏才活」。因此只有"已经有可见窗口"时才
/// 直接激活；只有隐藏窗口时交给应用自己的打开路径（预设置信入口重新拉起，注册应用
/// 走激活契约），与开始菜单、桌面快捷方式、任务栏的打开方式一致。
///
/// 比面积的原因（同日追加实测）：主窗口收在托盘、而应用还留着登录/入口小窗口时，
/// 只看"有没有可见窗口"会把那个小窗口当目标（点出来的是它，不是用户要的主窗口）。
/// 因此只要**隐藏的最大窗口比可见的最大窗口更大**，就按"主窗口隐藏"处理。
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunningWindowDisposition {
    /// 已有可见主窗口：显示/还原并强制置前。
    ActivateVisible,
    /// 主窗口隐藏（哪怕还有小窗口可见）：交给应用自己打开，不直接 ShowWindow。
    DelegateToApp,
    /// 没有窗口：按未运行处理，走启动路径。
    StartFresh,
}

#[cfg(windows)]
fn running_window_disposition(
    visible_windows: usize,
    visible_max_area: i64,
    hidden_windows: usize,
    hidden_max_area: i64,
) -> RunningWindowDisposition {
    if hidden_windows > 0 && hidden_max_area > visible_max_area {
        RunningWindowDisposition::DelegateToApp
    } else if visible_windows > 0 {
        RunningWindowDisposition::ActivateVisible
    } else if hidden_windows > 0 {
        RunningWindowDisposition::DelegateToApp
    } else {
        RunningWindowDisposition::StartFresh
    }
}

/// 第一次按常规公开 API 激活；若 Windows 只闪任务栏、前台读回仍不是目标进程，
/// 再用一次成对 Alt 边沿解除 foreground lock 后重试。API 布尔值只作诊断，
/// 最终成功判据必须是 `GetForegroundWindow` 读回属于目标进程。
fn drive_foreground_activation(
    mut attempt: impl FnMut(bool) -> ForegroundAttempt,
) -> ForegroundActivationOutcome {
    let first = attempt(false);
    if first.target_is_foreground {
        return ForegroundActivationOutcome {
            activated: true,
            attempt_count: 1,
            alt_unlock_submitted: false,
            last_set_foreground_ok: first.set_foreground_ok,
        };
    }

    let retry = attempt(true);
    ForegroundActivationOutcome {
        activated: retry.target_is_foreground,
        attempt_count: 2,
        alt_unlock_submitted: retry.alt_unlock_submitted,
        last_set_foreground_ok: retry.set_foreground_ok,
    }
}

/// 复核"已知的那个隐藏窗口"是否已经由应用自己显示出来（有界）。供注册应用路径复用。
#[cfg(windows)]
pub(crate) fn readback_hidden_window_shown(hwnd: Option<isize>) -> bool {
    hwnd.is_some_and(|hwnd| win_impl::known_window_shown(hwnd))
}

/// 重新拉起后等待应用自己显示窗口的复核：6 × 200 ms。冷启动到窗口出现通常在 1 s 内，
/// 没有出现就按失败处理、回落到旧的显示隐藏窗口路径。
const RELAUNCH_VISIBLE_CHECKS: u8 = 6;
const RELAUNCH_VISIBLE_WAIT: std::time::Duration = std::time::Duration::from_millis(200);

/// `SetForegroundWindow` 之后 Windows 可能异步完成前台切换，单次读回会把成功的
/// 置前误判为失败。这里做有界复核：首次读回，失败后等待 `wait()` 再读回，最多
/// `rechecks` 次。成功判据仍是前台读回本身，复核只影响"还要不要重试"。
fn readback_with_recheck(
    mut is_foreground: impl FnMut() -> bool,
    mut wait: impl FnMut(),
    rechecks: u8,
) -> bool {
    if is_foreground() {
        return true;
    }
    for _ in 0..rechecks {
        wait();
        if is_foreground() {
            return true;
        }
    }
    false
}

/// 已运行 → 恢复窗口并前置。区分“未找到”与“找到但 Windows 拒绝前置”，
/// 防止后者被误报为成功或错误地再启动一个实例。
#[cfg(windows)]
fn activate_running(exe_names: &[&str]) -> RunningActivation {
    use std::collections::HashSet;

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
            return RunningActivation::NotFound;
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
        return RunningActivation::NotFound;
    }

    activate_process_windows(&pids)
}

/// 按 Windows 激活契约返回的 PID 查找主窗口，并以与普通 EXE 相同的读回判据
/// 恢复/前置。注册应用的启动提交成功不代表窗口已经到了前台。
#[cfg(windows)]
pub(crate) fn activate_process_window(pid: u32) -> bool {
    let pids = std::collections::HashSet::from([pid]);
    matches!(
        activate_process_windows(&pids),
        RunningActivation::Activated(_)
    )
}

/// AUMID 是 Windows 用来把一个应用的多个进程和窗口关联起来的公开身份。
/// Electron/MSIX 应用的激活契约 PID 可能不是拥有主窗口的 PID，因此按精确
/// AUMID 收集同一应用的进程后再激活，不能只盯契约返回的单个进程。
#[cfg(windows)]
pub(crate) fn activate_application_window(app_user_model_id: &str) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    if matches!(
        activate_windows_by_app_user_model_id(app_user_model_id),
        RunningActivation::Activated(_)
    ) {
        crate::gatt_note(
            "app_launcher action=identity_match source=window_aumid terminal_result=passed"
                .to_owned(),
        );
        return true;
    }

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return false;
    };
    let mut pids = std::collections::HashSet::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        if process_app_user_model_id(entry.th32ProcessID)
            .is_some_and(|value| value.eq_ignore_ascii_case(app_user_model_id))
        {
            pids.insert(entry.th32ProcessID);
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    let activated = !pids.is_empty()
        && matches!(
            activate_process_windows(&pids),
            RunningActivation::Activated(_)
        );
    if activated {
        crate::gatt_note(
            "app_launcher action=identity_match source=process_aumid terminal_result=passed"
                .to_owned(),
        );
    }
    activated
}

#[cfg(windows)]
fn activate_windows_by_app_user_model_id(app_user_model_id: &str) -> RunningActivation {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    let mut context = AppIdentityEnumContext {
        app_user_model_id,
        activation: None,
        hidden_candidates: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_app_identity_windows_proc),
            LPARAM(&mut context as *mut AppIdentityEnumContext as isize),
        );
    }
    if context.activation.is_none() {
        context.activation =
            unsafe { win_impl::activate_hidden_candidates(&context.hidden_candidates) };
    }
    match context.activation {
        Some(outcome) if outcome.activated => RunningActivation::Activated(outcome),
        Some(_) => RunningActivation::ForegroundDenied,
        None => RunningActivation::NotFound,
    }
}

#[cfg(windows)]
pub(crate) fn activate_executable_path(executable_path: &str) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return false;
    };
    let mut pids = std::collections::HashSet::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        if process_image_path(entry.th32ProcessID)
            .is_some_and(|image| process_image_matches_target(&image, executable_path))
        {
            pids.insert(entry.th32ProcessID);
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    !pids.is_empty()
        && matches!(
            activate_process_windows(&pids),
            RunningActivation::Activated(_)
        )
}

fn normalized_windows_path(value: &str) -> String {
    value
        .strip_prefix(r"\\?\")
        .unwrap_or(value)
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn process_image_matches_target(image: &str, target: &str) -> bool {
    normalized_windows_path(image) == normalized_windows_path(target)
}

/// 启动器式注册目标的「同族进程」判定（纯函数，便于单测）。
///
/// 背景（2026-10-02 实测）：`shell:AppsFolder\Kingsoft.Office.WPS` 的
/// `PKEY_Link_TargetParsingPath` 解析结果是启动器 `...\WPS Office\ksolaunch.exe`，
/// 而真正承载文档窗口的进程是 `...\WPS Office\<版本>\office6\wps.exe`。此时
/// 「按精确路径找运行中进程」必然落空，于是每次触发都新开一个窗口。
///
/// 判定规则：候选必须位于解析路径的同一安装目录之下（含子目录），且不是解析
/// 路径本身。返回 `Some(1)` 表示优先候选——exe 基名出现在 AUMID 里（如
/// `Kingsoft.Office.WPS` ↔ `wps.exe`）；`Some(0)` 表示同目录下的其它可执行文件
/// （可用但不优先，避免把「WPS 表格」当成「WPS 文字」）；`None` 表示不相干。
pub fn launcher_family_rank(
    image_path: &str,
    resolved_path: &str,
    app_user_model_id: &str,
) -> Option<u8> {
    let image = normalized_windows_path(image_path);
    let resolved = normalized_windows_path(resolved_path);
    if image.is_empty() || resolved.is_empty() || image == resolved {
        return None;
    }
    let install_dir = resolved.rsplit_once('\\')?.0;
    if install_dir.is_empty() || !image.starts_with(&format!("{install_dir}\\")) {
        return None;
    }
    let file_name = image.rsplit('\\').next().unwrap_or_default();
    let stem = file_name.strip_suffix(".exe").unwrap_or(file_name);
    let aumid = app_user_model_id.to_ascii_lowercase();
    if stem.chars().count() >= 2 && aumid.contains(stem) {
        Some(1)
    } else {
        Some(0)
    }
}

/// 启动器式目标的兜底激活：在同一安装目录下寻找同族进程并前置其窗口。
#[cfg(windows)]
pub(crate) fn activate_install_directory_family(
    resolved_path: &str,
    app_user_model_id: &str,
) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return false;
    };
    let mut preferred = std::collections::HashSet::new();
    let mut fallback = std::collections::HashSet::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        if let Some(image) = process_image_path(entry.th32ProcessID) {
            match launcher_family_rank(&image, resolved_path, app_user_model_id) {
                Some(1) => {
                    preferred.insert(entry.th32ProcessID);
                }
                Some(_) => {
                    fallback.insert(entry.th32ProcessID);
                }
                None => {}
            }
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    let activated = !preferred.is_empty()
        && matches!(
            activate_process_windows(&preferred),
            RunningActivation::Activated(_)
        );
    if activated {
        return true;
    }
    !fallback.is_empty()
        && matches!(
            activate_process_windows(&fallback),
            RunningActivation::Activated(_)
        )
}

#[cfg(windows)]
fn process_image_path(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = vec![0u16; 32768];
    let mut length = buffer.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    }
    .is_ok();
    unsafe {
        let _ = CloseHandle(process);
    }
    queried.then(|| String::from_utf16_lossy(&buffer[..length as usize]))
}

#[cfg(windows)]
fn process_app_user_model_id(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let result = (|| {
        let mut length = 0u32;
        if unsafe { GetApplicationUserModelId(process, &mut length, None) }
            != ERROR_INSUFFICIENT_BUFFER
            || !(2..=4096).contains(&length)
        {
            return None;
        }
        let mut buffer = vec![0u16; length as usize];
        if unsafe {
            GetApplicationUserModelId(process, &mut length, Some(PWSTR(buffer.as_mut_ptr())))
        } != ERROR_SUCCESS
        {
            return None;
        }
        let used = buffer.iter().position(|value| *value == 0)?;
        String::from_utf16(&buffer[..used]).ok()
    })();
    unsafe {
        let _ = CloseHandle(process);
    }
    result
}

/// 窗口可见性查询口径：与 activate_* 系列一致，用于先问"有没有可见窗口"，
/// 再决定是直接激活还是交给应用自己的打开路径。
#[cfg(windows)]
pub(crate) enum WindowSelector<'a> {
    ExecutableNames(&'a [&'a str]),
    ExecutablePath(&'a str),
    AppUserModelId(&'a str),
}

/// 只读枚举结果。`largest_hidden` 是**面积最大**的隐藏候选（通常是应用主窗口），
/// 用于复核"应用是否把我们已知的主窗口自己显示出来"，以及回落时定点显示它——
/// 只看"有没有可见窗口"会被应用一闪而过的登录/入口小窗口骗过（2026-10-03 实测假阳性）。
#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowVisibility {
    pub visible: usize,
    pub hidden: usize,
    pub largest_hidden: Option<isize>,
    pub visible_max_area: i64,
    pub hidden_max_area: i64,
}

#[cfg(windows)]
impl WindowVisibility {
    pub(crate) const EMPTY: Self = Self {
        visible: 0,
        hidden: 0,
        largest_hidden: None,
        visible_max_area: 0,
        hidden_max_area: 0,
    };

    /// 合并两个口径（同一应用的 AUMID 与 exe 路径命中可能重叠，取较大者即可）。
    pub(crate) fn merge(self, other: Self) -> Self {
        Self {
            visible: self.visible.max(other.visible),
            hidden: self.hidden.max(other.hidden),
            largest_hidden: match (self.largest_hidden, other.largest_hidden) {
                (Some(left), Some(right)) => {
                    if self.hidden_max_area >= other.hidden_max_area {
                        Some(left)
                    } else {
                        Some(right)
                    }
                }
                (left, right) => left.or(right),
            },
            visible_max_area: self.visible_max_area.max(other.visible_max_area),
            hidden_max_area: self.hidden_max_area.max(other.hidden_max_area),
        }
    }
}

/// 目标进程/AUMID 当前可见窗口与隐藏窗口的数量（与激活路径同一份过滤规则）。
/// 只读枚举：不显示、不置前、不写任何窗口状态。
#[cfg(windows)]
pub(crate) fn window_visibility(selector: WindowSelector<'_>) -> WindowVisibility {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    let (pids, app_user_model_id) = match selector {
        WindowSelector::ExecutableNames(names) => (pids_for_exe_names(names), None),
        WindowSelector::ExecutablePath(path) => (pids_for_executable_path(path), None),
        WindowSelector::AppUserModelId(aumid) => (pids_for_app_user_model_id(aumid), Some(aumid)),
    };
    if pids.is_empty() && app_user_model_id.is_none() {
        return WindowVisibility::EMPTY;
    }
    let mut context = win_impl::VisibilityEnumContext {
        pids: &pids,
        app_user_model_id,
        visible: 0,
        hidden: 0,
        largest_hidden: None,
        largest_hidden_area: 0,
        visible_max_area: 0,
    };
    unsafe {
        let _ = EnumWindows(
            Some(win_impl::enum_visibility_proc),
            LPARAM(&mut context as *mut win_impl::VisibilityEnumContext as isize),
        );
    }
    WindowVisibility {
        visible: context.visible,
        hidden: context.hidden,
        largest_hidden: context.largest_hidden,
        visible_max_area: context.visible_max_area,
        hidden_max_area: context.largest_hidden_area,
    }
}

/// 进程快照：可执行文件路径 → pid 集合（不同进程的映像路径可能大小写/前缀不同）。
#[cfg(windows)]
fn pids_for_executable_path(executable_path: &str) -> std::collections::HashSet<u32> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut pids = std::collections::HashSet::new();
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return pids;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        if process_image_path(entry.th32ProcessID)
            .as_deref()
            .is_some_and(|image| process_image_matches_target(image, executable_path))
        {
            pids.insert(entry.th32ProcessID);
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
    }
    pids
}

/// 进程快照：可执行文件名 → pid 集合。
#[cfg(windows)]
fn pids_for_exe_names(exe_names: &[&str]) -> std::collections::HashSet<u32> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let wanted: std::collections::HashSet<String> = exe_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    let mut pids = std::collections::HashSet::new();
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return pids;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
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
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
    }
    pids
}

/// 契约 PID 可能不是拥有主窗口的 PID，因此按精确 AUMID 收集同一应用的进程。
#[cfg(windows)]
fn pids_for_app_user_model_id(app_user_model_id: &str) -> std::collections::HashSet<u32> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut pids = std::collections::HashSet::new();
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return pids;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        if process_app_user_model_id(entry.th32ProcessID)
            .is_some_and(|value| value.eq_ignore_ascii_case(app_user_model_id))
        {
            pids.insert(entry.th32ProcessID);
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
    }
    pids
}

#[cfg(windows)]
fn activate_process_windows(pids: &std::collections::HashSet<u32>) -> RunningActivation {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;

    // 枚举顶层窗口：找到目标进程的主窗口（无所有者、非工具窗口）→ 恢复/显示
    // 并强制置前。先优先可见窗口；若只在托盘隐藏，则按枚举顺序回退激活隐藏候选
    // （第一个候选不一定是主窗口，置前失败会试下一个）。
    let mut context = EnumContext {
        pids,
        activation: None,
        hidden_candidates: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_proc),
            LPARAM(&mut context as *mut EnumContext as isize),
        );
    }
    if context.activation.is_none() {
        context.activation =
            unsafe { win_impl::activate_hidden_candidates(&context.hidden_candidates) };
    }
    match context.activation {
        Some(outcome) if outcome.activated => RunningActivation::Activated(outcome),
        Some(_) => RunningActivation::ForegroundDenied,
        None => RunningActivation::NotFound,
    }
}

/// EnumWindows 回调（extern "system" ABI，无捕获）。
#[cfg(windows)]
mod win_impl {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_MENU,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowRect,
        GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_HIDE, SW_RESTORE, SW_SHOW,
        WS_EX_TOOLWINDOW,
    };

    /// 隐藏候选最多记录/尝试的个数：覆盖"消息窗口 + 主窗口 + 辅助窗口"这类组合即可，
    /// 不宜更多——每次尝试都会显示并抢一次前台。
    pub(super) const HIDDEN_CANDIDATE_LIMIT: usize = 3;

    /// 置前读回复核：Windows 可能异步完成前台切换，读回失败要等一小段时间再确认，
    /// 避免把"已经切过去但读回偏早"误判为失败。
    const FOREGROUND_RECHECKS: u8 = 2;
    const FOREGROUND_RECHECK_WAIT: std::time::Duration = std::time::Duration::from_millis(120);

    pub(super) struct EnumContext<'a> {
        pub pids: &'a std::collections::HashSet<u32>,
        pub activation: Option<super::ForegroundActivationOutcome>,
        /// 目标进程的隐藏（如收进托盘）窗口候选，按枚举顺序排列；仅在无可见窗口时
        /// 逐个回退激活。第一个候选不一定是主窗口（消息/托盘窗口可能排在前面）。
        pub hidden_candidates: Vec<HWND>,
    }

    /// 只读枚举：统计目标（pid 集合 / 窗口 AUMID）当前可见与隐藏的候选窗口数量，
    /// 并记下**面积最大**的隐藏候选（通常是主窗口，用于复核与定点回落）。
    pub(super) struct VisibilityEnumContext<'a> {
        pub pids: &'a std::collections::HashSet<u32>,
        pub app_user_model_id: Option<&'a str>,
        pub visible: usize,
        pub hidden: usize,
        pub largest_hidden: Option<isize>,
        pub largest_hidden_area: i64,
        pub visible_max_area: i64,
    }

    pub(super) unsafe extern "system" fn enum_visibility_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let context = &mut *(lparam.0 as *mut VisibilityEnumContext);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let by_pid = context.pids.contains(&pid);
        let by_identity = context.app_user_model_id.is_some_and(|value| {
            window_app_user_model_id(hwnd).is_some_and(|id| id.eq_ignore_ascii_case(value))
        });
        if !(by_pid || by_identity) || !eligible_window_without_logging(hwnd) {
            return BOOL::from(true);
        }
        let mut rect = RECT::default();
        let area = if GetWindowRect(hwnd, &mut rect).is_ok() {
            i64::from(rect.right - rect.left) * i64::from(rect.bottom - rect.top)
        } else {
            0
        };
        if IsWindowVisible(hwnd).as_bool() {
            context.visible += 1;
            context.visible_max_area = context.visible_max_area.max(area);
        } else {
            context.hidden += 1;
            if area > context.largest_hidden_area {
                context.largest_hidden_area = area;
                context.largest_hidden = Some(hwnd.0 as isize);
            }
        }
        BOOL::from(true)
    }

    /// 定点显示并置前一个已知窗口（回落用）。返回是否读回到前台。
    pub(super) unsafe fn activate_known_window(hwnd: isize) -> bool {
        let window = HWND(hwnd as *mut core::ffi::c_void);
        show_and_force_foreground(window).activated
    }

    /// 复核"已知的那个隐藏窗口"是否已经由应用自己显示出来（有界）。
    pub(super) fn known_window_shown(hwnd: isize) -> bool {
        let target = HWND(hwnd as *mut core::ffi::c_void);
        super::readback_with_recheck(
            || unsafe { IsWindowVisible(target) }.as_bool(),
            || std::thread::sleep(super::RELAUNCH_VISIBLE_WAIT),
            super::RELAUNCH_VISIBLE_CHECKS,
        )
    }

    pub(super) struct AppIdentityEnumContext<'a> {
        pub app_user_model_id: &'a str,
        pub activation: Option<super::ForegroundActivationOutcome>,
        pub hidden_candidates: Vec<HWND>,
    }

    fn record_hidden_candidate(candidates: &mut Vec<HWND>, hwnd: HWND) {
        if candidates.len() < HIDDEN_CANDIDATE_LIMIT {
            candidates.push(hwnd);
        }
    }

    fn window_class_name(hwnd: HWND) -> String {
        let mut buffer = [0u16; 256];
        let length = unsafe { GetClassNameW(hwnd, &mut buffer) }.max(0) as usize;
        String::from_utf16_lossy(&buffer[..length])
    }

    #[derive(Debug, Clone, Copy)]
    pub(crate) struct AltUnlockResult {
        pub pair_submitted: bool,
        pub physical_alt_held: bool,
    }

    // 进程/AUMID 只能确定应用身份，不能确定 HWND 的用途。Electron 等应用会
    // 在同一身份下创建崩溃监视、消息、托盘与渲染辅助窗口；它们也可能无 owner、
    // 非 WS_EX_TOOLWINDOW。窗口类只排除已知的框架辅助用途，尺寸兜底排除
    // 尚未布局的消息窗口；仅对 Chromium 主窗口类检查标题是否存在，
    // 不读取标题内容，也不以应用名称作判断。
    pub(super) fn window_rejection_reason(
        class_name: &str,
        width: i32,
        height: i32,
        owned: bool,
        cloaked: bool,
        has_title: bool,
    ) -> Option<&'static str> {
        if owned {
            return Some("owned");
        }
        if cloaked {
            return Some("cloaked");
        }
        // 消息/托盘窗口类：无内容、通常与屏幕同尺寸，但可能无 owner、非工具窗口、
        // 有正常尺寸，从而骗过下面所有兜底。2026-10-03 真机实测：微信收进托盘时
        // `Qt51514WxTrayIconMessageWindowClass`（1920×1025）在 EnumWindows 顺序里
        // 排在真正主窗口 `Qt51514QWindowIcon` 之前；把它当"主窗口"显示出来会得到
        // 一个盖住整屏、点击无反应的空白窗口（用户报告"打开的目标应用点不动"）。
        if class_name.starts_with("crashpad_")
            || class_name == "Base_PowerMessageWindow"
            || class_name == "Chrome_WidgetWin_0"
            || class_name.contains("NotifyIconHostWindow")
            || class_name.contains("SystemPreferencesHostWindow")
            || class_name == "Chrome_StatusTrayWindow"
            || class_name.contains("MessageWindow")
            || class_name.contains("TrayIcon")
        {
            return Some("auxiliary_class");
        }
        // Chromium/Electron 的无标题 Chrome_WidgetWin_1 也可能是预创建的
        // 空白 BrowserWindow。即使它有正常尺寸和 WS_EX_APPWINDOW，也不能
        // 在应用自己的激活契约完成前把它强制显示出来。
        if class_name == "Chrome_WidgetWin_1" && !has_title {
            return Some("untitled_chromium_window");
        }
        if width < 120 || height < 80 {
            return Some("small_or_unlaid_out");
        }
        None
    }

    unsafe fn eligible_window(hwnd: HWND) -> bool {
        eligible_window_with_logging(hwnd, true)
    }

    /// 只读查询用的静默版本：不写候选诊断日志（`window_visibility` 每次按键都会问一次，
    /// 否则会把候选日志翻倍）。
    unsafe fn eligible_window_without_logging(hwnd: HWND) -> bool {
        eligible_window_with_logging(hwnd, false)
    }

    unsafe fn eligible_window_with_logging(hwnd: HWND, log: bool) -> bool {
        let owned = GetWindow(hwnd, GW_OWNER).unwrap_or(HWND::default()).0 != std::ptr::null_mut();
        let tool = (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32) & (WS_EX_TOOLWINDOW.0 as u32) != 0;
        if tool {
            if log {
                crate::ble::gatt_note(
                    "app_launcher action=window_candidate terminal_result=rejected reason=tool_window"
                        .to_owned(),
                );
            }
            return false;
        }
        let class_name = window_class_name(hwnd);
        let class_len = class_name.len();
        let mut rect = RECT::default();
        let has_rect = GetWindowRect(hwnd, &mut rect).is_ok();
        let mut cloaked = 0i32;
        let cloak_result = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
        let reason = if class_len == 0 {
            Some("class_unavailable")
        } else if !has_rect {
            Some("rect_unavailable")
        } else if cloak_result.is_err() {
            Some("cloak_unavailable")
        } else {
            window_rejection_reason(
                &class_name,
                rect.right - rect.left,
                rect.bottom - rect.top,
                owned,
                cloaked != 0,
                GetWindowTextLengthW(hwnd) > 0,
            )
        };
        if log {
            crate::ble::gatt_note(format!(
                "app_launcher action=window_candidate terminal_result={} reason={} class={} visible={} size_class={}",
                if reason.is_none() { "accepted" } else { "rejected" },
                reason.unwrap_or("main_candidate"),
                class_name,
                IsWindowVisible(hwnd).as_bool(),
                if !has_rect { "unknown" } else if rect.right - rect.left < 120 || rect.bottom - rect.top < 80 { "small" } else { "normal" },
            ));
        }
        reason.is_none()
    }

    pub(super) unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let context = &mut *(lparam.0 as *mut EnumContext);
        if context.activation.is_some() {
            return BOOL::from(true);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if !context.pids.contains(&pid) {
            return BOOL::from(true);
        }
        if !eligible_window(hwnd) {
            return BOOL::from(true);
        }
        if IsWindowVisible(hwnd).as_bool() {
            // 可见但被其它窗口遮挡：直接恢复并强制置前。
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            context.activation = Some(show_and_force_foreground(hwnd));
            return BOOL::from(false);
        }
        // 隐藏（如收进托盘）的窗口：记录为候选，枚举结束后再逐个激活。
        record_hidden_candidate(&mut context.hidden_candidates, hwnd);
        BOOL::from(true)
    }

    pub(super) unsafe extern "system" fn enum_app_identity_windows_proc(
        hwnd: HWND,
        lparam: LPARAM,
    ) -> BOOL {
        let context = &mut *(lparam.0 as *mut AppIdentityEnumContext);
        if context.activation.is_some() {
            return BOOL::from(true);
        }
        if !window_app_user_model_id(hwnd)
            .is_some_and(|value| value.eq_ignore_ascii_case(context.app_user_model_id))
            || !eligible_window(hwnd)
        {
            return BOOL::from(true);
        }
        if IsWindowVisible(hwnd).as_bool() {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            context.activation = Some(show_and_force_foreground(hwnd));
            return BOOL::from(false);
        }
        record_hidden_candidate(&mut context.hidden_candidates, hwnd);
        BOOL::from(true)
    }

    unsafe fn window_app_user_model_id(hwnd: HWND) -> Option<String> {
        use windows::Win32::Foundation::PROPERTYKEY;
        use windows::Win32::System::Com::StructuredStorage::{
            PropVariantClear, PropVariantToString,
        };
        use windows::Win32::UI::Shell::PropertiesSystem::{
            IPropertyStore, SHGetPropertyStoreForWindow,
        };

        const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
            fmtid: windows::core::GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
            pid: 5,
        };
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let mut value = store.GetValue(&PKEY_APP_USER_MODEL_ID).ok()?;
        let mut buffer = [0u16; 4096];
        let converted = PropVariantToString(&value, &mut buffer).is_ok();
        let _ = PropVariantClear(&mut value);
        if !converted {
            return None;
        }
        let used = buffer.iter().position(|item| *item == 0)?;
        String::from_utf16(&buffer[..used]).ok()
    }

    /// 显示（若隐藏）/还原（若最小化）目标窗口并强制置前。
    /// 绕过 Windows 前台锁定（foreground lock）：菜单键的"打开应用"事件来自后台
    /// 进程的引擎线程，直接 `SetForegroundWindow` 会静默失败（仅任务栏闪烁、不置前）。
    /// 用 `AttachThreadInput` 把本线程输入挂到当前前台线程，使置前调用被视为前台线程
    /// 发起而获准。不使用 TOPMOST 置顶：短暂的 topmost 状态会污染窗口常驻 Z 序，
    /// 且与 `window.hide()`（关到托盘）交互时会造成窗口无法正常隐藏；仅依赖
    /// attach + SetForegroundWindow（与仓库 wetype_dormancy_probe 已验证的前台切换同款）。
    pub(super) unsafe fn show_and_force_foreground(
        hwnd: HWND,
    ) -> super::ForegroundActivationOutcome {
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
        let mut outcome = super::drive_foreground_activation(|use_alt_unlock| {
            foreground_attempt(hwnd, use_alt_unlock)
        });
        // 前台切换可能异步完成：读回失败时先有界复核，避免误判失败后去显示下一个候选。
        let verified_by_recheck = !outcome.activated
            && super::readback_with_recheck(
                || foreground_matches_window(GetForegroundWindow().0 as isize, hwnd.0 as isize),
                || std::thread::sleep(FOREGROUND_RECHECK_WAIT),
                FOREGROUND_RECHECKS,
            );
        if verified_by_recheck {
            outcome.activated = true;
        }
        crate::ble::gatt_note(format!(
            "app_launcher action=show_and_force_foreground terminal_result={} target_result={} class={} self_window={self_window} visible_before={visible_before} took_show_path={took_show_path} attempt_count={} alt_unlock_submitted={} set_foreground_ok={} verified_by_recheck={verified_by_recheck}",
            if outcome.activated { "passed" } else { "failed" },
            if outcome.activated { "foreground_observed" } else { "foreground_denied" },
            window_class_name(hwnd),
            outcome.attempt_count,
            outcome.alt_unlock_submitted,
            outcome.last_set_foreground_ok,
        ));
        outcome
    }

    /// 按枚举顺序逐个尝试隐藏候选：显示（若隐藏）并置前 → 读回复核。某个候选没有
    /// 拿到前台时，若本次是它被我们显示出来的，先恢复隐藏再试下一个，避免把无内容
    /// 的空白窗口留在屏幕上。全部失败返回 None（调用方按"被拒绝"处理，不会再启实例）。
    pub(super) unsafe fn activate_hidden_candidates(
        candidates: &[HWND],
    ) -> Option<super::ForegroundActivationOutcome> {
        for (index, hwnd) in candidates.iter().take(HIDDEN_CANDIDATE_LIMIT).enumerate() {
            let visible_before = IsWindowVisible(*hwnd).as_bool();
            let outcome = show_and_force_foreground(*hwnd);
            let restored =
                if !outcome.activated && !visible_before && IsWindowVisible(*hwnd).as_bool() {
                    let _ = ShowWindow(*hwnd, SW_HIDE);
                    !IsWindowVisible(*hwnd).as_bool()
                } else {
                    false
                };
            crate::ble::gatt_note(format!(
                "app_launcher action=hidden_candidate_attempt attempt={} class={} terminal_result={} reason={} restored_hidden={restored}",
                index + 1,
                window_class_name(*hwnd),
                if outcome.activated { "passed" } else { "failed" },
                if outcome.activated { "foreground_observed" } else { "foreground_not_observed" },
            ));
            if outcome.activated {
                return Some(outcome);
            }
        }
        None
    }

    unsafe fn foreground_attempt(hwnd: HWND, use_alt_unlock: bool) -> super::ForegroundAttempt {
        let foreground = GetForegroundWindow();
        let foreground_thread = GetWindowThreadProcessId(foreground, None);
        let current_thread = GetCurrentThreadId();
        let attached = foreground_thread != 0 && foreground_thread != current_thread;
        let attach_ok =
            !attached || AttachThreadInput(foreground_thread, current_thread, true).as_bool();

        let (set_foreground_ok, alt_unlock) = if use_alt_unlock {
            with_alt_foreground_unlock(|| SetForegroundWindow(hwnd).as_bool())
        } else {
            (
                SetForegroundWindow(hwnd).as_bool(),
                AltUnlockResult {
                    pair_submitted: false,
                    physical_alt_held: false,
                },
            )
        };

        if attached && attach_ok {
            let _ = AttachThreadInput(foreground_thread, current_thread, false);
        }

        // SetForegroundWindow 的 BOOL 不是产品成功判据；读回选定的前台 HWND。
        let target_is_foreground =
            foreground_matches_window(GetForegroundWindow().0 as isize, hwnd.0 as isize);
        crate::ble::gatt_note(format!(
            "app_launcher action=foreground_attempt use_alt_unlock={use_alt_unlock} physical_alt_held={} attach_requested={attached} attach_ok={attach_ok} alt_unlock_submitted={} set_foreground_ok={set_foreground_ok} target_is_foreground={target_is_foreground}",
            alt_unlock.physical_alt_held,
            alt_unlock.pair_submitted,
        ));
        super::ForegroundAttempt {
            set_foreground_ok,
            target_is_foreground,
            alt_unlock_submitted: alt_unlock.pair_submitted,
        }
    }

    /// Windows 在用户按 Alt 后会解除 foreground lock。这里仅在普通前置失败后
    /// 或即将经 Shell 启动/激活目标时，成对提交 Alt DOWN/UP 包住操作；物理 Alt
    /// 已按住时严格跳过，避免把用户自己的按住态提前释放。
    pub(crate) fn with_alt_foreground_unlock<T>(
        operation: impl FnOnce() -> T,
    ) -> (T, AltUnlockResult) {
        unsafe {
            let physical_alt_held = GetAsyncKeyState(VK_MENU.0 as i32) < 0;
            let alt_down_submitted = !physical_alt_held && submit_alt_edge(false);
            let value = operation();
            let alt_up_submitted = if alt_down_submitted {
                let submitted = submit_alt_edge(true);
                if !submitted {
                    // SendInput 部分失败时再补一次释放，不能把 Alt 留在按下态。
                    let _ = submit_alt_edge(true);
                }
                submitted
            } else {
                false
            };
            (
                value,
                AltUnlockResult {
                    pair_submitted: alt_down_submitted && alt_up_submitted,
                    physical_alt_held,
                },
            )
        }
    }

    pub(super) fn foreground_matches_window(foreground: isize, target: isize) -> bool {
        foreground != 0 && foreground == target
    }

    unsafe fn submit_alt_edge(key_up: bool) -> bool {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(VK_MENU.0),
                    wScan: 0,
                    dwFlags: if key_up {
                        KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0)
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32) == 1
    }
}

#[cfg(windows)]
use win_impl::{
    enum_app_identity_windows_proc, enum_windows_proc, show_and_force_foreground,
    AppIdentityEnumContext, EnumContext,
};

#[cfg(windows)]
pub(crate) use win_impl::with_alt_foreground_unlock;

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
                let (result, unlock) = with_alt_foreground_unlock(|| unsafe {
                    ShellExecuteW(
                        None,
                        PCWSTR(verb.as_ptr()),
                        PCWSTR(wide.as_ptr()),
                        None,
                        None,
                        SW_SHOWNORMAL,
                    )
                });
                crate::ble::gatt_note(format!(
                    "app_launcher action=launch_new phase=foreground_handoff alt_unlock_submitted={} physical_alt_held={}",
                    unlock.pair_submitted, unlock.physical_alt_held
                ));
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
pub(crate) fn launch_path(path: &str) -> Result<(), String> {
    launch_explicit(path, None, None, true)
}

/// 按完整路径启动（可带参数与工作目录；短命 COM 线程内 ShellExecuteW）。
/// 启动后短暂保活线程：覆盖 shell 异步派生链路（.lnk 场景），避免
/// 线程退出中止挂起的启动（2026-09-06 实证）。
#[cfg(windows)]
fn launch_explicit(
    target: &str,
    arguments: Option<&str>,
    working_dir: Option<&str>,
    foreground_unlock: bool,
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
            // 只有"激活/启动应用"路径需要解除 foreground lock。打开目录等无前台
            // 诉求的路径必须跳过 Alt 注入：Alt 按住期间调用 shell 会干扰 explorer，
            // ShellExecuteW 可能长时间不返回（2026-09-26 CI 实测卡死）。
            let (result, unlock) = if foreground_unlock {
                let (value, unlock) = with_alt_foreground_unlock(|| unsafe {
                    ShellExecuteW(
                        None,
                        PCWSTR(verb.as_ptr()),
                        PCWSTR(wide.as_ptr()),
                        args_ptr,
                        dir_ptr,
                        SW_SHOWNORMAL,
                    )
                });
                (value, Some(unlock))
            } else {
                (
                    unsafe {
                        ShellExecuteW(
                            None,
                            PCWSTR(verb.as_ptr()),
                            PCWSTR(wide.as_ptr()),
                            args_ptr,
                            dir_ptr,
                            SW_SHOWNORMAL,
                        )
                    },
                    None,
                )
            };
            let (alt_unlock_submitted, physical_alt_held) = match unlock {
                Some(unlock) => (unlock.pair_submitted, unlock.physical_alt_held),
                None => (false, false),
            };
            crate::ble::gatt_note(format!(
                "app_launcher action=launch_explicit phase=foreground_handoff foreground_unlock={foreground_unlock} alt_unlock_submitted={alt_unlock_submitted} physical_alt_held={physical_alt_held}"
            ));
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
    launch_explicit(&path.to_string_lossy(), None, None, false)
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
pub(crate) mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn window_candidate_rejects_auxiliary_windows_from_real_apps() {
        use super::win_impl::window_rejection_reason;

        assert_eq!(
            window_rejection_reason("crashpad_SessionEndWatcher", 136, 39, false, false, false),
            Some("auxiliary_class")
        );
        assert_eq!(
            window_rejection_reason("Base_PowerMessageWindow", 0, 0, false, false, false),
            Some("auxiliary_class")
        );
        assert_eq!(
            window_rejection_reason("Chrome_WidgetWin_0", 1920, 1019, false, false, false),
            Some("auxiliary_class")
        );
        assert_eq!(
            window_rejection_reason("Electron_NotifyIconHostWindow", 0, 0, false, false, false),
            Some("auxiliary_class")
        );
        assert_eq!(
            window_rejection_reason("Chrome_WidgetWin_1", 960, 720, false, false, true),
            None
        );
        assert_eq!(
            window_rejection_reason("Chrome_WidgetWin_1", 960, 720, false, false, false),
            Some("untitled_chromium_window")
        );
        assert_eq!(
            window_rejection_reason("Chrome_WidgetWin_1", 960, 720, true, false, true),
            Some("owned")
        );
        assert_eq!(
            window_rejection_reason("Chrome_WidgetWin_1", 960, 720, false, true, true),
            Some("cloaked")
        );
        assert_eq!(
            window_rejection_reason("UnknownWindow", 136, 39, false, false, false),
            Some("small_or_unlaid_out")
        );
    }

    /// 真机实测（2026-10-03）：微信在托盘隐藏时，`EnumWindows` 顺序里排在最前的
    /// 不是主窗口，而是托盘图标消息窗口 `Qt51514WxTrayIconMessageWindowClass`
    /// （1920×1025、无 owner、非工具窗口、无内容）。它必须被判为辅助窗口，否则
    /// 会被当作"已运行应用的主窗口"显示出来，得到一个盖住整屏、点不动的空白窗口。
    #[cfg(windows)]
    #[test]
    fn window_candidate_rejects_tray_message_windows() {
        use super::win_impl::window_rejection_reason;

        assert_eq!(
            window_rejection_reason(
                "Qt51514WxTrayIconMessageWindowClass",
                1920,
                1025,
                false,
                false,
                true
            ),
            Some("auxiliary_class")
        );
        assert_eq!(
            window_rejection_reason(
                "DisplayICC_SystemMessageWindow",
                135,
                37,
                false,
                false,
                false
            ),
            Some("auxiliary_class")
        );
        assert_eq!(
            window_rejection_reason("Chrome_SystemMessageWindow", 135, 37, false, false, false),
            Some("auxiliary_class")
        );
        // 负例：同进程的真正主窗口类不能被误伤，否则托盘唤回会找不到窗口。
        assert_eq!(
            window_rejection_reason("Qt51514QWindowIcon", 1134, 865, false, false, true),
            None
        );
        assert_eq!(
            window_rejection_reason("Chrome_WidgetWin_1", 1513, 1180, false, false, true),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn running_window_disposition_prefers_app_owned_opening_for_hidden_windows() {
        use super::*;
        // 已有可见窗口、没有更大的隐藏窗口：直接激活（原有行为，Word/WPS 等）。
        assert_eq!(
            running_window_disposition(1, 900_000, 0, 0),
            RunningWindowDisposition::ActivateVisible
        );
        assert_eq!(
            running_window_disposition(2, 900_000, 3, 500_000),
            RunningWindowDisposition::ActivateVisible
        );
        // 只有隐藏窗口（收在托盘）：交给应用自己打开——直接 ShowWindow 会让应用
        // 内部状态与窗口状态脱节，客户端区点击不进入应用（2026-10-03 真机）。
        assert_eq!(
            running_window_disposition(0, 0, 1, 900_000),
            RunningWindowDisposition::DelegateToApp
        );
        // 主窗口隐藏、只剩登录/入口小窗口可见（实测微信最小化）：仍按"主窗口隐藏"处理，
        // 否则点出来的是那个小窗口，不是用户要的主窗口。
        assert_eq!(
            running_window_disposition(1, 114_000, 1, 942_000),
            RunningWindowDisposition::DelegateToApp
        );
        // 没有窗口：按未运行处理，走启动路径。
        assert_eq!(
            running_window_disposition(0, 0, 0, 0),
            RunningWindowDisposition::StartFresh
        );
    }

    #[cfg(windows)]
    #[test]
    fn foreground_must_be_the_selected_window() {
        assert!(super::win_impl::foreground_matches_window(42, 42));
        assert!(!super::win_impl::foreground_matches_window(42, 43));
    }

    #[test]
    fn executable_identity_normalizes_windows_path_forms() {
        assert!(process_image_matches_target(
            r"\\?\C:\Program Files\Example\Example.exe",
            r"c:/program files/example/example.exe"
        ));
        assert!(!process_image_matches_target(
            r"C:\Program Files\Example\Example.exe",
            r"C:\Other\Example.exe"
        ));
    }

    #[test]
    fn foreground_activation_retries_when_api_only_flashes_taskbar() {
        let mut unlock_flags = Vec::new();
        let mut attempts = vec![
            ForegroundAttempt {
                set_foreground_ok: true,
                target_is_foreground: false,
                alt_unlock_submitted: false,
            },
            ForegroundAttempt {
                set_foreground_ok: true,
                target_is_foreground: true,
                alt_unlock_submitted: true,
            },
        ]
        .into_iter();

        let outcome = drive_foreground_activation(|use_alt_unlock| {
            unlock_flags.push(use_alt_unlock);
            attempts
                .next()
                .expect("unexpected extra activation attempt")
        });

        assert_eq!(unlock_flags, [false, true]);
        assert!(outcome.activated);
        assert_eq!(outcome.attempt_count, 2);
        assert!(outcome.alt_unlock_submitted);
    }

    #[test]
    fn foreground_activation_does_not_trust_api_return_without_readback() {
        let outcome = drive_foreground_activation(|use_alt_unlock| ForegroundAttempt {
            set_foreground_ok: true,
            target_is_foreground: false,
            alt_unlock_submitted: use_alt_unlock,
        });

        assert!(!outcome.activated);
        assert_eq!(outcome.attempt_count, 2);
        assert!(outcome.alt_unlock_submitted);
    }

    /// Windows 可能异步完成前台切换：单次读回失败不等于置前失败，必须有界复核；
    /// 但复核次数固定，读回始终失败时要尽早返回，不能把启动路径拖住。
    #[test]
    fn foreground_readback_rechecks_before_giving_up() {
        let mut reads = 0;
        let mut waits = 0;
        let activated = readback_with_recheck(
            || {
                reads += 1;
                reads >= 2
            },
            || waits += 1,
            2,
        );
        assert!(activated, "第二次读回成功必须被判为已置前");
        assert_eq!(reads, 2);
        assert_eq!(waits, 1, "首次失败后必须等待一次再读回");

        let mut waits = 0;
        let activated = readback_with_recheck(|| false, || waits += 1, 2);
        assert!(!activated);
        assert_eq!(waits, 2, "复核次数必须封顶");

        let mut waits = 0;
        assert!(readback_with_recheck(|| true, || waits += 1, 2));
        assert_eq!(waits, 0, "首次读回即成功时不得引入额外等待");
    }

    /// Windows 桌面实测（默认忽略）：先手动让记事本保持运行、再把其他应用切到
    /// 前台，随后运行本测试。成功必须来自 `GetForegroundWindow` 的目标进程读回，
    /// 不能只看 SetForegroundWindow/ShellExecute 返回值。
    #[test]
    #[cfg(windows)]
    #[ignore = "会真实把已运行的记事本切到前台"]
    fn running_notepad_reaches_observed_foreground() {
        assert!(
            matches!(
                activate_running(&["notepad.exe"]),
                RunningActivation::Activated(_)
            ),
            "请先启动记事本并让另一个应用处于前台"
        );
    }

    /// 子进程锁持有器。普通测试运行时立即返回；仅由下面的忽略探针通过环境变量
    /// 启动。独立进程是必要条件——同进程既持有前台又调用 SetForegroundWindow
    /// 会天然获准，无法覆盖用户报告的后台进程场景。
    #[test]
    #[cfg(windows)]
    fn foreground_lock_holder_process() {
        if std::env::var("SAYALL_FOREGROUND_LOCK_HOLDER").as_deref() != Ok("1") {
            return;
        }
        use std::io::{Read, Write};
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowW, GetForegroundWindow, LockSetForegroundWindow, MessageBoxW, PostMessageW,
            SetForegroundWindow, LSFW_LOCK, MB_OK, WM_CLOSE,
        };

        let title = format!("SayAll foreground lock probe {}", std::process::id());
        let thread_title = title.clone();
        let dialog = std::thread::spawn(move || {
            let title: Vec<u16> = thread_title.encode_utf16().chain(Some(0)).collect();
            let message: Vec<u16> = "SayAll foreground activation probe"
                .encode_utf16()
                .chain(Some(0))
                .collect();
            unsafe {
                let _ = MessageBoxW(
                    None,
                    PCWSTR(message.as_ptr()),
                    PCWSTR(title.as_ptr()),
                    MB_OK,
                );
            }
        });
        let wide_title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        let mut found = None;
        for _ in 0..100 {
            if let Ok(hwnd) = unsafe { FindWindowW(None, PCWSTR(wide_title.as_ptr())) } {
                if !hwnd.0.is_null() {
                    found = Some(hwnd);
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let hwnd = found.expect("测试消息框未创建");

        for _ in 0..10 {
            let _ = with_alt_foreground_unlock(|| unsafe { SetForegroundWindow(hwnd).as_bool() });
            if unsafe { GetForegroundWindow() } == hwnd {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(unsafe { GetForegroundWindow() }, hwnd);
        assert!(unsafe { LockSetForegroundWindow(LSFW_LOCK) }.is_ok());

        let port: u16 = std::env::var("SAYALL_FOREGROUND_LOCK_PORT")
            .expect("缺少锁探针端口")
            .parse()
            .expect("锁探针端口无效");
        let mut stream =
            std::net::TcpStream::connect(("127.0.0.1", port)).expect("无法连接锁探针父进程");
        stream.write_all(&[1]).expect("无法发送锁就绪信号");
        let mut stop = [0u8; 1];
        stream.read_exact(&mut stop).expect("无法读取锁停止信号");
        let _ = unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) };
        let _ = dialog.join();
    }

    #[cfg(windows)]
    pub(crate) fn with_foreground_lock<T>(operation: impl FnOnce() -> T) -> T {
        use std::io::{Read, Write};
        use std::process::{Command, Stdio};

        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("无法创建锁探针监听器");
        let port = listener.local_addr().expect("无法读取锁探针端口").port();
        let mut child = Command::new(std::env::current_exe().expect("无法读取测试程序路径"))
            .args([
                "--exact",
                "app_launcher::tests::foreground_lock_holder_process",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("SAYALL_FOREGROUND_LOCK_HOLDER", "1")
            .env("SAYALL_FOREGROUND_LOCK_PORT", port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .expect("无法启动锁探针子进程");

        listener
            .set_nonblocking(true)
            .expect("无法设置锁探针非阻塞监听");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut connected = None;
        while std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok(connection) => {
                    connected = Some(connection);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Some(status) = child.try_wait().expect("无法读取锁探针子进程状态")
                    {
                        panic!("锁探针子进程在连接前退出：{status}");
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(error) => panic!("锁探针监听失败：{error}"),
            }
        }
        let Some((mut stream, _)) = connected else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("锁探针子进程未在 3 秒内连接");
        };
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .expect("无法设置锁探针读取超时");
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(3)))
            .expect("无法设置锁探针写入超时");
        let mut ready = [0u8; 1];
        stream.read_exact(&mut ready).expect("无法读取锁就绪信号");
        let result = operation();
        stream.write_all(&[1]).expect("无法发送锁停止信号");
        let child_status = child.wait().expect("无法等待锁探针子进程");

        assert!(child_status.success(), "锁探针子进程失败");
        result
    }

    /// 强制 foreground lock 的 Windows 探针：独立子进程持有前台锁，父测试进程
    /// 从后台调用产品路径。必须观察到第一次被拒绝、成对 Alt 解锁、第二次读回成功。
    #[test]
    #[cfg(windows)]
    #[ignore = "会显示短暂测试消息框并把已运行的记事本切到前台"]
    fn foreground_lock_retry_reaches_observed_notepad() {
        let activation = with_foreground_lock(|| activate_running(&["notepad.exe"]));
        let RunningActivation::Activated(outcome) = activation else {
            panic!("foreground lock 后的 Alt 解锁重试未激活记事本：{activation:?}");
        };
        assert_eq!(outcome.attempt_count, 2, "必须实际走到第二次尝试");
        assert!(outcome.alt_unlock_submitted, "第二次尝试必须成对提交 Alt");
    }

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

    /// 2026-10-02 扩充的预设应用（Vokie / Visual Studio Code / Cursor / DimAgent /
    /// QQ / 飞书 / Hermes）：必须带开始菜单快捷方式探测名——这些应用常装在
    /// 自定义目录（如 D:\Apps），App Paths 与 System32 探测覆盖不到。
    #[test]
    fn preset_table_includes_requested_apps() {
        for (id, name) in [
            ("vokie", "Vokie"),
            ("vscode", "Visual Studio Code"),
            ("cursor", "Cursor"),
            ("dimagent", "DimAgent"),
            ("qq", "QQ"),
            ("feishu", "飞书"),
            ("hermes", "Hermes"),
        ] {
            let app = preset_app(id).unwrap_or_else(|| panic!("预设表缺少 {id}"));
            assert_eq!(app.name, name, "{id} 的展示名");
            assert!(
                !app.shortcut_names.is_empty(),
                "{id} 需要开始菜单快捷方式兜底探测"
            );
        }
    }

    /// 微信常装在自定义目录（本机 D:\Apps\Weixin）：必须带开始菜单兜底，
    /// 否则已安装的微信不会出现在"打开应用"列表（2026-10-02 用户反馈）。
    #[test]
    fn wechat_preset_falls_back_to_start_menu_shortcut() {
        let app = preset_app("wechat").expect("微信预设");
        assert!(
            app.shortcut_names.contains(&"微信"),
            "微信需要开始菜单快捷方式兜底探测"
        );
    }

    /// 安装候选路径模板展开：正常变量、整串变量与缺失变量。
    #[test]
    #[cfg(windows)]
    fn install_path_templates_expand_environment_variables() {
        let expanded = expand_install_path("%SystemRoot%\\System32\\notepad.exe")
            .expect("SystemRoot 应可展开");
        assert!(expanded.is_file(), "展开结果应指向真实文件：{expanded:?}");
        assert!(expand_install_path("%SystemRoot%").is_some());
        assert!(
            expand_install_path("%SAYALL_MISSING_VAR_PROBE%\\app.exe").is_none(),
            "变量缺失必须返回 None，不能留下字面量路径"
        );
    }

    /// 开始菜单索引：识别 .lnk（大小写不敏感）、递归子目录、忽略其他文件。
    #[test]
    #[cfg(windows)]
    fn start_menu_index_collects_lnk_names() {
        let root = std::env::temp_dir().join("sayall-start-menu-index-probe");
        let _ = std::fs::remove_dir_all(&root);
        let nested = root.join("Tools");
        std::fs::create_dir_all(&nested).expect("创建探测目录");
        std::fs::write(root.join("readme.txt"), b"x").expect("写入无关文件");
        std::fs::write(root.join("Hermes.LNK"), b"x").expect("写入快捷方式");
        std::fs::write(nested.join("Visual Studio Code.lnk"), b"x").expect("写入嵌套快捷方式");
        let mut out = Vec::new();
        collect_start_menu_shortcuts(&root, &mut out);
        let stems: Vec<&str> = out.iter().map(|(stem, _)| stem.as_str()).collect();
        assert!(stems.contains(&"hermes"), "扩展名大小写不敏感：{stems:?}");
        assert!(
            stems.contains(&"visual studio code"),
            "应递归子目录：{stems:?}"
        );
        assert_eq!(out.len(), 2, "非 .lnk 文件不应入索引");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 候选路径存在时启动目标直接取完整路径（自定义安装目录的关键路径）。
    #[test]
    #[cfg(windows)]
    fn preset_launch_target_prefers_existing_install_path() {
        let probe = std::env::temp_dir().join("sayall-preset-install-path-probe.exe");
        std::fs::write(&probe, b"x").expect("创建探测文件");
        let app = PresetApp {
            id: "probe",
            name: "probe",
            exe_names: &["probe.exe"],
            install_paths: &["%TEMP%\\sayall-preset-install-path-probe.exe"],
            shortcut_names: &[],
        };
        let target = preset_launch_target(&app, &[]).expect("候选路径应命中");
        assert_eq!(target.source, "install_path");
        assert!(target
            .exe_path
            .to_lowercase()
            .ends_with("sayall-preset-install-path-probe.exe"));
        let _ = std::fs::remove_file(&probe);
    }

    /// 开始菜单快捷方式：名称命中且目标 exe 名匹配才可用；同名快捷方式
    /// 指向别的程序时必须拒绝，避免把未安装误报成已安装。
    #[test]
    #[cfg(windows)]
    fn preset_launch_target_resolves_matching_shortcut_only() {
        let dir = std::env::temp_dir().join("sayall-preset-shortcut-probe");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("创建探测目录");
        let target_exe = dir.join("Hermes.exe");
        std::fs::write(&target_exe, b"x").expect("创建目标 exe 占位文件");
        let lnk = dir.join("Hermes.lnk");
        assert!(
            create_test_shortcut(&lnk, &target_exe.to_string_lossy(), "", ""),
            "创建探测快捷方式失败"
        );

        let app = PresetApp {
            id: "shortcut-probe",
            name: "shortcut-probe",
            exe_names: &["Hermes.exe"],
            install_paths: &[],
            shortcut_names: &["Hermes"],
        };
        let index = vec![("hermes".to_owned(), lnk.clone())];
        let target = preset_launch_target(&app, &index).expect("快捷方式应命中");
        assert_eq!(target.source, "start_menu_shortcut");
        assert!(target.exe_path.to_lowercase().ends_with("hermes.exe"));

        // 同名快捷方式指向不匹配的 exe：拒绝。
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).expect("创建子目录");
        let other_exe = sub.join("Other.exe");
        std::fs::write(&other_exe, b"x").expect("创建无关 exe 占位文件");
        let other_lnk = sub.join("Hermes.lnk");
        assert!(create_test_shortcut(
            &other_lnk,
            &other_exe.to_string_lossy(),
            "",
            ""
        ));
        let index = vec![("hermes".to_owned(), other_lnk)];
        assert!(
            preset_launch_target(&app, &index).is_none(),
            "目标 exe 与 exe_names 不匹配必须拒绝"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 探测结果一致性：probe 判定已安装的预设（自身除外）必须能给出
    /// System32/App Paths 或候选路径/开始菜单启动位置——否则 UI 会展示
    /// 一个点了打不开的芯片。
    #[test]
    #[cfg(windows)]
    fn installed_preset_apps_expose_launch_targets() {
        let shortcuts = start_menu_shortcuts();
        for info in probe_preset_apps() {
            if !info.installed || info.id == "sayall" {
                continue;
            }
            let app = preset_app(&info.id).expect("probe 只返回预设表内的条目");
            assert!(
                app.exe_names.iter().any(|exe| exe_resolvable(exe))
                    || preset_launch_target(app, &shortcuts).is_some(),
                "{} 判定已安装却没有可用的启动位置",
                info.id
            );
        }
    }

    /// 在测试里创建指向指定 exe 的 .lnk（真机 COM 链路，与
    /// `lnk_resolution_round_trips` 同一套 IShellLinkW + IPersistFile）。
    #[cfg(windows)]
    fn create_test_shortcut(
        lnk: &std::path::Path,
        target: &str,
        arguments: &str,
        working_dir: &str,
    ) -> bool {
        let lnk = lnk.to_path_buf();
        let target = target.to_owned();
        let arguments = arguments.to_owned();
        let working_dir = working_dir.to_owned();
        std::thread::Builder::new()
            .name("sayall-test-lnk-create".to_owned())
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
                    let wide =
                        |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
                    let target_wide = wide(&target);
                    link.SetPath(PCWSTR(target_wide.as_ptr())).ok()?;
                    let args_wide = wide(&arguments);
                    link.SetArguments(PCWSTR(args_wide.as_ptr())).ok()?;
                    let dir_wide = wide(&working_dir);
                    link.SetWorkingDirectory(PCWSTR(dir_wide.as_ptr())).ok()?;
                    let persist: IPersistFile = link.cast().ok()?;
                    let lnk_wide = wide(&lnk.to_string_lossy());
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
            .expect("create thread panicked")
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

    #[test]
    fn launcher_family_rank_prefers_exe_name_matching_aumid() {
        let resolved = r"C:\Program Files (x86)\WPS Office\ksolaunch.exe";
        let aumid = "Kingsoft.Office.WPS";

        // 真正的文档进程：同安装目录的版本子目录里，基名 wps 出现在 AUMID 中 → 优先
        assert_eq!(
            launcher_family_rank(
                r"C:\Program Files (x86)\WPS Office\12.1.0.25225\office6\wps.exe",
                resolved,
                aumid
            ),
            Some(1)
        );
        // 同族的表格进程：同目录之下，但基名与 AUMID 不符 → 可用但不优先
        assert_eq!(
            launcher_family_rank(
                r"C:\Program Files (x86)\WPS Office\12.1.0.25225\office6\et.exe",
                resolved,
                aumid
            ),
            Some(0)
        );
        // 表格 AUMID 下，et.exe 反过来是优先候选
        assert_eq!(
            launcher_family_rank(
                r"C:\Program Files (x86)\WPS Office\12.1.0.25225\office6\et.exe",
                resolved,
                "Kingsoft.Office.ET"
            ),
            Some(1)
        );
        // 不在同一安装目录 → 不相干
        assert_eq!(
            launcher_family_rank(r"C:\Other\app.exe", resolved, aumid),
            None
        );
        // 解析路径本身由精确匹配负责，不在这里重复判定
        assert_eq!(launcher_family_rank(resolved, resolved, aumid), None);
    }

    #[test]
    fn launcher_family_rank_normalizes_case_separators_and_prefix() {
        let resolved = r"C:\Program Files (x86)\WPS Office\ksolaunch.exe";
        assert_eq!(
            launcher_family_rank(
                r"\\?\c:/program files (x86)/wps office/12.1.0.25225/office6/WPS.EXE",
                resolved,
                "Kingsoft.Office.WPS"
            ),
            Some(1)
        );
        // 只差一个字符的相邻目录不能算同族（避免误命中同级产品）
        assert_eq!(
            launcher_family_rank(
                r"C:\Program Files (x86)\WPS Office Backup\wps.exe",
                resolved,
                "Kingsoft.Office.WPS"
            ),
            None
        );
    }
}
