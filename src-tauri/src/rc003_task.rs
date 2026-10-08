//! RC003 三键助手：计划任务的检测 / 授权 / 触发 / 停止。
//!
//! 产品流程（2026-10-03 Andy 定稿：**每次开启都重新弹窗 + 重新授权**）：
//! * 打开：每次都重走一次 UAC、重新注册任务（用户点了「是」才算重新授权），
//!   然后触发助手；
//! * 关闭：结束当前助手进程；**任务保留在系统里**——提权进程创建的任务普通
//!   权限删不掉（真机实测），"取消授权"只能以「下次开启强制重装任务」落地，
//!   所以关闭后授权即视为作废，下次开启必然再弹一次 UAC；
//! * 主程序每次启动：若开关仍开着（设置里 enabled），自动触发一次 —— 这是
//!   "开着开关的用户重启应用后无需再授权即恢复"的既有语义，不在"每次打开"
//!   的范围里（"打开"指用户拨动开关这个动作）。
//!
//! 任务名必须与助手侧一致：`hardware/RC003/helper/src/main.rs` 的
//! `SCHEDULED_TASK_NAME`。两侧各有一份常量，靠 `--selftest` 与本文件的
//! 测试分别钉住字面值；改任何一边都要同步另一边。

/// 与助手侧 `SCHEDULED_TASK_NAME` 必须逐字符一致。
pub const SCHEDULED_TASK_NAME: &str = "SayAll RC003 Helper";

/// 开关状态。`installed` = 任务在系统里（**不等于**授权有效：2026-10-03 起
/// 每次开启都重新授权，任务在不在都不影响下次开启必弹 UAC）；
/// `enabled` = 用户意图（持久化在设置里，默认关闭）。
/// 两者是**不同的状态**：关闭开关只结束助手、任务保留，
/// 所以开关显示读 `enabled`，绝不能读 `installed`。
///
/// `authorization_required` = **这次打开开关会触发系统授权（UAC）**。
/// 2026-10-03 起恒为 true（见 [`AUTHORIZATION_REQUIRED_ON_EVERY_ENABLE`]）：
/// 每次开启都重新授权。字段保留给诊断与 IPC 兼容，前端弹窗判据已不依赖它
/// （每次开启都弹确认弹窗，只有关闭方向直接执行）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub installed: bool,
    pub authorization_required: bool,
    pub enabled: bool,
    pub helper_path: Option<String>,
    pub last_error: Option<String>,
}

/// 「这次打开开关会触发系统授权（UAC）」的唯一判据。
///
/// 2026-10-03 Andy 定稿：**每次开启都重新弹窗 + 重新授权**——开关从关到开的
/// 每一次都要重走一次 UAC，所以这里恒为 true（此前是「任务未注册或重授权
/// 标记在」才要授权）。`status()` 与 `enable_capture` 共用本判据，防止两处漂移；
/// 改回旧语义前，必须同步前端弹窗文案（EnhancedCaptureConfirmDialog）与
/// ButtonsPage / ConnectionPage 的「每次开启都先弹确认」测试。
const AUTHORIZATION_REQUIRED_ON_EVERY_ENABLE: bool = true;

/// 定位助手 exe。按序尝试：
/// 1. 环境变量覆盖（验收 / 非标准安装位置）；
/// 2. 与主程序同目录（产品化布局：安装器把两者放在一起）——文件名按**本机原生架构**取
///    （x64 = `sayall-helper.exe`，arm64 = `sayall-helper-arm64.exe`，见 ADR 0003）；
/// 3. 开发布局：`target/debug/sayall-windows-app.exe` 向上找到仓库根，再进
///    `hardware/RC003/helper/target/[<triple>/]release/sayall-helper.exe`
///    （cargo 产物名恒为包名，arm64 只多一层 triple 目录）。
///
/// 架构由 [`sayall_windows::os_arch::native_arch`] 判定，**不**用编译期常量或
/// `PROCESSOR_ARCHITECTURE`——x64 主程序在 ARM64 上运行时那两者都报 x64（Issue #206）。
pub fn locate_helper_exe() -> Option<std::path::PathBuf> {
    locate_helper_exe_for(sayall_windows::os_arch::native_arch())
}

pub fn locate_helper_exe_for(
    arch: sayall_windows::os_arch::NativeArch,
) -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("SAYALL_RC003_HELPER") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    if let Some(dir) = exe.parent() {
        let candidate = dir.join(arch.helper_file_name());
        if candidate.is_file() {
            return Some(candidate);
        }
        // 开发布局：沿着父目录向上找仓库根。
        let mut dir = dir.to_path_buf();
        for _ in 0..4 {
            dir = dir.parent()?.to_path_buf();
            let mut candidate = dir
                .join("hardware")
                .join("RC003")
                .join("helper")
                .join("target");
            if let Some(triple) = dev_target_triple(arch) {
                candidate = candidate.join(triple);
            }
            let candidate = candidate.join("release").join("sayall-helper.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 该架构的 cargo `--target` 目录名。x64 用默认 target 目录（无 triple），
/// arm64 必须显式 `--target aarch64-pc-windows-msvc`（交叉编译），因此多一层。
fn dev_target_triple(arch: sayall_windows::os_arch::NativeArch) -> Option<&'static str> {
    match arch {
        sayall_windows::os_arch::NativeArch::Arm64 => Some("aarch64-pc-windows-msvc"),
        _ => None,
    }
}

/// 增强捕获在本机的可用性（IPC `get_capture_support` 的返回体）。
///
/// 为什么要有它：ARM64 上"打开开关 → 永远停在正在启动"曾经没有任何解释（Issue #206），
/// 用户与排查者都只能看到 25 秒后的一条失败日志。这里把结论前移：本机架构有没有对应的
/// 助手 / Gadget，界面据此置灰并说明原因。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSupport {
    /// 本机**原生**架构：`x64` / `arm64` / `unknown`（探测失败或不受支持的原生架构）。
    pub native_arch: String,
    /// 本架构应当存在的助手文件名（诊断用；界面不显示）。
    pub helper_expected: String,
    pub available: bool,
    /// `arch_unsupported` = 本机原生架构不属于 x64 / arm64；
    /// `helper_missing` = 架构对得上，但安装包里没有对应的助手（载荷缺失）。
    pub reason: Option<String>,
}

/// 可用性判定（纯函数，便于单测覆盖两种不可用原因）。
pub(crate) fn capture_support_decision(
    arch: sayall_windows::os_arch::NativeArch,
    helper_found: bool,
) -> (bool, Option<&'static str>) {
    if arch.is_explicitly_unsupported() {
        return (false, Some("arch_unsupported"));
    }
    if !helper_found {
        return (false, Some("helper_missing"));
    }
    (true, None)
}

/// 本机增强捕获的可用性。探测失败（`unknown`）时沿用 x64 路径与既有行为：
/// 一次探测失败不足以把功能判死。
pub fn capture_support() -> CaptureSupport {
    let arch = sayall_windows::os_arch::native_arch();
    let helper_found = locate_helper_exe_for(arch).is_some();
    let (available, reason) = capture_support_decision(arch, helper_found);
    CaptureSupport {
        native_arch: arch.as_str().to_string(),
        helper_expected: arch.helper_file_name().to_string(),
        available,
        reason: reason.map(|value| value.to_string()),
    }
}

/// 任务是否已注册（= 用户已授权）。只看退出码，不解析 GBK 输出。
pub fn task_installed() -> bool {
    schtasks(&["/query", "/tn", SCHEDULED_TASK_NAME])
        .map(|ok| ok)
        .unwrap_or(false)
}

/// 触发助手（免提权）。助手带 `--follow-app`，会在主程序关闭后自行退出。
pub fn task_trigger() -> Result<(), String> {
    run_schtasks(&["/run", "/tn", SCHEDULED_TASK_NAME], "触发")
}

/// 结束助手（免提权尝试；被拒时由调用方走提权路径）。
/// **不移除任务**——授权保留，这是"只弹一次 UAC"的一部分。
pub fn task_stop() -> Result<(), String> {
    run_schtasks(&["/end", "/tn", SCHEDULED_TASK_NAME], "停止")
}

/// 注册任务：弹**一次** UAC（PowerShell `Start-Process -Verb RunAs` 等到装完），
/// GUI 子系统的主程序没有自己的控制台，而 `schtasks` / `powershell` 都是
/// 控制台程序——不加这个标志，每次调用都会分配一个新控制台窗口并立刻
/// 消失（按键页每秒轮询任务状态 = **每秒闪一次黑窗**，2026-09-24 真机复现）。
/// dev 模式看不出来：那时主程序带着父控制台，子进程直接继承。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn silent_command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

/// 弹一次 UAC 完成任务注册：**直接 ShellExecuteExW(runas) 助手本体**，
/// 不再经由 PowerShell。
///
/// 为什么必须换掉 PowerShell（2026-09-24 两轮真机证伪）：PowerShell 5.1
/// 对「UAC 被取消」的报错是**非终止性错误**，`-Command` 退出码 0——
/// 连 `-ErrorAction Stop` 都可能不产生预期效果（两次 passed 且任务
/// Date 未变）。「用户拒绝」与「用户允许」从此无法区分。
/// ShellExecuteExW 则给出操作系统级的确定信号：
///   用户点否 / 叉掉 UAC 窗口 → 返回 FALSE + GetLastError = ERROR_CANCELLED
///   用户允许 → hProcess 有效，等助手退出码（0 = 安装成功）
pub fn task_install_elevated(helper: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_CANCELLED, HANDLE};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };

    let file: Vec<u16> = std::ffi::OsStr::new(helper)
        .encode_wide()
        .chain(Some(0))
        .collect();
    // lpParameters 是单个字符串；助手自 2026-10-04 起是 **GUI 子系统**程序：
    // 提权 / 计划任务拉起时不创建任何控制台窗口。`--hide-window` 保留为兼容
    // 参数（旧版助手用它隐藏自己的黑框；新版接受但无窗口可隐藏）。
    let parameters: Vec<u16> = "--install-task --hide-window"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let verb: Vec<u16> = "runas\0".encode_utf16().collect();

    let mut sei = SHELLEXECUTEINFOW::default();
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    // NOCLOSEPROCESS：要 hProcess 等助手退出码；FLAG_NO_UI：失败不要
    // Shell 自己弹 UI，错误归我们表达。
    sei.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI;
    sei.lpVerb = PCWSTR(verb.as_ptr());
    sei.lpFile = PCWSTR(file.as_ptr());
    sei.lpParameters = PCWSTR(parameters.as_ptr());
    // SW_HIDE 双保险：新版助手本就没有窗口；万一拉起的是尚未升级的旧版
    // 控制台助手，也让它隐藏启动，而不是闪一下黑框。
    sei.nShow = 0;

    if let Err(error) = unsafe { ShellExecuteExW(&mut sei) } {
        // 用户点「否」或叉掉 UAC 窗口都走这里：ERROR_CANCELLED (1223)。
        let cancelled = error.code() == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0);
        sayall_windows::gatt_note(format!(
            "rc003 feature=enhanced-capture action=elevated_install outcome={} hresult={:?}",
            if cancelled {
                "cancelled_by_user"
            } else {
                "shell_execute_failed"
            },
            error.code()
        ));
        return Err(if cancelled {
            "授权未完成（UAC 被取消）。三键捕获保持关闭，可再次打开重试。".to_string()
        } else {
            format!("提权安装失败：{error}")
        });
    }
    let process = sei.hProcess;
    if process.is_invalid() || process == HANDLE::default() {
        return Err("提权流程没有返回助手进程句柄（安装未执行）".to_string());
    }
    unsafe {
        let _ = WaitForSingleObject(process, INFINITE);
    }
    let mut exit_code = 0u32;
    let _ = unsafe { GetExitCodeProcess(process, &mut exit_code) };
    sayall_windows::gatt_note(format!(
        "rc003 feature=enhanced-capture action=elevated_install outcome=helper_exit exit_code={exit_code}"
    ));
    if exit_code != 0 {
        return Err(format!(
            "授权未完成（助手注册计划任务失败，退出码 {exit_code}）。可再次打开重试。"
        ));
    }
    Ok(())
}

/// 开关打开：**每次都重新授权**（弹一次 UAC 重装任务），然后触发。
/// 失败原样返回（调用方保持开关原状态）。
pub fn enable_capture() -> Result<(), String> {
    // 本机没有对应架构的载荷时直接拒绝（界面已按 `get_capture_support` 置灰，这里是兜底）：
    // 放它走下去只会白弹一次系统授权窗口，然后必然连不上（Issue #206）。
    let support = capture_support();
    if !support.available {
        sayall_windows::gatt_note(format!(
            "rc003 feature=enhanced-capture action=enable phase=completed terminal_result=failed \
             reason={} native_arch={} helper_expected={} retryable=false",
            support.reason.as_deref().unwrap_or("unavailable"),
            support.native_arch,
            support.helper_expected
        ));
        return Err(match support.reason.as_deref() {
            Some("arch_unsupported") => "这台电脑暂不支持全按键支持。".to_string(),
            _ => "安装包缺少本机需要的组件，全按键支持暂不可用；请重新安装本版本。".to_string(),
        });
    }
    // 清掉可能残留的停用信号：否则"关开关（没助手在跑）→ 立刻再开"时，
    // 新助手一启动就见到信号当场退出，开关开着却永远连不上。
    let _ = std::fs::remove_file(stop_signal_path());
    let helper = locate_helper_exe().ok_or_else(|| {
        format!(
            "找不到 {}（检查安装布局，或设置 SAYALL_RC003_HELPER 指向它）",
            support.helper_expected
        )
    })?;
    // 2026-10-03 Andy 定稿：每次开启都重新授权——无条件重装任务，于是每次
    // 开启都会走一次 UAC（用户点了「是」才算这次授权）。为什么不是"关闭时
    // 删除任务"：提权进程创建的任务普通权限删不掉（真机实测），撤销只能以
    // "下次开启强制重装"落地。判据与 `status()` 同源（同一个常量）。
    if AUTHORIZATION_REQUIRED_ON_EVERY_ENABLE {
        // 重授权成功的判据**不能是退出码**：PowerShell 5.1 对 UAC 取消的
        // 非终止性错误即使加了 -ErrorAction Stop 也可能退出 0（真机实测
        // 两次 passed 且任务 Date 未变）。硬判据是「任务的注册时间真的
        // 变了」——/create /f 会重写任务 XML 的 <Date>。
        let before = task_registered_at();
        task_install_elevated(&helper)?;
        let after = task_registered_at();
        let verified = match (before.as_deref(), after.as_deref()) {
            (_, None) => true,       // 基准/事后都读不到任务文件（罕见 ACL）：退回信任退出码
            (None, Some(_)) => true, // 之前无任务、之后有了 = 新建成功
            (Some(b), Some(a)) => b != a,
            (None, None) => true,
        };
        if !verified {
            return Err(
                "授权未完成（UAC 未被确认，任务没有重新注册）。三键捕获保持关闭，可再次打开重试。"
                    .to_string(),
            );
        }
    }
    let result = task_trigger();
    if result.is_ok() {
        clear_reauth_marker();
    }
    result
}

/// 开关关闭：结束助手，**不移除任务**（提权任务普通权限删不掉；授权视为作废，
/// 下次开启必弹 UAC——见 [`AUTHORIZATION_REQUIRED_ON_EVERY_ENABLE`]）。
///
/// 主程序是普通权限，**杀不掉提权助手**；`schtasks /end` 只能停"当前任务
/// 实例"——若实例已换（任务重装、改名后旧进程挂着），它完全落空，
/// 旧助手就继续映射（2026-09-24 真机复现）。所以真正的停止通道是**文件
/// 信号**：写一个助手可见的标记，助手轮询到就自行退出（它有权限删自己）。
/// task_stop 保留为兜底（对恰好还是当前实例的任务有效）。
pub fn disable_capture() -> Result<(), String> {
    if let Some(parent) = stop_signal_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(stop_signal_path(), b"stop")
        .map_err(|error| format!("写停用信号失败：{error}"))?;
    task_stop()
}

/// 停用信号文件：`%LOCALAPPDATA%\SayAll\rc003-capture-stop`。
/// 与助手侧 `app_stop_signal_path()` 的路径约定**必须逐字符一致**
/// （两侧各有一份常量，靠测试钉住——见 rc003_task 测试与助手自检）。
fn stop_signal_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
        format!("{home}\\AppData\\Local")
    });
    std::path::PathBuf::from(base)
        .join("SayAll")
        .join("rc003-capture-stop")
}

/// 重授权标记：`%LOCALAPPDATA%\SayAll\rc003-reauth-required`。
///
/// **为什么需要**：授权（计划任务）由提权进程创建，普通权限的卸载器
/// **删不掉它**（真机实测：schtasks /delete 静默失败）——卸载时
/// 只能写这个标记作为「授权已应撤销」的凭证。应用启动见到标记就把
/// 开关回落为关闭；用户重新打开时 enable 强制重装任务（必弹 UAC），
/// 成功后清除标记。
///
/// 内容与写入时机（2026-09-28 修订）：**只有真卸载**（NSIS 把卸载器
/// 拷进临时目录跑，`$EXEDIR != $INSTDIR`）才写，内容为
/// `uninstalled=<卸载时刻 GetTickCount>`；升级安装原位调用旧卸载器
/// 不写。带 `uninstalled=` 前缀的新格式**任何安装都不删**（重装凭证）；
/// 旧格式（裸 tick / 字面 `reauth`）是历史版本卸载器产物，安装器侧
/// 只做过渡清理。与安装器侧写入的路径**必须逐字符一致**。
pub fn reauth_marker_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
        format!("{home}\\AppData\\Local")
    });
    std::path::PathBuf::from(base)
        .join("SayAll")
        .join("rc003-reauth-required")
}

/// 是否处于「需重新授权」状态（安装/升级后的首次开启前）。
pub fn reauth_required() -> bool {
    reauth_marker_path().exists()
}

fn clear_reauth_marker() {
    let _ = std::fs::remove_file(reauth_marker_path());
}

/// 计划任务 XML 原文（UTF-16LE，带 BOM）。读不到就返回 `None`：任务缺失、被删除
/// 或当前用户没有读权限都走这一条。
fn task_xml() -> Option<String> {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let bytes = std::fs::read(
        std::path::Path::new(&root)
            .join("System32")
            .join("Tasks")
            .join(SCHEDULED_TASK_NAME),
    )
    .ok()?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// 任务的注册时间（任务 XML 的 `<Date>`），作为「任务真的被重装过」的硬判据。
fn task_registered_at() -> Option<String> {
    extract_task_date(&task_xml()?)
}

fn extract_task_date(xml: &str) -> Option<String> {
    let start = xml.find("<Date>")? + "<Date>".len();
    let end = xml[start..].find("</Date>")? + start;
    Some(xml[start..end].to_string())
}

/// 任务注册的可执行文件名（`<Exec><Command>` 的最后一段路径）。
/// **只取文件名**：完整路径是个人路径，不进日志。
fn extract_task_command(xml: &str) -> Option<String> {
    let start = xml.find("<Command>")? + "<Command>".len();
    let end = xml[start..].find("</Command>")? + start;
    let raw = xml[start..end].trim().trim_matches('"');
    let name = raw.rsplit(|c| c == '\\' || c == '/').next()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// 计划任务指向的助手是不是**本架构**那一份（纯函数，便于单测）。
/// `None` = 拿不到命令：那属于"任务缺失"另一条判据，这里不表态。
pub(crate) fn task_target_verdict(command: Option<&str>, expected: &str) -> Option<bool> {
    Some(command?.trim().eq_ignore_ascii_case(expected))
}

/// 计划任务当前指向的助手是否与本机架构相符。
///
/// **为什么需要**（2026-10-07 报障人 ARM64 实测发现）：覆盖安装会保留旧版注册的计划任务，
/// 任务目标写的是**当时那个助手**的路径。从"只有 x64"的版本升到带 arm64 载荷的版本后，
/// ARM64 机器上的任务仍指向 `sayall-helper.exe`：启动自动拉起会去跑 x64 助手，架构闸门
/// 会拦下它（文案清楚），但界面要等约 25 秒重试完才失败，用户会以为功能坏了。
/// 所以启动对账先比一次：不一致就回落开关，等用户拨一次开关重建任务——重装任务要提权，
/// 不在启动时擅自弹 UAC（与"每次开启都重新授权"的既有语义一致）。
pub fn task_target_matches(helper_file_name: &str) -> Option<bool> {
    // 任务不存在时 `task_installed()` 那条判据已经会处理，这里不重复表态。
    if !task_installed() {
        return None;
    }
    let xml = task_xml()?;
    task_target_verdict(extract_task_command(&xml).as_deref(), helper_file_name)
}

/// `enabled` 来自持久化的用户意图（AppSettings.rc003_capture_enabled），
/// **不是**"任务是否存在"——两者是不同的状态（见 TaskStatus 注释）。
pub fn status(enabled: bool) -> TaskStatus {
    let installed = task_installed();
    TaskStatus {
        installed,
        // 每次开启都会触发系统授权（UAC）——2026-10-03 起恒为 true，
        // 与 enable_capture 的强制重装同源（同一个常量）。
        authorization_required: AUTHORIZATION_REQUIRED_ON_EVERY_ENABLE,
        enabled,
        helper_path: locate_helper_exe().map(|p| p.display().to_string()),
        last_error: None,
    }
}

fn run_schtasks(args: &[&str], label: &str) -> Result<(), String> {
    let output = silent_command("schtasks")
        .args(args)
        .output()
        .map_err(|error| format!("无法执行 schtasks（{label}）：{error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Err(format!(
        "schtasks {label}失败：{}",
        if stderr.is_empty() { stdout } else { stderr }
    ))
}

/// `schtasks /query` 的成功/失败就是"存在/不存在"，不需要碰输出编码。
fn schtasks(args: &[&str]) -> Option<bool> {
    silent_command("schtasks")
        .args(args)
        .output()
        .ok()
        .map(|output| output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_name_matches_helper_side() {
        // 与助手侧的常量逐字符一致。助手 --selftest 钉住它那一边，
        // 这里钉住这一边；两边任何一个改名而另一个没跟上，这条就会红。
        assert_eq!(SCHEDULED_TASK_NAME, "SayAll RC003 Helper");
    }

    #[test]
    fn trigger_args_use_task_name_verbatim() {
        // schtasks 的 /tn 值含空格，靠 Command 参数数组传递（不拼字符串），
        // 因此这里断言"一个参数就是完整任务名"，防止有人改成拼接后再踩编码坑。
        let name = SCHEDULED_TASK_NAME;
        assert!(name.contains(' '));
        assert!(!name.starts_with('"') && !name.ends_with('"'));
    }

    #[test]
    fn stop_signal_path_matches_helper_side() {
        // 停用信号路径与助手侧 app_stop_signal_path() 逐字符一致。
        // 这条路径错一个字符，停用就静默失效（旧助手继续映射）——
        // 2026-09-24 的"开关关了还能映射"正是停止通道不可靠的结果。
        let path = stop_signal_path();
        assert!(path.ends_with(std::path::Path::new("SayAll").join("rc003-capture-stop")));
    }

    #[test]
    fn reauth_marker_path_matches_installer_side() {
        // 重授权标记路径与安装器钩子写入的路径逐字符一致
        // （installer-hooks.nsh：$LOCALAPPDATA\SayAll\rc003-reauth-required）。
        // 错一个字符，"升级后需重新授权"就静默失效。
        let path = reauth_marker_path();
        assert!(path.ends_with(std::path::Path::new("SayAll").join("rc003-reauth-required")));
    }

    #[test]
    fn extracts_task_registration_date() {
        // 重授权成功判据 = 任务 XML 的 <Date> 变化。解析错了，
        // "UAC 取消被误判成功"那一整类问题就会回来。
        let xml = "<?xml version=\"1.0\"?><Task><RegistrationInfo><Date>2026-09-24T20:27:20</Date>\
                   <Author>SAYALL\\hd838</Author></RegistrationInfo></Task>";
        assert_eq!(
            extract_task_date(xml),
            Some("2026-09-24T20:27:20".to_string())
        );
        assert_eq!(extract_task_date("<Task></Task>"), None);
    }

    #[test]
    fn extracts_task_command_file_name_without_paths() {
        // 真实任务 XML 的形态：<Command> 是完整路径（可能带引号），参数在 <Arguments> 里。
        // 这里只允许取文件名——完整路径是个人路径，不能进日志。
        let xml = r#"<?xml version="1.0"?><Task><Actions><Exec>
            <Command>"C:\Users\someone\AppData\Local\无线麦 SayAll\sayall-helper.exe"</Command>
            <Arguments>--follow-app</Arguments></Exec></Actions></Task>"#;
        assert_eq!(
            extract_task_command(xml).as_deref(),
            Some("sayall-helper.exe")
        );
        // 不带引号 / 正斜杠 / 尾随空白都要能吃下
        assert_eq!(
            extract_task_command("<Command>C:/x/sayall-helper-arm64.exe </Command>").as_deref(),
            Some("sayall-helper-arm64.exe")
        );
        // 没有 <Command> 或值为空 → None（交给"任务缺失"那条判据）
        assert_eq!(extract_task_command("<Task></Task>"), None);
        assert_eq!(extract_task_command("<Command>   </Command>"), None);
    }

    #[test]
    fn task_target_verdict_flags_the_other_arch_helper() {
        // 2026-10-07 报障人现场：ARM64 机器上任务仍指向 x64 助手（覆盖安装保留了旧任务）
        assert_eq!(
            task_target_verdict(Some("sayall-helper.exe"), "sayall-helper-arm64.exe"),
            Some(false)
        );
        assert_eq!(
            task_target_verdict(Some("sayall-helper-arm64.exe"), "sayall-helper-arm64.exe"),
            Some(true)
        );
        // Windows 路径大小写不敏感
        assert_eq!(
            task_target_verdict(Some("SAYALL-HELPER-ARM64.EXE"), "sayall-helper-arm64.exe"),
            Some(true)
        );
        // 拿不到命令 → 不表态
        assert_eq!(task_target_verdict(None, "sayall-helper.exe"), None);
    }

    #[test]
    fn every_enable_requires_reauthorization() {
        // 2026-10-03 Andy 定稿：每次打开开关都要重新弹窗 + 重新授权（每次都会
        // 弹 Windows 授权窗口）。这条一旦改回「任务在就不授权」，必须同步：
        // ① 前端弹窗（EnhancedCaptureConfirmDialog）文案与
        // ② ButtonsPage / ConnectionPage 的「每次开启都先弹确认」测试。
        assert!(AUTHORIZATION_REQUIRED_ON_EVERY_ENABLE);
    }

    #[test]
    fn capture_support_reasons_are_stable_strings() {
        use sayall_windows::os_arch::NativeArch;
        // 不受支持的原生架构（如 32 位 ARM）：无论助手在不在，都判不可用且原因固定
        assert_eq!(
            capture_support_decision(NativeArch::Other, true),
            (false, Some("arch_unsupported"))
        );
        assert_eq!(
            capture_support_decision(NativeArch::Other, false),
            (false, Some("arch_unsupported"))
        );
        // 架构对得上但载荷缺失：只有这一种情况算 helper_missing
        assert_eq!(
            capture_support_decision(NativeArch::X64, false),
            (false, Some("helper_missing"))
        );
        assert_eq!(
            capture_support_decision(NativeArch::Arm64, false),
            (false, Some("helper_missing"))
        );
        // 正常与"探测失败"都必须可用：探测失败不改既有行为
        assert_eq!(
            capture_support_decision(NativeArch::X64, true),
            (true, None)
        );
        assert_eq!(
            capture_support_decision(NativeArch::Arm64, true),
            (true, None)
        );
        assert_eq!(
            capture_support_decision(NativeArch::Unknown, true),
            (true, None)
        );
        // 这两条字符串是前端契约（`reason` 字段），改名即破坏界面判据
        assert_eq!(
            capture_support_decision(NativeArch::Other, false).1,
            Some("arch_unsupported")
        );
    }

    #[test]
    fn helper_file_name_follows_the_native_arch() {
        use sayall_windows::os_arch::NativeArch;
        // 安装目录里的文件名按架构分（ADR 0003）；开发布局里 cargo 产物名不变，
        // 只多一层 triple 目录。
        assert_eq!(NativeArch::X64.helper_file_name(), "sayall-helper.exe");
        assert_eq!(
            NativeArch::Arm64.helper_file_name(),
            "sayall-helper-arm64.exe"
        );
        assert_eq!(
            dev_target_triple(NativeArch::Arm64),
            Some("aarch64-pc-windows-msvc")
        );
        assert_eq!(dev_target_triple(NativeArch::X64), None);
        assert_eq!(dev_target_triple(NativeArch::Unknown), None);
    }

    #[test]
    fn capture_support_reports_a_consistent_shape() {
        // 本机是 x64 且仓库里就有助手产物（开发布局）——这依赖环境，故只断言形状不变量：
        // 字段命名是前端契约，available 与 reason 必须自洽。
        let support = capture_support();
        assert!(matches!(
            support.native_arch.as_str(),
            "x64" | "arm64" | "unknown"
        ));
        assert!(support.helper_expected.ends_with(".exe"));
        assert_eq!(support.available, support.reason.is_none());
    }
}
