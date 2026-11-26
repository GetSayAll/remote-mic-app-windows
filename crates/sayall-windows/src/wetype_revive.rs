//! Observe WeType microphone use through the public Windows ConsentStore.
//! Updates leave historical executable entries behind; enumeration order is
//! unrelated to the version that is currently recording.

use crate::ConnectionSnapshot;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use wasapi::{DeviceEnumerator, Direction, SessionState};
use windows::core::BOOL;
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, HWND, LPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
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

/// 微信输入法是否**正在占用录音端点**（WASAPI 会话，2026-11-26 新增）。
///
/// 背景：WeType 3.0.3.x 起不再注入 `0xFC`/`WTYP` 存活标记，也不更新 ConsentStore
/// 的历史时间戳。原有两条判据同时失明时，`NotReacted` 会被误判，随后的和弦重放
/// 会把正在录音的"按住说话"拆掉（2026-11-26 社区报告）。
///
/// 这条判据与输入法版本解耦：只要任一录音端点上有一个属于 WeType 的 Active
/// 会话，就证明它确实在录音。它只读取公开的音频会话状态与进程可执行文件名，
/// 不读取音频内容、路径或第三方私有数据。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CaptureEvidence {
    /// 有 WeType 进程持有 Active 录音会话（正面证据）。
    Active,
    /// 枚举成功且没有 WeType 的 Active 会话（可作为"未响应"的负面证据）。
    Inactive,
    /// COM/端点/会话枚举不可用：调用方不得据此做任何推断。
    Unavailable,
    /// 非微信输入法工具，不适用这条判据（保持既有裁决语义）。
    #[default]
    NotApplicable,
}

impl CaptureEvidence {
    pub(crate) fn as_log_str(self) -> &'static str {
        match self {
            CaptureEvidence::Active => "active",
            CaptureEvidence::Inactive => "inactive",
            CaptureEvidence::Unavailable => "unavailable",
            CaptureEvidence::NotApplicable => "not_applicable",
        }
    }

    /// 只有 `Inactive` 在稳定采样点才能当"未响应"的负面证据；录音会话从按下
    /// 到出现 Active 需要约 1 秒（2026-11-26 本机探针 1.02s），过早的不存在
    /// 不等于没响应。
    pub(crate) fn can_convict(self, settled: bool) -> bool {
        self == CaptureEvidence::Inactive && settled
    }
}

/// 进程是否属于微信输入法：可执行文件在 `WeType` 安装目录下，或文件名以
/// `wetype` 开头。探针实测录音会话由 `wetype_update.exe` 持有；按安装目录匹配
/// 是为了覆盖未来版本更换录音组件名的情况。只读取路径用于匹配，日志不记录路径。
fn is_wetype_process_image(path: &str) -> bool {
    let normalized = path.to_ascii_lowercase().replace('/', "\\");
    normalized.contains("\\wetype\\")
        || normalized
            .rsplit('\\')
            .next()
            .is_some_and(|name| name.starts_with("wetype"))
}

/// 枚举所有活动录音端点上的 WASAPI 会话，回答"WeType 现在是否正在录音"。
///
/// 失败安全：COM 初始化、端点或会话枚举任一步失败都返回 `Unavailable`，绝不
/// 返回 `Inactive`。这样上层在观测不可用时不会执行任何破坏性恢复动作。
pub(crate) fn wetype_capture_evidence() -> CaptureEvidence {
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized.is_err() {
        return CaptureEvidence::Unavailable;
    }
    let result = capture_evidence_for(is_wetype_process_image);
    unsafe {
        CoUninitialize();
    }
    result
}

/// 只有微信输入法适用录音会话判据；其他输入工具保持原裁决语义。
pub(crate) fn capture_for_marker(marker: &str) -> CaptureEvidence {
    if marker == "wetype" {
        wetype_capture_evidence()
    } else {
        CaptureEvidence::NotApplicable
    }
}

/// 录音会话从按下到出现 Active 的稳定观察点（本机 2026-11-26 实测 1.02s；
/// 取 2.5s 留出冷启动/输入法切换余量）。早于该点的"Inactive"只能算未知。
pub(crate) const CAPTURE_SETTLE_MS: u64 = 2500;

/// 单个录音端点上的会话判定：`Some(true)` = 有 WeType Active 会话，
/// `Some(false)` = 端点可枚举且没有，`None` = 该端点不可枚举。
fn device_session_evidence(
    device: &wasapi::Device,
    matches_process: fn(&str) -> bool,
) -> Option<bool> {
    let manager = device.get_iaudiosessionmanager().ok()?;
    let sessions = manager.get_audiosessionenumerator().ok()?;
    let count = sessions.get_count().ok()?;
    for index in 0..count {
        let Ok(session) = sessions.get_session(index) else {
            // 读不到某条会话就不能断言"没有 WeType 在录音"。
            return None;
        };
        match session.get_state() {
            Ok(SessionState::Active) => {}
            Ok(_) => continue,
            Err(_) => return None,
        }
        let Ok(process_id) = session.get_process_id() else {
            return None;
        };
        if process_id == 0 {
            // 系统/聚合会话不可能是微信输入法；不要让它把判据拖成不可用。
            continue;
        }
        let matched = match crate::app_launcher::process_image_path(process_id) {
            Some(path) => matches_process(&path),
            None => {
                // 进程名读不到（保护进程/权限边界）时用会话标识兜底；
                // 两者都读不到就不能把"Inactive"当负面证据。
                let identifier = session.get_session_identifier().unwrap_or_default();
                if identifier.is_empty() {
                    return None;
                }
                matches_process(&identifier)
            }
        };
        if matched {
            return Some(true);
        }
    }
    Some(false)
}

fn capture_evidence_for(matches_process: fn(&str) -> bool) -> CaptureEvidence {
    let enumerator = match DeviceEnumerator::new() {
        Ok(enumerator) => enumerator,
        Err(_) => return CaptureEvidence::Unavailable,
    };
    // 微信输入法的“自动检测”跟随系统默认录音设备；默认端点必须可枚举，
    // 否则"Inactive"不是可靠负面证据（用户手动选定的其它端点仍会在下面补查）。
    let mut unknown = false;
    let default_ok = match enumerator.get_default_device(&Direction::Capture) {
        Ok(device) => match device_session_evidence(&device, matches_process) {
            Some(true) => return CaptureEvidence::Active,
            Some(false) => true,
            None => {
                unknown = true;
                false
            }
        },
        Err(_) => false,
    };
    let collection = match enumerator.get_device_collection(&Direction::Capture) {
        Ok(collection) => collection,
        Err(_) => return CaptureEvidence::Unavailable,
    };
    let mut other_ok = false;
    for device in &collection {
        let Ok(device) = device else { continue };
        match device_session_evidence(&device, matches_process) {
            Some(true) => return CaptureEvidence::Active,
            Some(false) => other_ok = true,
            None => unknown = true,
        }
    }
    if unknown {
        CaptureEvidence::Unavailable
    } else if default_ok || other_ok {
        CaptureEvidence::Inactive
    } else {
        CaptureEvidence::Unavailable
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

/// 合并裁决的日志标记（`voice_verify ... verdict=`，与 `wetype_check` 同词表）。
fn reaction_log_str(reaction: WetypeReaction) -> &'static str {
    match reaction {
        WetypeReaction::Reacted => "reacted",
        WetypeReaction::NotReacted => "not_reacted",
        WetypeReaction::Unknown => "unknown",
    }
}

/// 合并裁决：任一**正面**证据（标记 / 开麦 / 录音会话）都判 `Reacted`，
/// 否决一切破坏性恢复动作。
///
/// 负面裁决（`NotReacted`）的规则在 2026-11-26 收紧：
/// - 微信输入法：不再把"ConsentStore 没更新"当独立负面证据（3.0.3.x 起它不写），
///   必须由 WASAPI 录音会话在稳定采样点确认"没有在录音"才判 `NotReacted`；
///   录音会话观测不可用时判 `Unknown`，**不重发按键**（报告 #… 的诉求）。
/// - 其他输入工具：保持既有语义（`NotApplicable` + 开麦 `NotObserved` → `NotReacted`）。
pub(crate) fn reaction_verdict(
    mic: MicResponse,
    marker_baseline: u64,
    marker_now: u64,
    capture: CaptureEvidence,
    capture_settled: bool,
) -> WetypeReaction {
    if marker_now > marker_baseline {
        // 微信输入法自注入标记只可能来自它自己（0xFC break key + extra="WTYP"）：
        // 出现即证明其钩子活着并已消费本次和弦，任何重放都是破坏性的。
        return WetypeReaction::Reacted;
    }
    if mic == MicResponse::Observed || capture == CaptureEvidence::Active {
        return WetypeReaction::Reacted;
    }
    if capture.can_convict(capture_settled) {
        return WetypeReaction::NotReacted;
    }
    if capture == CaptureEvidence::NotApplicable && mic == MicResponse::NotObserved {
        // 豆包 / Vokie 等不适用录音会话判据的工具：沿用原行为。
        return WetypeReaction::NotReacted;
    }
    WetypeReaction::Unknown
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

/// 自动救回的上下文：按下目标工具有可切换的输入法目标时携带。
pub(crate) struct ImeHealContext {
    pub state: Arc<Mutex<ConnectionSnapshot>>,
    /// 用户选的输入工具（救回第 2 档要对齐它对应的输入法）。
    pub tool: sayall_core::settings::VoiceInputTool,
}

/// 救回动作档位（2026-10-28，逐级升级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HealAction {
    /// 第 1 档：只做焦点往返（不切输入法、不改前台窗口，零副作用）。
    Bounce,
    /// 第 2 档起：先对齐目标输入法（强制切一次），再做焦点往返。
    AlignThenBounce,
    /// 到上限：停手，等用户下一次按下。
    Stop,
}

/// 档位选择（纯函数，单测覆盖）：首次最轻，之后升级，到 `IME_HEAL_STEP_CAP` 停。
fn heal_action_for_step(step: u32) -> HealAction {
    if step >= IME_HEAL_STEP_CAP {
        HealAction::Stop
    } else if step == 0 {
        HealAction::Bounce
    } else {
        HealAction::AlignThenBounce
    }
}

/// 是否执行自动救回（纯函数，单测覆盖）：**按合并裁决判**，且必须"确认未响应"。
///
/// 为什么不是按开麦观测（2026-10-28 本机实测）：10/10 会话 `mic=not_observed`
/// 而 marker 命中（WeType 不写 ConsentStore，issue #118）。按 mic 判会让健康机器
/// 每按一次都触发一次对齐动作——把诊断手段变成新的故障源。
fn should_heal(verdict: WetypeReaction, eligible: bool) -> bool {
    eligible && verdict == WetypeReaction::NotReacted
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
///
/// 2026-10-28 追加 F5 三判据（现场：`mic=not_observed` 的按下里抑制器一条
/// F5 边沿都收不到，而宿主当时是活的；日志无法区分"语音键的键盘报告没进
/// OS"与"进了 OS 但被我们的钩子绕过"）：
/// - `f5_edges`：本次按住期间抑制器收到的 DOWN 边沿数（相对按下前基线）；
/// - `f5_leaked`：其中被放行进 OS 的条数（>0 即和弦会被目标按"额外按键"拒绝）；
/// - `f5_key_down`：`GetAsyncKeyState(F5)` 的 OS 侧键态（不受本进程钩子影响）。
/// 三者组合的读法见 `Testing/WindowsVoiceF5Probe.md`。
///
/// 2026-10-28 同轮修正两处语义：
/// - 行里加 `verdict=`（合并裁决，marker 优先），与 `wetype_check` 同源——
///   `mic=` 单独在本机会全程 `not_observed`（issue #118），不能当判据；
/// - 救回判据从"开麦未观测"改为"合并裁决确认未响应"，并按 `HealAction`
///   逐级升级（第 1 档只焦点往返，第 2 档起强制对齐目标输入法）。
///
/// 2026-11-26：`voice_verify` 同步记录 `capture=`（WASAPI 录音会话）。
/// 会话已经结束（短按/已松手）时，录音会话消失属于正常收尾，不能当"没响应"
/// 的证据；只有仍处于 Streaming 的稳定采样点才允许用 `capture=inactive` 判负。
pub(crate) fn spawn_mic_verify(
    session: u64,
    tool: &'static str,
    marker: &'static str,
    baseline: Option<MicObservation>,
    f5_baseline: crate::key_suppressor::F5Snapshot,
    marker_baseline: u64,
    session_state: Arc<Mutex<ConnectionSnapshot>>,
    heal: Option<ImeHealContext>,
) {
    const SAMPLES_MS: [u64; 2] = [800, 2500];
    let _ = std::thread::Builder::new()
        .name("sayall-mic-verify".to_owned())
        .spawn(move || {
            let started = std::time::Instant::now();
            let mut reaction = WetypeReaction::Unknown;
            for delay in SAMPLES_MS {
                let elapsed = started.elapsed().as_millis() as u64;
                if elapsed < delay {
                    std::thread::sleep(std::time::Duration::from_millis(delay - elapsed));
                }
                let mic = response_since(baseline, mic_observation_for(marker));
                let streaming = {
                    let snapshot = session_state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    snapshot.voice_state == sayall_core::VoiceSessionState::Streaming
                };
                let capture = capture_for_marker(marker);
                reaction = reaction_verdict(
                    mic,
                    marker_baseline,
                    crate::key_suppressor::wetype_marker_count(),
                    capture,
                    // 会话已结束时，录音会话消失是正常收尾，不是"没响应"的证据；
                    // 只有按住期间（仍在 Streaming）的稳定点才允许用 Inactive 判负。
                    delay >= CAPTURE_SETTLE_MS && streaming,
                );
                let f5 = crate::key_suppressor::f5_snapshot().delta_since(f5_baseline);
                crate::ble::gatt_note(format!(
                    "voice_verify session={session} tool={tool} marker={marker} at_ms={} mic={} capture={} verdict={} f5_edges={} f5_leaked={} f5_key_down={}",
                    started.elapsed().as_millis(),
                    mic.as_log_str(),
                    capture.as_log_str(),
                    reaction_log_str(reaction),
                    f5.edges,
                    f5.leaked,
                    if crate::key_suppressor::f5_key_down_in_os() {
                        "true"
                    } else {
                        "false"
                    },
                ));
            }
            if reaction != WetypeReaction::NotReacted {
                // 被唤起过（或观测不足以下结论）：救回计数归零，且绝不动作。
                IME_HEAL_STEPS.store(0, Ordering::Relaxed);
            }
            let eligible = heal.is_some();
            if !should_heal(reaction, eligible) {
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
            // 动作前再裁决一次：等会话结束的这段时间里恢复阶梯可能已经重放和弦
            // 把语音拉起来了（用户一直按着）。迟到的正面证据必须否决动作，
            // 否则会把刚起来的会话拆掉（10-20 教训）。
            let recheck = reaction_verdict(
                response_since(baseline, mic_observation_for(marker)),
                marker_baseline,
                crate::key_suppressor::wetype_marker_count(),
                capture_for_marker(marker),
                true,
            );
            if recheck == WetypeReaction::Reacted {
                IME_HEAL_STEPS.store(0, Ordering::Relaxed);
                crate::ble::gatt_note(format!(
                    "voice_heal session={session} tool={tool} action=focus_bounce skipped=reacted_late"
                ));
                return;
            }
            let steps = IME_HEAL_STEPS.load(Ordering::Relaxed);
            let action = heal_action_for_step(steps);
            if action == HealAction::Stop {
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
            // 第 2 档起先对齐目标输入法：应用侧 `already_active` 读的是本进程会话，
            // 看不见目标窗口（2026-10-28 现场：`last_switch_age_ms=never` 全程没切过），
            // 所以这里强制切一次，让目标窗口的会话绑回用户选的输入法。
            if action == HealAction::AlignThenBounce {
                let aligned = crate::ime::heal_target_ime(heal.tool);
                crate::ble::gatt_note(format!(
                    "voice_heal session={session} tool={tool} action=ime_align step={} outcome={}",
                    steps + 1,
                    match &aligned {
                        Ok(activation) => activation.log_token(),
                        Err(_) => "failed",
                    },
                ));
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
    #[ignore = "requires installed WeType; read-only"]
    fn live_capture_session_observation_is_readable() {
        let mut readable = false;
        for _ in 0..12 {
            let evidence = wetype_capture_evidence();
            println!("wetype_capture_evidence={evidence:?}");
            readable |= evidence != CaptureEvidence::Unavailable;
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
        assert!(readable, "capture session enumeration unavailable");
    }

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

    /// 2026-10-28：救回判据必须用**合并裁决**，不能用开麦观测。
    /// 本机实测 10/10 会话 `mic=not_observed` 而 marker 命中（issue #118 盲判形态）；
    /// 若按 mic 判，健康机器上每按一次都会触发一次对齐动作（无谓打扰）。
    #[test]
    fn should_heal_uses_merged_verdict_not_mic() {
        assert!(!should_heal(WetypeReaction::Reacted, true));
        assert!(should_heal(WetypeReaction::NotReacted, true));
        assert!(!should_heal(WetypeReaction::NotReacted, false));
        assert!(!should_heal(WetypeReaction::Unknown, true));
    }

    /// 2026-10-28：救回动作必须逐级升级——首次只做零副作用的焦点往返，
    /// 同一目标再次失败才动输入法（切换 + 焦点往返），到上限停手。
    /// 理由：切换会让目标窗口重绑输入法上下文（Chromium 系可能整页重绘），
    /// 不能作为每次失败的第一反应。
    #[test]
    fn heal_action_escalates_from_bounce_to_align_then_stops() {
        assert_eq!(heal_action_for_step(0), HealAction::Bounce);
        assert_eq!(heal_action_for_step(1), HealAction::AlignThenBounce);
        assert_eq!(heal_action_for_step(2), HealAction::AlignThenBounce);
        assert_eq!(heal_action_for_step(IME_HEAL_STEP_CAP), HealAction::Stop);
        assert_eq!(
            heal_action_for_step(IME_HEAL_STEP_CAP + 1),
            HealAction::Stop
        );
    }

    /// 2026-10-28：救回动作执行前必须**再裁决一次**——按下后 2.5 秒判"未响应"、
    /// 用户又一直按着时，恢复阶梯可能已经重放和弦并把语音拉起来了；此时再做
    /// 焦点往返/切换会把它拆掉（10-20 教训）。迟到的正面证据必须否决动作。
    #[test]
    fn late_positive_evidence_cancels_a_pending_heal() {
        // 采样时未响应（marker 未前进、录音端点无活动会话）→ 可以救。
        assert!(should_heal(
            reaction_verdict(
                MicResponse::NotObserved,
                7,
                7,
                CaptureEvidence::Inactive,
                true
            ),
            true
        ));
        // 动作前再裁决：marker 前进 → 放弃动作。
        assert!(!should_heal(
            reaction_verdict(
                MicResponse::NotObserved,
                7,
                9,
                CaptureEvidence::Inactive,
                true
            ),
            true
        ));
    }

    #[test]
    fn capture_session_is_positive_evidence_when_marker_and_consentstore_are_blind() {
        // 2026-11-26 现场（WeType 3.0.3.x）：marker 不再前进，ConsentStore 也不更新，
        // 但录音端点上有 WeType 的 Active 会话。必须判 Reacted，绝不重发和弦。
        assert_eq!(
            reaction_verdict(
                MicResponse::NotObserved,
                7,
                7,
                CaptureEvidence::Active,
                false
            ),
            WetypeReaction::Reacted
        );
    }

    #[test]
    fn inactive_capture_session_only_convicts_after_settle() {
        // 按下后 ~1s 录音会话才出现；700ms 的 Inactive 只是"还没看到"，不是
        // "没响应"。稳定采样点（>=2.5s）仍无会话才允许恢复。
        assert_eq!(
            reaction_verdict(
                MicResponse::NotObserved,
                7,
                7,
                CaptureEvidence::Inactive,
                false
            ),
            WetypeReaction::Unknown
        );
        assert_eq!(
            reaction_verdict(
                MicResponse::NotObserved,
                7,
                7,
                CaptureEvidence::Inactive,
                true
            ),
            WetypeReaction::NotReacted
        );
    }

    #[test]
    fn unavailable_capture_session_never_convicts_wechat() {
        // 2026-11-26 报告诉求：判据读不到时"不重发按键、只提示"，不得把
        // ConsentStore 的 not_observed 当成证据（WeType 3.x 不写它）。
        assert_eq!(
            reaction_verdict(
                MicResponse::NotObserved,
                7,
                7,
                CaptureEvidence::Unavailable,
                true
            ),
            WetypeReaction::Unknown
        );
    }

    #[test]
    fn capture_inactive_still_allows_recovery_when_consentstore_is_unavailable() {
        // 反面：录音端点可枚举且确认无活动会话时，即使 ConsentStore 读不到，
        // 也足以判定微信输入法未响应，保留真休眠的自动恢复。
        assert_eq!(
            reaction_verdict(MicResponse::Unknown, 7, 7, CaptureEvidence::Inactive, true),
            WetypeReaction::NotReacted
        );
    }

    #[test]
    fn non_wechat_tools_keep_legacy_mic_verdict() {
        assert_eq!(
            reaction_verdict(
                MicResponse::NotObserved,
                7,
                7,
                CaptureEvidence::NotApplicable,
                true
            ),
            WetypeReaction::NotReacted
        );
        assert_eq!(
            reaction_verdict(
                MicResponse::Unknown,
                7,
                7,
                CaptureEvidence::NotApplicable,
                true
            ),
            WetypeReaction::Unknown
        );
    }

    #[test]
    fn wetype_process_image_matching_is_name_or_install_dir_and_case_insensitive() {
        assert!(is_wetype_process_image(
            r"C:\Program Files\Tencent\WeType\3.0.3.8\wetype_update.exe"
        ));
        assert!(is_wetype_process_image(
            r"C:\Program Files\Tencent\WeType\wetype_server.exe"
        ));
        // 未来版本即使换了录音组件名，只要仍在 WeType 安装目录下也应命中。
        assert!(is_wetype_process_image(
            r"C:\Program Files\Tencent\WeType\3.1\voice_helper.exe"
        ));
        assert!(is_wetype_process_image(r"WETYPE_RENDERER.EXE"));
        assert!(!is_wetype_process_image(r"C:\Windows\System32\notepad.exe"));
        assert!(!is_wetype_process_image(r"C:\Tools\notwetype.exe"));
        assert!(!is_wetype_process_image(""));
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
            reaction_verdict(
                MicResponse::NotObserved,
                7,
                9,
                CaptureEvidence::Inactive,
                true
            ),
            WetypeReaction::Reacted
        );
    }

    #[test]
    fn dormant_hook_without_marker_and_without_mic_opening_stays_recoverable() {
        // 2026-09-05 真休眠：无标记、无开麦、录音端点也没有活动会话 →
        // NotReacted，恢复阶梯照旧可用。
        assert_eq!(
            reaction_verdict(
                MicResponse::NotObserved,
                7,
                7,
                CaptureEvidence::Inactive,
                true
            ),
            WetypeReaction::NotReacted
        );
    }

    #[test]
    fn marker_evidence_beats_unavailable_mic_observation() {
        // 观测不可用但标记前进：已有正面存活证据，不得报"不可用"。
        assert_eq!(
            reaction_verdict(
                MicResponse::Unknown,
                7,
                8,
                CaptureEvidence::Unavailable,
                true
            ),
            WetypeReaction::Reacted
        );
    }

    #[test]
    fn unavailable_mic_observation_without_marker_is_never_dormancy() {
        // 失败安全保持：所有观测都不可用且无标记 → Unknown（不得恢复）。
        assert_eq!(
            reaction_verdict(
                MicResponse::Unknown,
                7,
                7,
                CaptureEvidence::Unavailable,
                true
            ),
            WetypeReaction::Unknown
        );
    }

    #[test]
    fn mic_opening_without_marker_is_still_reacted() {
        assert_eq!(
            reaction_verdict(MicResponse::Observed, 7, 7, CaptureEvidence::Inactive, true),
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
