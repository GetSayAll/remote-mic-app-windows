//! Observe WeType microphone use through the public Windows ConsentStore.
//! Updates leave historical executable entries behind; enumeration order is
//! unrelated to the version that is currently recording.

use crate::ConnectionSnapshot;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use windows::core::BOOL;
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, HWND, LPARAM};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    REG_QWORD, REG_VALUE_TYPE,
};

const CONSENT_NONPACKAGED: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone\\NonPackaged";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MicObservation {
    latest_start: u64,
    active: bool,
    entries: usize,
}

impl MicObservation {
    fn record(&mut self, start: u64, stop: u64) {
        self.latest_start = self.latest_start.max(start);
        self.active |= start > 0 && stop == 0;
        self.entries += 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MicResponse {
    Observed,
    NotObserved,
    Unknown,
}

impl MicResponse {
    /// 诊断日志用的开麦判据结果标记。与标记判据分别记录，才能在复现窗口区分
    /// "两条通道同时命中"与"只有标记命中"（后者正是 issue #118 的盲判形态）。
    pub(crate) fn as_log_str(self) -> &'static str {
        match self {
            MicResponse::Observed => "observed",
            MicResponse::NotObserved => "not_observed",
            MicResponse::Unknown => "unknown",
        }
    }
}

pub(crate) fn response_since(
    baseline: Option<MicObservation>,
    current: Option<MicObservation>,
) -> MicResponse {
    match (baseline, current) {
        (_, Some(current)) if current.active => MicResponse::Observed,
        (Some(base), Some(current)) if current.latest_start > base.latest_start => {
            MicResponse::Observed
        }
        (Some(base), Some(current))
            if current.entries >= base.entries && current.latest_start == base.latest_start =>
        {
            MicResponse::NotObserved
        }
        _ => MicResponse::Unknown,
    }
}

/// 录入会话开始前的微信输入法麦克风观测基线（latest_start；None = 观测不可用）。
///
/// 微信输入法的 LL 钩子吞掉其语音热键组成键的物理边沿发生在 RIT 层，对本进程
/// 的一切用户态通道（低级钩子、Raw Input、GetAsyncKeyState）都不可见（探针实测，
/// 见 2026-09-27 诊断）。录入会话零/半截边沿时，"微信输入法语音是否在会话期间
/// 被触发"是判断用户按了其语音热键的唯一可观测旁证。
pub fn capture_mic_baseline() -> Option<u64> {
    wetype_mic_observation().map(|observation| observation.latest_start)
}

/// 录入会话结束后判定微信输入法语音是否在会话期间被触发。
///
/// 基线只携带 latest_start（active 语义由 current.active 分支兜底）。返回
/// "observed"（触发了新录音或仍在录音）、"not_observed"（确认未触发）或
/// "unknown"（观测不可用，调用方不得据此做任何推断）。
pub fn capture_mic_verdict(baseline: Option<u64>) -> &'static str {
    let baseline = baseline.map(|latest_start| MicObservation {
        latest_start,
        active: false,
        entries: 0,
    });
    match response_since(baseline, wetype_mic_observation()) {
        MicResponse::Observed => "observed",
        MicResponse::NotObserved => "not_observed",
        MicResponse::Unknown => "unknown",
    }
}

/// 微信输入法"本次按住是否真的被触发"的合并裁决。
///
/// 两个**相互独立**的证据源：
/// - ConsentStore 开麦观测（历史判据，见本模块顶部说明）；
/// - 钩子层观测到的微信输入法自注入标记（`0xFC` break key，extra="WTYP"）：
///   其钩子存活时吞掉和弦的 LWin 边沿并注入自己的标记边沿对，休眠时无标记
///   （2026-09-05 kb-live 实测，与开麦时间戳 100% 交叉一致，见 ATTRIBUTION.md）。
///
/// 2026-09-23 社区报告（issue #118 / PR #119）：微信输入法 2.1.4.6 起录音不再
/// 写 ConsentStore，单靠开麦观测会把"其实已在录音"判成未响应，随后重放和弦
/// 拆掉进行中的会话（长按约 2.9s 即断）。标记是**正面存活证据**，一旦在本次
/// 按住期间出现即否决一切破坏性恢复动作；判据缺失时退化为原行为（只减不增，
/// 绝不比现状更差）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WetypeReaction {
    /// 已有正面证据证明本次按住触发了微信输入法。
    Reacted,
    /// 两个证据源都确认未触发：此时恢复阶梯（配置切换 + 重放和弦）仍可执行。
    NotReacted,
    /// 观测不可用，调用方不得据此做任何推断。
    Unknown,
}

/// 合并裁决：标记证据优先于开麦观测（正面证据否决误判）。
pub(crate) fn reaction_verdict(
    mic: MicResponse,
    marker_baseline: u64,
    marker_now: u64,
) -> WetypeReaction {
    if marker_now > marker_baseline {
        // 微信输入法自注入标记只可能来自它自己（0xFC break key + extra="WTYP"）：
        // 出现即证明其钩子活着并已消费本次和弦，任何重放都是破坏性的。
        return WetypeReaction::Reacted;
    }
    match mic {
        MicResponse::Observed => WetypeReaction::Reacted,
        MicResponse::NotObserved => WetypeReaction::NotReacted,
        MicResponse::Unknown => WetypeReaction::Unknown,
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn read_qword(key: HKEY, name: &str) -> Option<u64> {
    let value = wide(name);
    let mut data = 0_u64;
    let mut size = std::mem::size_of::<u64>() as u32;
    let mut kind = REG_VALUE_TYPE::default();
    let result = unsafe {
        RegQueryValueExW(
            key,
            windows::core::PCWSTR(value.as_ptr()),
            None,
            Some(&mut kind),
            Some((&mut data as *mut u64).cast()),
            Some(&mut size),
        )
    };
    (result.0 == 0 && kind == REG_QWORD && size == 8).then_some(data)
}

/// Missing or partially unreadable observations must not trigger recovery.
pub(crate) fn wetype_mic_observation() -> Option<MicObservation> {
    mic_observation_for("wetype")
}

/// 指定标记（ConsentStore 条目名子串，**须小写**）的麦克风观测，语义与微信版一致：
/// 缺条目、条目不可读一律返回 None（调用方不得据此做任何推断）。
///
/// 2026-10-09 起供 `voice_verify` 复用：豆包=`doubao`（本机实测条目
/// `ImeService.exe` 路径含该片段，录音启停更新时间戳）、Vokie=`vokie`。
pub(crate) fn mic_observation_for(marker: &str) -> Option<MicObservation> {
    let subkey = wide(CONSENT_NONPACKAGED);
    let mut root = HKEY::default();
    if unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(subkey.as_ptr()),
            None,
            KEY_READ,
            &mut root,
        )
    }
    .0 != 0
    {
        return None;
    }
    let mut index = 0;
    let mut observation = MicObservation::default();
    let mut complete = true;
    loop {
        let mut name = [0u16; 260];
        let mut len = name.len() as u32;
        let result = unsafe {
            RegEnumKeyExW(
                root,
                index,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
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
            complete = false;
            break;
        }
        index += 1;
        let entry = String::from_utf16_lossy(&name[..len as usize]);
        if !entry.to_ascii_lowercase().contains(marker) {
            continue;
        }
        let mut key = HKEY::default();
        let entry_wide = wide(&entry);
        if unsafe {
            RegOpenKeyExW(
                root,
                windows::core::PCWSTR(entry_wide.as_ptr()),
                None,
                KEY_READ,
                &mut key,
            )
        }
        .0 != 0
        {
            complete = false;
            continue;
        }
        let start = read_qword(key, "LastUsedTimeStart");
        let stop = read_qword(key, "LastUsedTimeStop");
        unsafe {
            let _ = RegCloseKey(key);
        }
        match (start, stop) {
            (Some(start), Some(stop)) => observation.record(start, stop),
            _ => complete = false,
        }
    }
    unsafe {
        let _ = RegCloseKey(root);
    }
    (complete && observation.entries > 0).then_some(observation)
}

/// 连续"未响应"计数（跨按下累计；任一按下判为 Observed 即归零）。
static IME_HEAL_STEPS: AtomicU32 = AtomicU32::new(0);

/// 连续救回上限（防循环的行为卫生上限；用户每重按一次至多触发一次救回）。
const IME_HEAL_STEP_CAP: u32 = 4;

/// EnumWindows 回调：采集"替身窗口"候选（经由 LPARAM 传出）。
unsafe extern "system" fn collect_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let found = &mut *(lparam.0 as *mut Vec<HWND>);
    found.push(hwnd);
    BOOL(1)
}

/// 程序化"焦点往返"（2026-10-10 真机验证，替换步进方案）：**不改变前台窗口、
/// 不切换任何输入法**——借目标线程里的替身窗口，让前台窗口收到一对
/// `WM_KILLFOCUS/WM_SETFOCUS`，使其重绑输入法上下文；僵死态的直达解药。
///
/// 为什么是它：会话级切换（`TF_IPPMF_FORSESSION`）够不到"窗口自己的输入法"；
/// 通知类消息（`WM_INPUTLANGCHANGE` / `WM_INPUTLANGCHANGEREQUEST`）实测均无效；
/// 用户手动"点走再点回"有效，本函数即其最小等价（真机：僵死中 bounce →
/// 下一次按下 `mic=observed`）。
///
/// 有界返回，只输出诊断片段供 `voice_heal` 日志；不做重试。
pub(crate) fn focus_bounce_foreground() -> String {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowThreadProcessId,
    };
    unsafe {
        let target = GetForegroundWindow();
        if target.0.is_null() {
            return "target=none".to_owned();
        }
        let target_thread = GetWindowThreadProcessId(target, None);
        let current_thread = GetCurrentThreadId();
        let mut windows: Vec<HWND> = Vec::new();
        let _ = EnumWindows(
            Some(collect_window),
            LPARAM(&mut windows as *mut _ as isize),
        );
        let attached = AttachThreadInput(current_thread, target_thread, true);
        if !attached.as_bool() {
            return format!("target=0x{:X} attached=false", target.0 as usize);
        }
        // 替身窗口必须在目标线程里，且 SetFocus 成功；一个失败换下一个。
        let mut kicker: Option<HWND> = None;
        for hwnd in windows.iter().copied() {
            if hwnd == target || GetWindowThreadProcessId(hwnd, None) != target_thread {
                continue;
            }
            if SetFocus(Some(hwnd)).is_ok() {
                kicker = Some(hwnd);
                break;
            }
        }
        if kicker.is_none() {
            // 无可用替身：退化为直接摘焦点（底层调用仍会发生）。
            let _ = SetFocus(None);
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
        let restored = SetFocus(Some(target)).is_ok();
        let detached = AttachThreadInput(current_thread, target_thread, false).as_bool();
        format!(
            "target=0x{:X} attached=true kicker=0x{:X} restored={restored} detached={detached}",
            target.0 as usize,
            kicker.map(|hwnd| hwnd.0 as usize).unwrap_or(0)
        )
    }
}

/// 自动救回的上下文：仅当按下目标工具有可切换的输入法目标时携带。
pub(crate) struct ImeHealContext {
    pub state: Arc<Mutex<ConnectionSnapshot>>,
}

/// 是否执行自动救回（纯函数，单测覆盖）：仅在"确认未响应"且具备条件时动作。
fn should_heal(verdict: MicResponse, eligible: bool) -> bool {
    eligible && verdict == MicResponse::NotObserved
}

/// 有界等待语音会话结束（结束才注入，避免与按住中的和弦叠加）。
fn wait_session_end(state: &Arc<Mutex<ConnectionSnapshot>>, budget_ms: u64) -> bool {
    let started = std::time::Instant::now();
    loop {
        let idle = {
            let guard = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.voice_state != sayall_core::VoiceSessionState::Streaming
        };
        if idle {
            return true;
        }
        if started.elapsed().as_millis() as u64 >= budget_ms {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// 每次语音按下的"输入法是否真的开麦"验证（2026-10-09 Andy：日志需要增加）。
///
/// 在固定延迟点读回 ConsentStore 观测，与按下前的基线比对，落
/// `voice_verify ... mic=observed|not_observed|unknown`。
/// `observed` = 输入法在按下后确实开始/持续录音（语音条被拉起的客观旁证）；
/// `unknown` = 观测不可用，不得据此推断（如该输入法不写 ConsentStore）。
///
/// 2026-10-10 起附带**自动救回**（直达、用户无感）：两轮判据均确认未响应 →
/// 等会话结束后对前台窗口做一次程序化"焦点往返"（`focus_bounce_foreground`，
/// 不切换输入法、不改前台窗口）。恢复（observed）即计数归零；连续未恢复
/// 达到 cap 上限后停止继续动作（防循环）。
pub(crate) fn spawn_mic_verify(
    session: u64,
    tool: &'static str,
    marker: &'static str,
    baseline: Option<MicObservation>,
    heal: Option<ImeHealContext>,
) {
    const SAMPLES_MS: [u64; 2] = [800, 2500];
    let _ = std::thread::Builder::new()
        .name("sayall-mic-verify".to_owned())
        .spawn(move || {
            let started = std::time::Instant::now();
            let mut verdict = MicResponse::Unknown;
            for delay in SAMPLES_MS {
                let elapsed = started.elapsed().as_millis() as u64;
                if elapsed < delay {
                    std::thread::sleep(std::time::Duration::from_millis(delay - elapsed));
                }
                verdict = response_since(baseline, mic_observation_for(marker));
                crate::ble::gatt_note(format!(
                    "voice_verify session={session} tool={tool} marker={marker} at_ms={} mic={}",
                    started.elapsed().as_millis(),
                    verdict.as_log_str(),
                ));
            }
            if verdict == MicResponse::Observed {
                // 真正被唤起过一次：整套状态健康，救回计数归零。
                IME_HEAL_STEPS.store(0, Ordering::Relaxed);
            }
            let eligible = heal.is_some();
            if !should_heal(verdict, eligible) {
                return;
            }
            let Some(heal) = heal else { return };
            if !wait_session_end(&heal.state, 6000) {
                crate::ble::gatt_note(format!(
                    "voice_heal session={session} tool={tool} action=focus_bounce skipped=session_active"
                ));
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(350));
            let steps = IME_HEAL_STEPS.load(Ordering::Relaxed);
            if steps >= IME_HEAL_STEP_CAP {
                crate::ble::gatt_note(format!(
                    "voice_heal session={session} tool={tool} action=focus_bounce skipped=cap steps={steps} cap={IME_HEAL_STEP_CAP}"
                ));
                return;
            }
            if crate::ime::foreground_is_self() {
                crate::ble::gatt_note(format!(
                    "voice_heal session={session} tool={tool} action=focus_bounce skipped=self_foreground"
                ));
                return;
            }
            let outcome = focus_bounce_foreground();
            IME_HEAL_STEPS.fetch_add(1, Ordering::Relaxed);
            crate::ble::gatt_note(format!(
                "voice_heal session={session} tool={tool} action=focus_bounce step={} cap={} {outcome}",
                IME_HEAL_STEPS.load(Ordering::Relaxed),
                IME_HEAL_STEP_CAP,
            ));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires installed WeType microphone history; read-only"]
    fn live_consentstore_observation_is_readable() {
        let observation = wetype_mic_observation().expect("WeType observation unavailable");
        assert!(observation.entries > 0);
        println!(
            "wetype_observation entries={} active={}",
            observation.entries, observation.active
        );
    }

    fn stopped(start: u64) -> MicObservation {
        let mut result = MicObservation::default();
        result.record(start, start + 1);
        result
    }

    #[test]
    fn should_heal_only_on_confirmed_not_observed_and_eligible() {
        assert!(should_heal(MicResponse::NotObserved, true));
        assert!(!should_heal(MicResponse::NotObserved, false));
        assert!(!should_heal(MicResponse::Observed, true));
        assert!(!should_heal(MicResponse::Unknown, true));
    }

    #[test]
    fn mic_response_log_tokens_are_distinct_and_stable() {
        // 三个标记必须互不相同，否则日志无法区分"盲判"（开麦 not_observed 而
        // 标记命中）与"两条通道同时命中"。
        assert_eq!(MicResponse::Observed.as_log_str(), "observed");
        assert_eq!(MicResponse::NotObserved.as_log_str(), "not_observed");
        assert_eq!(MicResponse::Unknown.as_log_str(), "unknown");
        assert_ne!(
            MicResponse::Observed.as_log_str(),
            MicResponse::NotObserved.as_log_str()
        );
    }

    #[test]
    fn marker_evidence_vetoes_recovery_when_mic_probe_is_blind() {
        // issue #118：WeType 2.1.4.6 起录音不再写 ConsentStore → 开麦观测判
        // NotObserved，但钩子层仍看到它的自注入标记（说明它正在工作）。
        // 必须判 Reacted：绝不重放和弦拆掉进行中的会话。
        assert_eq!(
            reaction_verdict(MicResponse::NotObserved, 7, 9),
            WetypeReaction::Reacted
        );
    }

    #[test]
    fn dormant_hook_without_marker_and_without_mic_opening_stays_recoverable() {
        // 2026-09-05 真休眠：无标记、无开麦 → NotReacted，恢复阶梯照旧可用。
        assert_eq!(
            reaction_verdict(MicResponse::NotObserved, 7, 7),
            WetypeReaction::NotReacted
        );
    }

    #[test]
    fn marker_evidence_beats_unavailable_mic_observation() {
        // 观测不可用但标记前进：已有正面存活证据，不得报"不可用"。
        assert_eq!(
            reaction_verdict(MicResponse::Unknown, 7, 8),
            WetypeReaction::Reacted
        );
    }

    #[test]
    fn unavailable_mic_observation_without_marker_is_never_dormancy() {
        // 失败安全保持：观测不可用且无标记 → Unknown（调用方不得据此恢复）。
        assert_eq!(
            reaction_verdict(MicResponse::Unknown, 7, 7),
            WetypeReaction::Unknown
        );
    }

    #[test]
    fn mic_opening_without_marker_is_still_reacted() {
        assert_eq!(
            reaction_verdict(MicResponse::Observed, 7, 7),
            WetypeReaction::Reacted
        );
    }

    #[test]
    fn historical_versions_do_not_hide_current_recording() {
        let mut baseline = MicObservation::default();
        for start in [10, 20, 30, 40, 50, 60, 70] {
            baseline.record(start, start + 1);
        }
        let mut current = MicObservation::default();
        for start in [10, 20, 30, 40, 50, 60] {
            current.record(start, start + 1);
        }
        current.record(80, 0);
        assert_eq!(
            response_since(Some(baseline), Some(current)),
            MicResponse::Observed
        );
        let mut reversed = MicObservation::default();
        reversed.record(80, 0);
        for start in [60, 50, 40, 30, 20, 10] {
            reversed.record(start, start + 1);
        }
        assert_eq!(current, reversed);
    }

    #[test]
    fn active_recording_is_not_retried_even_if_baseline_already_contains_start() {
        let mut active = stopped(10);
        active.active = true;
        assert_eq!(
            response_since(Some(active), Some(active)),
            MicResponse::Observed
        );
        assert_eq!(response_since(None, Some(active)), MicResponse::Observed);
    }

    #[test]
    fn capture_baseline_reconstruction_only_carries_latest_start() {
        // capture_mic_verdict 用 latest_start 重构基线（active=false、entries=0）。
        // 持平 → NotObserved；前进 → Observed；基线缺失 → Unknown（不得推断）。
        let base = MicObservation {
            latest_start: 10,
            active: false,
            entries: 0,
        };
        assert_eq!(
            response_since(Some(base), Some(stopped(10))),
            MicResponse::NotObserved
        );
        assert_eq!(
            response_since(Some(base), Some(stopped(20))),
            MicResponse::Observed
        );
        assert_eq!(
            response_since(None, Some(stopped(20))),
            MicResponse::Unknown
        );
    }

    #[test]
    fn new_completed_recording_is_a_response() {
        assert_eq!(
            response_since(Some(stopped(10)), Some(stopped(20))),
            MicResponse::Observed
        );
    }

    #[test]
    fn unchanged_inactive_recording_allows_recovery() {
        assert_eq!(
            response_since(Some(stopped(10)), Some(stopped(10))),
            MicResponse::NotObserved
        );
    }

    #[test]
    fn missing_or_regressed_observations_do_not_authorize_recovery() {
        assert_eq!(response_since(None, None), MicResponse::Unknown);
        assert_eq!(
            response_since(Some(stopped(10)), None),
            MicResponse::Unknown
        );
        assert_eq!(
            response_since(None, Some(stopped(10))),
            MicResponse::Unknown
        );
        assert_eq!(
            response_since(Some(stopped(20)), Some(stopped(10))),
            MicResponse::Unknown
        );
    }
}
