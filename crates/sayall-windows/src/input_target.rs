//! 注入目标诊断探针（2026-11-26）。
//!
//! 现场：拼多多商家版前台时，遥控器映射的方向键/回车/字母全部无效，且注入事件
//! 连本应用的全局低级键盘钩子都看不到（`map_inject echo_delta=0`），而同一发
//! 音量键能改系统音量、物理键盘在该窗口正常。原有字段只能回答"发没发、回声有
//! 没有"，回答不了"打在谁身上、对方什么权限、焦点在哪个进程"。
//!
//! 本模块只做**只读采样**，不改任何行为，回答以下几类此前只能靠猜的事实：
//!
//! - 目标是不是我们自己的窗口（`fg_self`）；
//! - 目标进程的完整性级别/是否提权（UIPI 会拦掉普通进程发往高完整性窗口的模拟输入）；
//! - 前台窗口的焦点控件属于哪个进程、类名哈希是什么（焦点被托管进另一个进程时，
//!   按键会打到那个进程的队列）；
//! - 前台窗口是否可见/最小化（不可见窗口拿不到键）；
//! - 前台线程的键盘布局（按物理键位理解注入键时，布局决定扫描码翻译）；
//! - 注入前钩子累计看到过多少键（与注入回声放在一起，可判"物理键还看得见吗"）。
//!
//! 隐私边界：只落**类名哈希**（FNV-1a 64 高位，8 位十六进制，与 `ime` 侧同一算法）、
//! 进程号、完整性级别和十六进制布局值；不落窗口标题、可执行文件路径或用户名。

use crate::key_gate;

/// 三态布尔：读不到时如实落 `unknown`，不猜。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tri {
    True,
    False,
    Unknown,
}

impl Tri {
    fn as_str(self) -> &'static str {
        match self {
            Self::True => "true",
            Self::False => "false",
            Self::Unknown => "unknown",
        }
    }
}

impl Default for Tri {
    fn default() -> Self {
        Self::Unknown
    }
}

/// 类名的稳定哈希（FNV-1a 64 → 高 32 位，8 位十六进制；输入先转小写）。
///
/// 与 `ime` 侧共用同一实现（`ime` 只做 re-export），避免两份哈希算法随版本漂移。
pub(crate) fn class_token(class: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in class.to_ascii_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", (hash >> 32) as u32)
}

/// 一次映射注入前的前台/焦点快照（只读）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TargetProbe {
    pub fg_pid: u32,
    /// 前台窗口类名哈希；没有前台窗口时 `none`。
    pub fg_class: String,
    /// 前台线程键盘布局（`GetKeyboardLayout`）。
    pub fg_hkl: u32,
    /// 前台线程焦点控件的类名哈希；无焦点控件时 `none`。
    pub focus_class: String,
    /// 焦点控件所属进程；无焦点控件时 0。
    pub focus_pid: u32,
    /// `low|medium|high|system|protected|unknown`。
    pub integrity: &'static str,
    pub elevated: Tri,
    pub visible: Tri,
    pub iconic: Tri,
    pub self_foreground: bool,
}

/// 本进程输入环境快照（启动时落一次）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputEnv {
    pub integrity: &'static str,
    pub elevated: Tri,
    /// `HKCU\Control Panel\Desktop\LowLevelHooksTimeout`，未设置/读不到为 `None`。
    pub low_level_hooks_timeout_ms: Option<u32>,
    /// `SAYALL_DIAG_NO_HOOKS` 的稳定标记（未设置为 `none`）。
    pub diag_no_hooks: &'static str,
    pub hook_host_pid: u32,
}

/// `map_target` 行（纯函数，单测覆盖）：
///
/// ```text
/// map_target fg_pid=23148 fg_class=4aafa526 fg_hkl=0x08040804 fg_focus=4aafa526
///   fg_focus_pid=23148 fg_integrity=medium fg_elevated=false fg_visible=true
///   fg_iconic=false fg_self=false hook_calls_total=97
/// ```
///
/// 语义边界：这些字段只描述"注入时前台是谁、焦点在哪、对方什么权限"，
/// **不证明按键投递结果**；投递结果仍看紧随其后的 `map_inject echo_delta`。
pub fn format_map_target(probe: &TargetProbe, hook_calls_total: u64) -> String {
    format!(
        "map_target fg_pid={} fg_class={} fg_hkl=0x{:08X} fg_focus={} fg_focus_pid={} \
         fg_integrity={} fg_elevated={} fg_visible={} fg_iconic={} fg_self={} \
         hook_calls_total={}",
        probe.fg_pid,
        probe.fg_class,
        probe.fg_hkl,
        probe.focus_class,
        probe.focus_pid,
        probe.integrity,
        probe.elevated.as_str(),
        probe.visible.as_str(),
        probe.iconic.as_str(),
        probe.self_foreground,
        hook_calls_total,
    )
}

/// `input_env` 行（纯函数，单测覆盖）。
pub fn format_input_env(env: &InputEnv) -> String {
    let timeout = env
        .low_level_hooks_timeout_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unset".to_owned());
    format!(
        "input_env own_integrity={} own_elevated={} low_level_hooks_timeout_ms={} \
         diag_no_hooks={} hook_host_pid={}",
        env.integrity,
        env.elevated.as_str(),
        timeout,
        env.diag_no_hooks,
        env.hook_host_pid,
    )
}

/// 采集前台/焦点快照并格式化为 `map_target` 行。任何一项读不到都落 `unknown`/`none`。
pub fn map_target_note() -> String {
    format_map_target(&probe_foreground(), key_gate::hook_calls_total())
}

/// 采集本进程输入环境并格式化为 `input_env` 行。
pub fn input_env_note(hook_host_pid: u32) -> String {
    let mut env = probe_input_env();
    env.hook_host_pid = hook_host_pid;
    format_input_env(&env)
}

/// 完整性级别的偏序（UIPI：低完整性进程不能向更高完整性的窗口投递模拟输入）。
fn integrity_rank(token: &str) -> u8 {
    match token {
        "low" => 1,
        "medium" => 2,
        "high" => 3,
        "system" => 4,
        "protected" => 5,
        // `unknown` 不参与比较：读不到不等于"更高"或"更低"。
        _ => 0,
    }
}

/// 本进程完整性（进程生命周期内不变，缓存一次；读不到为 `unknown`）。
pub fn own_integrity() -> &'static str {
    static OWN: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    OWN.get_or_init(|| probe_input_env().integrity)
}

/// 目标完整性严格高于本进程 → 模拟输入会被 UIPI 丢弃（纯判据，单测覆盖）。
fn uipi_blocks(own: &str, target: &str) -> bool {
    let own_rank = integrity_rank(own);
    let target_rank = integrity_rank(target);
    own_rank > 0 && target_rank > 0 && target_rank > own_rank
}

/// 本次注入的目标是否会被 UIPI 拦下（目标窗口完整性高于本进程）。
pub fn uipi_blocks_injection(target: &TargetProbe) -> bool {
    uipi_blocks(own_integrity(), target.integrity)
}

/// 累计被 UIPI 拦下的映射注入次数（诊断摘要用；进程生命周期内累加）。
static UIPI_BLOCKED_TOTAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn uipi_blocked_total() -> u64 {
    UIPI_BLOCKED_TOTAL.load(std::sync::atomic::Ordering::Relaxed)
}

/// 记录一次 UIPI 拦下的映射注入。
pub(crate) fn record_uipi_blocked() {
    UIPI_BLOCKED_TOTAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// `SAYALL_DIAG_NO_HOOKS` 的稳定标记：只接受已知取值，其余一律 `set`。
fn diag_no_hooks_token(value: Option<&str>) -> &'static str {
    match value.map(str::trim) {
        None | Some("") | Some("0") => "none",
        Some("1") => "1",
        Some(value) if value.eq_ignore_ascii_case("both") => "both",
        Some(value) if value.eq_ignore_ascii_case("gate") => "gate",
        Some(value) if value.eq_ignore_ascii_case("all") => "all",
        Some(_) => "set",
    }
}

/// 完整性级别 RID → 稳定标记。
fn integrity_token(rid: u32) -> &'static str {
    match rid {
        0x0000..=0x1FFF => "low",
        0x2000..=0x2FFF => "medium",
        0x3000..=0x3FFF => "high",
        0x4000..=0x4FFF => "system",
        0x5000..=0xFFFF => "protected",
        _ => "unknown",
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TokenIntegrityLevel, TOKEN_ELEVATION,
        TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    };
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    use windows::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, GUITHREADINFO,
    };

    fn window_class(hwnd: HWND) -> Option<String> {
        if hwnd.0.is_null() {
            return None;
        }
        let mut buffer = [0u16; 256];
        let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
        (len > 0).then(|| String::from_utf16_lossy(&buffer[..len as usize]))
    }

    fn class_or_none(hwnd: HWND) -> String {
        window_class(hwnd)
            .filter(|class| !class.is_empty())
            .map(|class| class_token(&class))
            .unwrap_or_else(|| "none".to_owned())
    }

    /// 进程 token 的完整性 RID 与提权标志。读不到分别返回 `None`。
    fn token_security(pid: u32) -> (Option<u32>, Option<bool>) {
        if pid == 0 {
            return (None, None);
        }
        unsafe {
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return (None, None);
            };
            let mut token = HANDLE::default();
            let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token).is_ok();
            let _ = CloseHandle(process);
            if !opened {
                return (None, None);
            }
            let (rid, elevated) = (read_integrity_rid(token), read_elevated(token));
            let _ = CloseHandle(token);
            (rid, elevated)
        }
    }

    unsafe fn read_integrity_rid(token: HANDLE) -> Option<u32> {
        use windows::Win32::Security::SID;
        let mut size = 0u32;
        let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut size);
        if size == 0 {
            return None;
        }
        let mut buffer = vec![0u8; size as usize];
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            Some(buffer.as_mut_ptr() as *mut c_void),
            size,
            &mut size,
        )
        .ok()?;
        let label = &*(buffer.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let psid = label.Label.Sid;
        if psid.0.is_null() {
            return None;
        }
        let sid = &*(psid.0 as *const SID);
        let count = sid.SubAuthorityCount as usize;
        if count == 0 {
            return None;
        }
        // `SubAuthority` 在结构体里声明为长度 1 的柔性数组：按指针取最后一项。
        Some((sid.SubAuthority.as_ptr().add(count - 1)).read())
    }

    unsafe fn read_elevated(token: HANDLE) -> Option<bool> {
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;
        GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
        .ok()?;
        Some(elevation.TokenIsElevated != 0)
    }

    pub fn probe_foreground() -> TargetProbe {
        let mut probe = TargetProbe {
            fg_class: "none".to_owned(),
            focus_class: "none".to_owned(),
            integrity: "unknown",
            ..Default::default()
        };
        unsafe {
            let foreground = GetForegroundWindow();
            if foreground.0.is_null() {
                return probe;
            }
            let mut pid = 0u32;
            let thread_id = GetWindowThreadProcessId(foreground, Some(&mut pid));
            probe.fg_pid = pid;
            probe.fg_class = class_or_none(foreground);
            probe.self_foreground = pid != 0 && pid == std::process::id();
            probe.visible = if IsWindowVisible(foreground).as_bool() {
                Tri::True
            } else {
                Tri::False
            };
            probe.iconic = if IsIconic(foreground).as_bool() {
                Tri::True
            } else {
                Tri::False
            };
            if thread_id != 0 {
                probe.fg_hkl = GetKeyboardLayout(thread_id).0 as u32;
                let mut info = GUITHREADINFO {
                    cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                    ..Default::default()
                };
                if GetGUIThreadInfo(thread_id, &mut info).is_ok() && !info.hwndFocus.0.is_null() {
                    probe.focus_class = class_or_none(info.hwndFocus);
                    let mut focus_pid = 0u32;
                    GetWindowThreadProcessId(info.hwndFocus, Some(&mut focus_pid));
                    probe.focus_pid = focus_pid;
                }
            }
        }
        let (rid, elevated) = token_security(probe.fg_pid);
        probe.integrity = rid.map(integrity_token).unwrap_or("unknown");
        probe.elevated = elevated
            .map(|value| if value { Tri::True } else { Tri::False })
            .unwrap_or(Tri::Unknown);
        probe
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn low_level_hooks_timeout_ms() -> Option<u32> {
        let subkey = wide("Control Panel\\Desktop");
        let value = wide("LowLevelHooksTimeout");
        let mut data = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                PCWSTR(value.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut data as *mut u32 as *mut c_void),
                Some(&mut size),
            )
        };
        (status.is_ok() && size == std::mem::size_of::<u32>() as u32).then_some(data)
    }

    pub fn probe_input_env() -> InputEnv {
        let (rid, elevated) = token_security(unsafe { GetCurrentProcessId() });
        InputEnv {
            integrity: rid.map(integrity_token).unwrap_or("unknown"),
            elevated: elevated
                .map(|value| if value { Tri::True } else { Tri::False })
                .unwrap_or(Tri::Unknown),
            low_level_hooks_timeout_ms: low_level_hooks_timeout_ms(),
            diag_no_hooks: diag_no_hooks_token(
                std::env::var("SAYALL_DIAG_NO_HOOKS").ok().as_deref(),
            ),
            hook_host_pid: 0,
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{probe_foreground, probe_input_env};

#[cfg(not(windows))]
pub fn probe_foreground() -> TargetProbe {
    TargetProbe {
        fg_class: "none".to_owned(),
        focus_class: "none".to_owned(),
        integrity: "unknown",
        ..Default::default()
    }
}

#[cfg(not(windows))]
pub fn probe_input_env() -> InputEnv {
    InputEnv {
        integrity: "unknown",
        diag_no_hooks: diag_no_hooks_token(std::env::var("SAYALL_DIAG_NO_HOOKS").ok().as_deref()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_target() -> TargetProbe {
        TargetProbe {
            fg_pid: 23148,
            fg_class: "4aafa526".to_owned(),
            fg_hkl: 0x0804_0804,
            focus_class: "4aafa526".to_owned(),
            focus_pid: 23148,
            integrity: "medium",
            elevated: Tri::False,
            visible: Tri::True,
            iconic: Tri::False,
            self_foreground: false,
        }
    }

    #[test]
    fn map_target_line_reports_every_probed_field() {
        let line = format_map_target(&sample_target(), 97);
        assert_eq!(
            line,
            "map_target fg_pid=23148 fg_class=4aafa526 fg_hkl=0x08040804 fg_focus=4aafa526 \
             fg_focus_pid=23148 fg_integrity=medium fg_elevated=false fg_visible=true \
             fg_iconic=false fg_self=false hook_calls_total=97"
        );
    }

    #[test]
    fn map_target_without_foreground_stays_explicit() {
        let probe = TargetProbe {
            fg_class: "none".to_owned(),
            focus_class: "none".to_owned(),
            integrity: "unknown",
            ..Default::default()
        };
        let line = format_map_target(&probe, 0);
        assert!(line.starts_with("map_target fg_pid=0 fg_class=none"));
        assert!(line.contains("fg_integrity=unknown"));
        assert!(line.contains("fg_elevated=unknown"));
        assert!(line.contains("hook_calls_total=0"));
    }

    #[test]
    fn map_target_line_never_carries_plaintext_window_class() {
        let line = format_map_target(&sample_target(), 1);
        for forbidden in ["Chrome", "Pdd", "Pinduoduo", "Qt515"] {
            assert!(!line.contains(forbidden), "{line} leaked {forbidden}");
        }
        assert!(line.contains("fg_class=4aafa526"));
    }

    #[test]
    fn class_token_is_stable_and_case_insensitive() {
        assert_eq!(class_token("Notepad"), class_token("notepad"));
        assert_eq!(class_token("Notepad"), "62bde74a");
        assert_ne!(class_token("Notepad"), class_token("Chrome_WidgetWin_1"));
    }

    #[test]
    fn input_env_line_marks_unset_timeout_and_diag_flag() {
        let env = InputEnv {
            integrity: "medium",
            elevated: Tri::False,
            low_level_hooks_timeout_ms: None,
            diag_no_hooks: "none",
            hook_host_pid: 21100,
        };
        assert_eq!(
            format_input_env(&env),
            "input_env own_integrity=medium own_elevated=false \
             low_level_hooks_timeout_ms=unset diag_no_hooks=none hook_host_pid=21100"
        );
    }

    #[test]
    fn diag_no_hooks_token_only_accepts_known_values() {
        assert_eq!(diag_no_hooks_token(None), "none");
        assert_eq!(diag_no_hooks_token(Some("0")), "none");
        assert_eq!(diag_no_hooks_token(Some("1")), "1");
        assert_eq!(diag_no_hooks_token(Some("BOTH")), "both");
        assert_eq!(diag_no_hooks_token(Some("gate")), "gate");
        assert_eq!(diag_no_hooks_token(Some("all")), "all");
        assert_eq!(diag_no_hooks_token(Some("C:\\temp\\secret")), "set");
    }

    #[test]
    fn integrity_tokens_stay_in_stable_english() {
        assert_eq!(integrity_token(0x1000), "low");
        assert_eq!(integrity_token(0x2000), "medium");
        assert_eq!(integrity_token(0x3000), "high");
        assert_eq!(integrity_token(0x4000), "system");
        assert_eq!(integrity_token(0x5000), "protected");
    }

    #[test]
    fn uipi_only_blocks_targets_strictly_above_own_integrity() {
        // 现场（2026-11-27）：无线麦 medium、拼多多商家版 high → 注入被系统丢弃。
        assert!(uipi_blocks("medium", "high"));
        assert!(uipi_blocks("medium", "system"));
        assert!(uipi_blocks("low", "medium"));
        // 同级别与更低级别不受 UIPI 影响。
        assert!(!uipi_blocks("medium", "medium"));
        assert!(!uipi_blocks("medium", "low"));
        assert!(!uipi_blocks("high", "medium"));
        // unknown 不参与比较：读不到不得当成"会被拦"。
        assert!(!uipi_blocks("unknown", "high"));
        assert!(!uipi_blocks("medium", "unknown"));
    }

    #[test]
    fn uipi_injection_check_reads_the_target_probe() {
        // 不依赖测试进程自身完整性（CI runner 可能是已提权的）：
        // 只断言"判据取的是 target.integrity"，其余由 `uipi_blocks` 的纯测试覆盖。
        let high = TargetProbe {
            integrity: "high",
            ..Default::default()
        };
        let unknown = TargetProbe {
            integrity: "unknown",
            ..Default::default()
        };
        assert!(!uipi_blocks_injection(&unknown));
        assert_eq!(
            uipi_blocks_injection(&high),
            uipi_blocks(own_integrity(), "high")
        );
    }
}
