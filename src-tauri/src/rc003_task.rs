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
    pub cleanup_pending: bool,
    pub can_retry_cleanup: bool,
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

pub fn locate_helper_exe() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("SAYALL_RC003_HELPER") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    if let Some(dir) = exe.parent() {
        let candidate = dir.join("sayall-helper.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
        let mut dir = dir.to_path_buf();
        for _ in 0..4 {
            dir = dir.parent()?.to_path_buf();
            let candidate = dir
                .join("hardware")
                .join("RC003")
                .join("helper")
                .join("target")
                .join("release")
                .join("sayall-helper.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn task_installed() -> bool {
    schtasks(&["/query", "/tn", SCHEDULED_TASK_NAME])
        .map(|ok| ok)
        .unwrap_or(false)
}

static CAPTURE_START_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
static CAPTURE_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SUBMITTED_START_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static CAPTURE_STOP_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn capture_epoch() -> u64 {
    CAPTURE_EPOCH.load(std::sync::atomic::Ordering::Acquire)
}

pub fn complete_capture_enable(
    epoch: u64,
    apply: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let _gate = CAPTURE_START_GATE.lock().unwrap_or_else(|p| p.into_inner());
    if capture_epoch() != epoch {
        return Err("全按键支持的启动请求已取消。".to_owned());
    }
    apply()
}

pub fn task_trigger(expected_epoch: u64) -> Result<(), String> {
    let _gate = CAPTURE_START_GATE.lock().unwrap_or_else(|p| p.into_inner());
    if capture_epoch() != expected_epoch {
        return Err("全按键支持的启动请求已取消。".to_owned());
    }
    match std::fs::remove_file(stop_signal_path()) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("无法清除上次停止请求，未启动按键助手。".to_owned()),
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "无法记录按键启动代次。".to_owned())?
        .as_millis() as u64;
    run_capture_task(
        silent_command("schtasks").args(["/run", "/tn", SCHEDULED_TASK_NAME]),
        &SUBMITTED_START_MS,
        now,
    )
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn silent_command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

struct RegistrationProcess(windows::Win32::Foundation::HANDLE);
// Kernel process handles may be waited on and closed from a different thread.
unsafe impl Send for RegistrationProcess {}
impl Drop for RegistrationProcess {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}
static PENDING_REGISTRATION: std::sync::Mutex<Option<RegistrationProcess>> =
    std::sync::Mutex::new(None);

fn settle_pending_registration() -> Result<(), String> {
    use windows::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::WaitForSingleObject;
    let mut pending = PENDING_REGISTRATION
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if let Some(process) = pending.as_ref() {
        match unsafe { WaitForSingleObject(process.0, 0) } {
            WAIT_OBJECT_0 => {
                pending.take();
                sayall_windows::gatt_note("rc003 feature=enhanced-capture action=elevated_install outcome=late_completion_ignored".to_owned());
            }
            WAIT_TIMEOUT => return Err("上一轮 Windows 授权仍在处理中，请完成后重试。".to_owned()),
            _ => return Err("尚未确认上一轮授权进程的状态，未发起新授权。".to_owned()),
        }
    }
    Ok(())
}

pub fn task_install_elevated(helper: &std::path::Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };

    settle_pending_registration()?;
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
    sei.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI;
    sei.lpVerb = PCWSTR(verb.as_ptr());
    sei.lpFile = PCWSTR(file.as_ptr());
    sei.lpParameters = PCWSTR(parameters.as_ptr());
    // SW_HIDE 双保险：新版助手本就没有窗口；万一拉起的是尚未升级的旧版
    // 控制台助手，也让它隐藏启动，而不是闪一下黑框。
    sei.nShow = 0;

    if let Err(error) = unsafe { ShellExecuteExW(&mut sei) } {
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
            "授权未完成（UAC 被取消）。本次开启未完成，可再次打开重试。".to_string()
        } else {
            format!("提权安装失败：{error}")
        });
    }
    let process = sei.hProcess;
    if process.is_invalid() || process == HANDLE::default() {
        return Err("提权流程没有返回助手进程句柄（安装未执行）".to_string());
    }
    let process = RegistrationProcess(process);
    let waited = unsafe { WaitForSingleObject(process.0, 30_000) };
    if waited != WAIT_OBJECT_0 {
        *PENDING_REGISTRATION
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(process);
        sayall_windows::gatt_note(format!("rc003 feature=enhanced-capture action=elevated_install outcome={} late_result_may_not_enable=true", if waited == WAIT_TIMEOUT { "timed_out" } else { "wait_failed" }));
        return Err("Windows 授权尚未完成，本次未开启全按键支持；完成后请重试。".to_owned());
    }
    let mut exit_code = 0u32;
    unsafe { GetExitCodeProcess(process.0, &mut exit_code) }
        .map_err(|_| "无法确认授权注册进程结果，本次未开启。".to_owned())?;
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

pub fn enable_capture(authorization_epoch: u64) -> Result<u64, String> {
    if capture_epoch() != authorization_epoch {
        return Err("全按键支持的启动请求已取消。".into());
    }
    settle_pending_registration()?;
    let helper = locate_helper_exe().ok_or_else(|| {
        "找不到 sayall-helper.exe（检查安装布局，或设置 SAYALL_RC003_HELPER 指向它）".to_string()
    })?;
    if AUTHORIZATION_REQUIRED_ON_EVERY_ENABLE {
        let before = task_registered_at();
        task_install_elevated(&helper)?;
        let after = task_registered_at();
        let verified = match (before.as_deref(), after.as_deref()) {
            (_, None) => false,
            (None, Some(_)) => true, // 之前无任务、之后有了 = 新建成功
            (Some(b), Some(a)) => b != a,
        };
        if !verified {
            return Err(
                "授权未完成（UAC 未被确认，任务没有重新注册）。本次开启未完成，可再次打开重试。"
                    .to_string(),
            );
        }
    }
    // Registration grants only the ability to run this fixed Helper. Recover
    // first even when a prior uninstall removed the task; never capture before
    // cleanup has been proved. Exit can invalidate the epoch throughout UAC.
    let expected_epoch = reset_capture(authorization_epoch)?;
    let result = task_trigger(expected_epoch);
    if result.is_ok() {
        clear_reauth_marker();
    }
    result.map(|()| expected_epoch)
}

#[derive(Debug, serde::Deserialize)]
struct CleanupReceipt {
    helper_pid: u32,
    started_unix_ms: u64,
    completed_unix_ms: Option<u64>,
    status: String,
    #[serde(default)]
    host_pid: u32,
    #[serde(default)]
    host_created: u64,
}

impl CleanupReceipt {
    fn settled(&self) -> bool {
        matches!(
            self.status.as_str(),
            "passed" | "agent_version_blocked" | "host_exited" | "not_started"
        ) && (self.status != "host_exited" || (self.host_pid != 0 && self.host_created != 0))
            && self.helper_pid != 0
            && self.started_unix_ms != 0
            && self
                .completed_unix_ms
                .is_some_and(|end| end >= self.started_unix_ms)
    }

    fn matches_process(&self, pid: u32, process_started_unix_ms: u64) -> bool {
        self.helper_pid == pid && self.started_unix_ms >= process_started_unix_ms
    }

    fn matches_submitted_start(&self, submitted_unix_ms: u64) -> bool {
        submitted_unix_ms == 0 || self.started_unix_ms >= submitted_unix_ms
    }

    fn recoverable(&self) -> bool {
        matches!(self.status.as_str(), "requested" | "unconfirmed")
            && self.host_pid != 0
            && self.host_created != 0
    }

    fn valid(&self) -> bool {
        self.helper_pid != 0
            && self.started_unix_ms != 0
            && match self.status.as_str() {
                "requested" => self.completed_unix_ms.is_none(),
                "passed" | "agent_version_blocked" | "host_exited" | "not_started" => {
                    self.settled()
                }
                "unconfirmed" => self
                    .completed_unix_ms
                    .is_none_or(|end| end >= self.started_unix_ms),
                _ => false,
            }
    }
}

fn cleanup_receipt_path() -> std::path::PathBuf {
    stop_signal_path().with_file_name("rc003-capture-cleanup.json")
}

fn read_cleanup_receipt() -> Result<Option<CleanupReceipt>, String> {
    match std::fs::read(cleanup_receipt_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "按键释放回执格式无效，尚未确认安全停止。".to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法读取按键释放回执，尚未确认安全停止。".to_owned()),
    }
}

pub fn start_was_rejected() -> bool {
    let submitted = SUBMITTED_START_MS.load(std::sync::atomic::Ordering::Acquire);
    rejected_start(read_cleanup_receipt().ok().flatten().as_ref(), submitted)
}

fn rejected_start(receipt: Option<&CleanupReceipt>, submitted: u64) -> bool {
    submitted != 0
        && receipt.is_some_and(|receipt| {
            receipt.status == "agent_version_blocked"
                && receipt.settled()
                && receipt.matches_submitted_start(submitted)
        })
}

#[derive(Debug)]
struct CleanupPresentation {
    pending: bool,
    can_retry: bool,
    reason: &'static str,
    message: Option<String>,
}

fn cleanup_presentation(
    receipt: Result<Option<CleanupReceipt>, String>,
    processes: Result<Vec<(u32, u64)>, String>,
    stop_requested: bool,
    submitted: u64,
) -> CleanupPresentation {
    let blocked = |can_retry, reason, message: String| CleanupPresentation {
        pending: true,
        can_retry,
        reason,
        message: Some(message),
    };
    let clear = |reason| CleanupPresentation {
        pending: false,
        can_retry: false,
        reason,
        message: None,
    };
    let processes = match processes {
        Ok(processes) => processes,
        Err(error) => return blocked(false, "process_observation_failed", error),
    };
    if processes.len() > 1 {
        return blocked(
            false,
            "multiple_helpers",
            "检测到多个按键助手，当前无法确认关闭状态。".into(),
        );
    }
    let active = processes.first().copied();
    let receipt = match receipt {
        Ok(receipt) => receipt,
        Err(error) => return blocked(active.is_some(), "receipt_unreadable", error),
    };
    if receipt.as_ref().is_some_and(|receipt| !receipt.valid()) {
        return blocked(
            active.is_some(),
            "receipt_invalid",
            "按键会话的结束记录无效，暂时无法确认关闭状态。".into(),
        );
    }
    let matching = receipt.as_ref().is_some_and(|receipt| {
        active.is_none_or(|(pid, started)| receipt.matches_process(pid, started))
            && receipt.matches_submitted_start(submitted)
    });
    if !stop_requested
        && submitted != 0
        && active.is_none_or(|(_, started)| started >= submitted)
        && receipt.as_ref().is_none_or(|receipt| {
            receipt.settled() && receipt.completed_unix_ms.is_some_and(|end| end < submitted)
        })
    {
        // Scheduling precedes both the process and its receipt. The bridge still
        // reports waiting; only an actual stop request makes this cleanup debt.
        return clear("start_requested");
    }
    if active.is_none() {
        if receipt.is_none() && submitted == 0 {
            return clear("no_session");
        }
        if matching && receipt.as_ref().is_some_and(CleanupReceipt::settled) {
            if !stop_requested
                && receipt
                    .as_ref()
                    .is_some_and(|receipt| receipt.status == "agent_version_blocked")
            {
                return CleanupPresentation {
                    pending: false,
                    can_retry: false,
                    reason: "agent_version_blocked",
                    message: Some("旧版按键组件仍被 Windows 占用。请在下次正常重启 Windows 后重新开启全按键支持。".into()),
                };
            }
            return clear(
                if receipt
                    .as_ref()
                    .is_some_and(|receipt| receipt.status == "agent_version_blocked")
                {
                    "start_rejected_without_capture"
                } else {
                    "release_confirmed"
                },
            );
        }
        if receipt.is_none() {
            return blocked(
                false,
                "start_unconfirmed",
                "尚未确认本次按键支持的启动结果，请刷新状态。".into(),
            );
        }
        return blocked(
            receipt.as_ref().is_some_and(CleanupReceipt::recoverable),
            "helper_exited_unconfirmed",
            "按键助手已退出，正在等待恢复上一次会话的按键状态。".into(),
        );
    }
    if !stop_requested
        && matching
        && receipt
            .as_ref()
            .is_some_and(|receipt| receipt.status == "requested")
    {
        // A running session creates its receipt before capture starts. This is
        // not a failed stop or proof that the bridge already owns any keys.
        return clear("session_running");
    }
    blocked(
        true,
        "release_pending",
        "上一次关闭尚未完成。请松开遥控器按键后重试关闭。".into(),
    )
}

// Scope process observations to this installation and Windows session.
fn helper_processes() -> Result<Vec<(u32, u64)>, String> {
    use windows::core::{HRESULT, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, FILETIME};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
        PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let Some(expected) = locate_helper_exe() else {
        return Ok(Vec::new());
    };
    let expected =
        std::fs::canonicalize(expected).map_err(|_| "无法核对当前安装的按键助手。".to_owned())?;
    let expected = expected.to_string_lossy();
    let expected = expected.trim_start_matches(r"\\?\");
    let mut current_session = 0;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut current_session) }
        .map_err(|_| "无法核对当前 Windows 会话。".to_owned())?;
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|_| "无法核对按键助手进程，未执行重启。".to_owned())?;
    let result = (|| {
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        unsafe { Process32FirstW(snapshot, &mut entry) }
            .map_err(|_| "无法读取按键助手进程列表。".to_owned())?;
        let mut found = Vec::new();
        loop {
            let end = entry
                .szExeFile
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(entry.szExeFile.len());
            let mut session = 0;
            let same_session = unsafe { ProcessIdToSessionId(entry.th32ProcessID, &mut session) }
                .is_ok()
                && session == current_session;
            if same_session
                && String::from_utf16_lossy(&entry.szExeFile[..end])
                    .eq_ignore_ascii_case("sayall-helper.exe")
            {
                let process = match unsafe {
                    OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION,
                        false,
                        entry.th32ProcessID,
                    )
                } {
                    Ok(handle) => Some(handle),
                    Err(error)
                        if error.code() == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) =>
                    {
                        None
                    }
                    Err(_) => return Err("无法核对按键助手身份，未执行重启。".to_owned()),
                };
                if let Some(process) = process {
                    let inspect: Result<Option<u64>, String> = (|| {
                        let mut image = vec![0u16; 32_768];
                        let mut length = image.len() as u32;
                        unsafe {
                            QueryFullProcessImageNameW(
                                process,
                                PROCESS_NAME_WIN32,
                                PWSTR(image.as_mut_ptr()),
                                &mut length,
                            )
                        }
                        .map_err(|_| "无法核对按键助手路径。".to_owned())?;
                        if !String::from_utf16_lossy(&image[..length as usize])
                            .eq_ignore_ascii_case(expected)
                        {
                            return Ok(None);
                        }
                        let mut created = FILETIME::default();
                        let mut exited = FILETIME::default();
                        let mut kernel = FILETIME::default();
                        let mut user = FILETIME::default();
                        unsafe {
                            GetProcessTimes(
                                process,
                                &mut created,
                                &mut exited,
                                &mut kernel,
                                &mut user,
                            )
                        }
                        .map_err(|_| "无法读取按键助手启动时间。".to_owned())?;
                        let ticks = (u64::from(created.dwHighDateTime) << 32)
                            | u64::from(created.dwLowDateTime);
                        Ok(Some((ticks / 10_000).saturating_sub(11_644_473_600_000)))
                    })();
                    let _ = unsafe { CloseHandle(process) };
                    if let Some(started) = inspect? {
                        found.push((entry.th32ProcessID, started));
                    }
                }
            }
            if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
        Ok(found)
    })();
    let _ = unsafe { CloseHandle(snapshot) };
    result
}

#[derive(Debug, PartialEq, Eq)]
enum StopProgress {
    Waiting,
    Recover,
    Complete,
}

struct StopAttempt {
    expected_process: Option<(u32, u64)>,
    submitted: u64,
    recovered: bool,
    queued_start: bool,
}

impl StopAttempt {
    fn new(submitted: u64) -> Self {
        Self {
            expected_process: None,
            submitted,
            recovered: false,
            queued_start: false,
        }
    }

    fn recovery_submitted(&mut self, submitted: u64) {
        self.expected_process = None;
        if !self.queued_start {
            self.submitted = submitted;
        }
        self.recovered = true;
    }

    fn observe(
        &mut self,
        processes: &[(u32, u64)],
        receipt: Option<&CleanupReceipt>,
    ) -> Result<StopProgress, String> {
        if processes.len() > 1 {
            return Err("当前安装存在多个按键助手，尚未确认统一释放状态。".into());
        }
        if receipt.is_some_and(|receipt| !receipt.valid()) {
            return Err("按键会话的结束记录无效，暂时无法确认关闭状态。".into());
        }
        if let Some(&process) = processes.first() {
            if self
                .expected_process
                .is_some_and(|expected| expected != process)
                || (self.recovered && process.1 < self.submitted)
            {
                return Err("停止期间出现新的按键助手，尚未确认释放状态。".into());
            }
            self.expected_process = Some(process);
        }
        let matching = receipt.is_some_and(|receipt| {
            self.expected_process
                .is_none_or(|(pid, started)| receipt.matches_process(pid, started))
                && receipt.matches_submitted_start(self.submitted)
        });
        if processes.is_empty() {
            if self.expected_process.is_none() && self.submitted == 0 && receipt.is_none() {
                return Ok(StopProgress::Complete);
            }
            if matching && receipt.is_some_and(CleanupReceipt::settled) {
                return Ok(StopProgress::Complete);
            }
            if !self.recovered {
                if receipt.is_none_or(|receipt| receipt.settled() || receipt.recoverable()) {
                    // A scheduler request can become visible just before the
                    // recovery /run call (which then merely observes that task).
                    // Keep its original lower bound only if it was unobserved
                    // and no unresolved receipt already belongs to that start.
                    self.queued_start = self.expected_process.is_none()
                        && self.submitted != 0
                        && receipt.is_none_or(|receipt| {
                            receipt.settled()
                                && receipt
                                    .completed_unix_ms
                                    .is_some_and(|end| end < self.submitted)
                        });
                    return Ok(StopProgress::Recover);
                }
                return Err("旧按键记录缺少可核实的宿主身份，已保留记录，未重新捕获按键。".into());
            }
            if matching
                && receipt
                    .is_some_and(|r| r.status == "unconfirmed" && r.completed_unix_ms.is_some())
            {
                return Err("按键恢复尚未确认完成，已保留释放记录。".into());
            }
        }
        Ok(StopProgress::Waiting)
    }
}

// Cancel startup immediately, before any potentially slow local shutdown work.
pub fn cancel_pending_start() {
    let _gate = CAPTURE_START_GATE.lock().unwrap_or_else(|p| p.into_inner());
    CAPTURE_EPOCH.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

fn request_stop(expected_epoch: Option<u64>) -> Result<(u64, u64), String> {
    use std::sync::atomic::Ordering;
    let _gate = CAPTURE_START_GATE.lock().unwrap_or_else(|p| p.into_inner());
    if expected_epoch.is_some_and(|expected| capture_epoch() != expected) {
        return Err("全按键支持的启动请求已取消。".into());
    }
    let epoch = CAPTURE_EPOCH.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    if let Some(parent) = stop_signal_path().parent() {
        std::fs::create_dir_all(parent).map_err(|_| "无法创建按键停止信号目录。".to_owned())?;
    }
    std::fs::write(stop_signal_path(), b"stop")
        .map_err(|_| "无法保存按键停止请求，未关闭释放通道。".to_owned())?;
    Ok((epoch, SUBMITTED_START_MS.load(Ordering::Acquire)))
}

pub fn reset_capture(expected_epoch: u64) -> Result<u64, String> {
    stop_capture(Some(expected_epoch))
}

pub fn disable_capture() -> Result<(), String> {
    stop_capture(None).map(|_| ())
}

fn stop_capture(expected_epoch: Option<u64>) -> Result<u64, String> {
    use std::sync::atomic::Ordering;
    let _stop = CAPTURE_STOP_GATE.lock().unwrap_or_else(|p| p.into_inner());
    let (epoch, submitted) = request_stop(expected_epoch)?;
    let mut attempt = StopAttempt::new(submitted);
    let mut deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let processes = helper_processes()?;
        let receipt = read_cleanup_receipt()?;
        match attempt.observe(&processes, receipt.as_ref())? {
            StopProgress::Complete => {
                sayall_windows::rc003_bridge::Rc003Bridge::confirm_capture_cleanup_completed();
                let _ = SUBMITTED_START_MS.compare_exchange(
                    submitted,
                    0,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                sayall_windows::gatt_note(format!("rc003 feature=enhanced-capture action=reset phase=completed terminal_result=passed recovery_attempted={}", attempt.recovered));
                return Ok(epoch);
            }
            StopProgress::Recover => {
                // Keep stop intent set: this task can only reconcile previous capture.
                // Never call task_trigger(), which removes that stop intent to enable.
                let recovery_started = trigger_cleanup_recovery(epoch)?;
                attempt.recovery_submitted(recovery_started);
                deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            }
            StopProgress::Waiting => {}
        }
        if std::time::Instant::now() >= deadline {
            sayall_windows::gatt_note(format!("rc003 feature=enhanced-capture action=reset phase=completed terminal_result=timeout recovery_attempted={} receipt_preserved=true", attempt.recovered));
            return Err("按键状态仍在恢复，释放记录已保留，重新打开时会继续处理。".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn trigger_cleanup_recovery(epoch: u64) -> Result<u64, String> {
    let _gate = CAPTURE_START_GATE.lock().unwrap_or_else(|p| p.into_inner());
    if capture_epoch() != epoch || !stop_signal_path().exists() {
        return Err("按键清理请求已被更新，本轮恢复已取消。".into());
    }
    if !task_installed() {
        return Err("按键恢复需要重新授权助手，现有释放记录已保留。".into());
    }
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "无法记录按键恢复代次。".to_owned())?
        .as_millis() as u64;
    if !schtasks(&["/run", "/tn", SCHEDULED_TASK_NAME])
        .ok_or_else(|| "无法确认按键恢复请求结果，释放记录已保留。".to_owned())?
    {
        return Err("未能提交按键恢复请求，释放记录已保留。".into());
    }
    sayall_windows::gatt_note("rc003 feature=enhanced-capture action=reset phase=submitted mode=cleanup_only terminal_result=pending".into());
    Ok(started)
}

fn stop_signal_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
        format!("{home}\\AppData\\Local")
    });
    std::path::PathBuf::from(base)
        .join("SayAll")
        .join("rc003-capture-stop")
}

pub fn reauth_marker_path() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string());
        format!("{home}\\AppData\\Local")
    });
    std::path::PathBuf::from(base)
        .join("SayAll")
        .join("rc003-reauth-required")
}

pub fn reauth_required() -> bool {
    reauth_marker_path().exists()
}

fn clear_reauth_marker() {
    let _ = std::fs::remove_file(reauth_marker_path());
}

fn task_registered_at() -> Option<String> {
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
    let text = String::from_utf16(&units).ok()?;
    extract_task_date(&text)
}

fn extract_task_date(xml: &str) -> Option<String> {
    let start = xml.find("<Date>")? + "<Date>".len();
    let end = xml[start..].find("</Date>")? + start;
    Some(xml[start..end].to_string())
}

pub fn status(enabled: bool) -> TaskStatus {
    let installed = task_installed();
    let cleanup = cleanup_presentation(
        read_cleanup_receipt(),
        helper_processes(),
        stop_signal_path().exists(),
        SUBMITTED_START_MS.load(std::sync::atomic::Ordering::Acquire),
    );
    static LAST_CLEANUP_STATUS: std::sync::Mutex<Option<(bool, bool, &'static str)>> =
        std::sync::Mutex::new(None);
    let observed = (cleanup.pending, cleanup.can_retry, cleanup.reason);
    let changed = {
        let mut last = LAST_CLEANUP_STATUS
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let changed = *last != Some(observed);
        *last = Some(observed);
        changed
    };
    if changed {
        sayall_windows::gatt_note(format!("rc003 feature=enhanced-capture action=status phase=observed cleanup_pending={} can_retry_cleanup={} reason={}", cleanup.pending, cleanup.can_retry, cleanup.reason));
    }
    TaskStatus {
        installed,
        // 每次开启都会触发系统授权（UAC）——2026-10-03 起恒为 true，
        // 与 enable_capture 的强制重装同源（同一个常量）。
        authorization_required: AUTHORIZATION_REQUIRED_ON_EVERY_ENABLE,
        enabled,
        cleanup_pending: cleanup.pending,
        can_retry_cleanup: cleanup.can_retry && installed,
        helper_path: locate_helper_exe().map(|p| p.display().to_string()),
        last_error: cleanup.message,
    }
}

fn run_capture_task(
    command: &mut std::process::Command,
    pending: &std::sync::atomic::AtomicU64,
    now: u64,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    let newly_pending = pending
        .compare_exchange(0, now, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    let clear_rejected_start = || {
        if newly_pending {
            let _ = pending.compare_exchange(now, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    };
    let child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| {
            clear_rejected_start();
            sayall_windows::gatt_note(format!(
                "rc003 feature=enhanced-capture action=task_trigger outcome=not_spawned pending_retained={}",
                pending.load(Ordering::Acquire) != 0
            ));
            format!("无法执行 schtasks（触发）：{error}")
        })?;
    // A wait/read failure cannot prove whether the scheduler accepted the request.
    let output = child.wait_with_output().map_err(|error| {
        sayall_windows::gatt_note("rc003 feature=enhanced-capture action=task_trigger outcome=unknown pending_retained=true".to_owned());
        format!("无法确认 schtasks 触发结果：{error}")
    })?;
    if output.status.success() {
        sayall_windows::gatt_note(
            "rc003 feature=enhanced-capture action=task_trigger outcome=submitted".to_owned(),
        );
        return Ok(());
    }
    if output.status.code().is_some() {
        clear_rejected_start();
    }
    sayall_windows::gatt_note(format!(
        "rc003 feature=enhanced-capture action=task_trigger outcome={} pending_retained={}",
        if output.status.code().is_some() {
            "rejected"
        } else {
            "unknown"
        },
        pending.load(Ordering::Acquire) != 0
    ));
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Err(format!(
        "schtasks 触发失败：{}",
        if stderr.is_empty() { stdout } else { stderr }
    ))
}

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

    fn status_receipt(
        status: &str,
        completed: Option<u64>,
    ) -> Result<Option<CleanupReceipt>, String> {
        Ok(Some(CleanupReceipt {
            helper_pid: 42,
            started_unix_ms: 2_000,
            completed_unix_ms: completed,
            status: status.into(),
            host_pid: 0,
            host_created: 0,
        }))
    }

    #[test]
    fn exited_unconfirmed_session_blocks_enable_without_offering_impossible_retry() {
        for stop_requested in [false, true] {
            let state = cleanup_presentation(
                status_receipt("unconfirmed", Some(3_000)),
                Ok(vec![]),
                stop_requested,
                0,
            );
            assert!(state.pending);
            assert!(!state.can_retry);
            assert!(state.message.unwrap().contains("已退出"));
        }
    }

    #[test]
    fn exited_helper_with_durable_host_identity_can_request_recovery() {
        let mut receipt = status_receipt("unconfirmed", Some(3_000)).unwrap().unwrap();
        receipt.host_pid = 17;
        receipt.host_created = 100;
        let state = cleanup_presentation(Ok(Some(receipt)), Ok(vec![]), true, 0);
        assert!(state.pending && state.can_retry);
    }

    #[test]
    fn cancelled_reset_never_writes_stop_or_cancels_a_newer_start() {
        let epoch = capture_epoch();
        assert!(reset_capture(epoch.wrapping_add(1)).is_err());
        assert_eq!(capture_epoch(), epoch);
    }

    #[test]
    fn auto_trigger_stops_only_for_its_own_terminal_version_rejection() {
        let receipt = status_receipt("agent_version_blocked", Some(3_000))
            .unwrap()
            .unwrap();
        assert!(rejected_start(Some(&receipt), 1_999));
        assert!(!rejected_start(Some(&receipt), 0));
        assert!(!rejected_start(Some(&receipt), 4_000));
        assert!(!rejected_start(None, 1_999));
    }

    #[test]
    fn active_session_is_not_cleanup_pending_until_stop_is_requested() {
        let state = cleanup_presentation(
            status_receipt("requested", None),
            Ok(vec![(42, 1_950)]),
            false,
            1_990,
        );
        assert!(!state.pending);
        assert!(state.message.is_none());
        let stopping = cleanup_presentation(
            status_receipt("requested", None),
            Ok(vec![(42, 1_950)]),
            true,
            1_990,
        );
        assert!(stopping.pending && stopping.can_retry);
        let exited =
            cleanup_presentation(status_receipt("requested", None), Ok(vec![]), false, 1_990);
        assert!(exited.pending && !exited.can_retry);
    }

    #[test]
    fn newly_submitted_start_waits_for_helper_receipt_without_reporting_failed_cleanup() {
        for processes in [vec![], vec![(43, 4_001)]] {
            for receipt in [Ok(None), status_receipt("passed", Some(3_000))] {
                let starting = cleanup_presentation(receipt, Ok(processes.clone()), false, 4_000);
                assert!(!starting.pending, "{starting:?}");
                assert_eq!(starting.reason, "start_requested");
            }
        }
        for receipt in [
            status_receipt("requested", None),
            status_receipt("unconfirmed", Some(3_000)),
        ] {
            assert!(cleanup_presentation(receipt, Ok(vec![]), false, 4_000).pending);
        }
        assert!(cleanup_presentation(Ok(None), Ok(vec![(42, 3_000)]), false, 4_000).pending);
    }

    #[test]
    fn version_rejection_without_capture_is_a_start_failure_not_cleanup_debt() {
        let rejected = cleanup_presentation(
            status_receipt("agent_version_blocked", Some(3_000)),
            Ok(vec![]),
            false,
            1_990,
        );
        assert!(!rejected.pending && !rejected.can_retry, "{rejected:?}");
        assert_eq!(rejected.reason, "agent_version_blocked");
        assert!(rejected.message.unwrap().contains("旧版按键组件"));
        let closed = cleanup_presentation(
            status_receipt("agent_version_blocked", Some(3_000)),
            Ok(vec![]),
            true,
            1_990,
        );
        assert!(!closed.pending && closed.message.is_none());
        for receipt in [
            status_receipt("agent_version_blocked", None),
            status_receipt("agent_version_blocked", Some(1_999)),
        ] {
            assert!(cleanup_presentation(receipt, Ok(vec![]), false, 1_990).pending);
        }
        assert!(
            cleanup_presentation(
                status_receipt("agent_version_blocked", Some(3_000)),
                Ok(vec![]),
                true,
                4_000
            )
            .pending
        );
    }

    #[test]
    fn cleanup_status_preserves_receipt_generation_and_unknown_boundaries() {
        assert!(!cleanup_presentation(Ok(None), Ok(vec![]), true, 0).pending);
        assert!(
            !cleanup_presentation(
                status_receipt("passed", Some(3_000)),
                Ok(vec![]),
                true,
                1_990
            )
            .pending
        );
        for state in [
            cleanup_presentation(
                status_receipt("passed", Some(3_000)),
                Ok(vec![]),
                true,
                2_001,
            ),
            cleanup_presentation(status_receipt("passed", None), Ok(vec![]), true, 0),
            cleanup_presentation(Ok(None), Ok(vec![]), true, 1_990),
            cleanup_presentation(Err("invalid receipt".into()), Ok(vec![]), false, 0),
            cleanup_presentation(Ok(None), Err("process query failed".into()), false, 0),
            cleanup_presentation(
                status_receipt("requested", None),
                Ok(vec![(42, 1_950), (43, 2_500)]),
                true,
                0,
            ),
        ] {
            assert!(state.pending && !state.can_retry, "{state:?}");
            assert!(state.message.is_some());
        }
    }

    #[test]
    fn definite_task_submission_failure_clears_only_its_new_pending_start() {
        use std::sync::atomic::{AtomicU64, Ordering};
        for previous in [0, 1_000] {
            let pending = AtomicU64::new(previous);
            let mut rejected = silent_command("cmd.exe");
            rejected.args(["/D", "/C", "exit", "3"]);
            assert!(run_capture_task(&mut rejected, &pending, 2_000).is_err());
            assert_eq!(pending.load(Ordering::Acquire), previous);

            let mut unavailable = silent_command("sayall-nonexistent-task-runner-test.exe");
            assert!(run_capture_task(&mut unavailable, &pending, 3_000).is_err());
            assert_eq!(pending.load(Ordering::Acquire), previous);
        }
    }

    #[test]
    fn accepted_task_submission_stays_pending_until_cleanup_is_observed() {
        use std::sync::atomic::{AtomicU64, Ordering};
        for previous in [0, 1_000] {
            let pending = AtomicU64::new(previous);
            let mut accepted = silent_command("cmd.exe");
            accepted.args(["/D", "/C", "exit", "0"]);
            assert!(run_capture_task(&mut accepted, &pending, 2_000).is_ok());
            assert_eq!(
                pending.load(Ordering::Acquire),
                if previous == 0 { 2_000 } else { previous }
            );
        }
    }

    #[test]
    fn cleanup_receipt_requires_a_terminal_ack_from_the_current_process() {
        let parse = |status: &str, ended: Option<u64>| CleanupReceipt {
            helper_pid: 42,
            started_unix_ms: 2_000,
            completed_unix_ms: ended,
            status: status.to_owned(),
            host_pid: 0,
            host_created: 0,
        };
        assert!(!parse("requested", None).settled());
        assert!(!parse("unconfirmed", None).settled());
        assert!(!parse("unconfirmed", Some(3_000)).settled());
        assert!(!parse("passed", None).settled());
        assert!(!parse("passed", Some(1_999)).settled());
        assert!(parse("agent_version_blocked", Some(3_000)).settled());
        let complete = parse("passed", Some(3_000));
        assert!(complete.settled());
        assert!(complete.matches_process(42, 1_950));
        assert!(!complete.matches_process(43, 1_950));
        assert!(!complete.matches_process(42, 2_001));
        assert!(complete.matches_submitted_start(1_990));
        assert!(!complete.matches_submitted_start(2_001));
    }

    #[test]
    fn cancelled_trigger_does_not_clear_stop_intent_or_run_a_task() {
        let stale_epoch = capture_epoch().wrapping_add(1);
        assert_eq!(
            task_trigger(stale_epoch).unwrap_err(),
            "全按键支持的启动请求已取消。"
        );
        assert_eq!(
            SUBMITTED_START_MS.load(std::sync::atomic::Ordering::Acquire),
            0
        );
    }

    #[test]
    fn killed_helper_recovers_once_and_accepts_only_fresh_recovery_receipt() {
        let mut old = status_receipt("unconfirmed", Some(3_000)).unwrap().unwrap();
        old.host_pid = 17;
        old.host_created = 100;
        let mut attempt = StopAttempt::new(0);
        assert_eq!(attempt.observe(&[], Some(&old)), Ok(StopProgress::Recover));
        attempt.recovery_submitted(4_000);
        assert_eq!(attempt.observe(&[], Some(&old)), Ok(StopProgress::Waiting));
        assert_eq!(
            attempt.observe(&[(43, 4_001)], Some(&old)),
            Ok(StopProgress::Waiting)
        );
        let mut fresh = old;
        fresh.helper_pid = 43;
        fresh.started_unix_ms = 4_002;
        fresh.completed_unix_ms = Some(4_003);
        fresh.status = "passed".into();
        assert_eq!(
            attempt.observe(&[], Some(&fresh)),
            Ok(StopProgress::Complete)
        );
        fresh.status = "unconfirmed".into();
        assert!(attempt.observe(&[], Some(&fresh)).is_err());
    }

    #[test]
    fn recovery_never_uses_unknown_identity_or_another_live_helper() {
        let old = status_receipt("unconfirmed", Some(3_000)).unwrap().unwrap();
        assert!(StopAttempt::new(0).observe(&[], Some(&old)).is_err());
        let mut attempt = StopAttempt::new(0);
        attempt.recovery_submitted(4_000);
        assert!(attempt.observe(&[(42, 1_950)], Some(&old)).is_err());
        assert!(attempt.observe(&[(43, 4_001), (44, 4_002)], None).is_err());
    }

    #[test]
    fn queued_start_may_appear_just_before_cleanup_task_submission() {
        let old = status_receipt("passed", Some(3_000)).unwrap().unwrap();
        let mut attempt = StopAttempt::new(4_000);
        assert_eq!(attempt.observe(&[], Some(&old)), Ok(StopProgress::Recover));
        // The original scheduler request starts in the gap between observation
        // and the recovery submission. It sees the same persistent stop intent.
        attempt.recovery_submitted(4_100);
        assert_eq!(
            attempt.observe(&[(43, 4_050)], Some(&old)),
            Ok(StopProgress::Waiting)
        );
        let mut fresh = status_receipt("not_started", Some(4_160)).unwrap().unwrap();
        fresh.helper_pid = 43;
        fresh.started_unix_ms = 4_060;
        assert_eq!(
            attempt.observe(&[], Some(&fresh)),
            Ok(StopProgress::Complete)
        );
    }

    #[test]
    fn host_exit_and_cancelled_start_are_real_terminal_states_not_guessed_release() {
        for status in ["host_exited", "not_started"] {
            let mut receipt = status_receipt(status, Some(3_000)).unwrap().unwrap();
            if status == "host_exited" {
                assert!(!receipt.settled());
                receipt.host_pid = 17;
                receipt.host_created = 100;
            }
            assert_eq!(
                StopAttempt::new(1_990).observe(&[], Some(&receipt)),
                Ok(StopProgress::Complete)
            );
            assert_ne!(
                StopAttempt::new(4_000).observe(&[], Some(&receipt)),
                Ok(StopProgress::Complete)
            );
            let invalid = status_receipt(status, None).unwrap().unwrap();
            assert!(StopAttempt::new(0).observe(&[], Some(&invalid)).is_err());
        }
        assert_eq!(
            StopAttempt::new(2_000).observe(&[], None),
            Ok(StopProgress::Recover)
        );
    }

    #[test]
    fn late_enable_completion_cannot_persist_or_activate() {
        let applied = std::cell::Cell::new(false);
        assert!(
            complete_capture_enable(capture_epoch().wrapping_add(1), || {
                applied.set(true);
                Ok(())
            })
            .is_err()
        );
        assert!(!applied.get());
    }

    #[test]
    fn cleanup_receipt_path_matches_helper_contract() {
        assert!(cleanup_receipt_path()
            .ends_with(std::path::Path::new("SayAll").join("rc003-capture-cleanup.json")));
    }

    #[test]
    fn task_name_matches_helper_side() {
        assert_eq!(SCHEDULED_TASK_NAME, "SayAll RC003 Helper");
    }

    #[test]
    fn trigger_args_use_task_name_verbatim() {
        let name = SCHEDULED_TASK_NAME;
        assert!(name.contains(' '));
        assert!(!name.starts_with('"') && !name.ends_with('"'));
    }

    #[test]
    fn stop_signal_path_matches_helper_side() {
        let path = stop_signal_path();
        assert!(path.ends_with(std::path::Path::new("SayAll").join("rc003-capture-stop")));
    }

    #[test]
    fn reauth_marker_path_matches_installer_side() {
        let path = reauth_marker_path();
        assert!(path.ends_with(std::path::Path::new("SayAll").join("rc003-reauth-required")));
    }

    #[test]
    fn extracts_task_registration_date() {
        let xml = "<?xml version=\"1.0\"?><Task><RegistrationInfo><Date>2026-09-24T20:27:20</Date>\
                   <Author>EXAMPLE\\test-user</Author></RegistrationInfo></Task>";
        assert_eq!(
            extract_task_date(xml),
            Some("2026-09-24T20:27:20".to_string())
        );
        assert_eq!(extract_task_date("<Task></Task>"), None);
    }

    #[test]
    fn every_enable_requires_reauthorization() {
        // 2026-10-03 Andy 定稿：每次打开开关都要重新弹窗 + 重新授权（每次都会
        // 弹 Windows 授权窗口）。这条一旦改回「任务在就不授权」，必须同步：
        // ① 前端弹窗（EnhancedCaptureConfirmDialog）文案与
        // ② ButtonsPage / ConnectionPage 的「每次开启都先弹确认」测试。
        assert!(AUTHORIZATION_REQUIRED_ON_EVERY_ENABLE);
    }
}
