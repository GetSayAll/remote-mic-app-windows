//! 微信输入法（WeType）会话级激活。
//!
//! 背景（2026-09-05 持锁实验实锤，Testing\investigation\p-ime-experiment.ps1
//! 与 examples\ime_probe_mta.rs，判据 = ConsentStore 开麦时间戳）：
//! - WeType 的语音热键（Ctrl+Win）**只有当 WeType 是当前会话的活动输入法时
//!   才生效**：会话切到微软拼音时注入和弦不开麦，切回 WeType 后恢复触发。
//! - Windows 按应用记忆输入法——用户在其他应用用过别的输入法后，这些应用
//!   的会话里 WeType 不活跃，语音键表现为"无法唤起"（与输入框聚焦无关：
//!   桌面/资源管理器聚焦 6/6 照常触发，p-focus-experiment.ps1）。
//! - **COM 套间陷阱（2026-09-05 MTA 探针实锤，examples\ime_probe_mta.rs）**：
//!   `ActivateProfile` 从 MTA 线程调用返回 S_OK 但**不生效**；必须从 STA
//!   线程调用才真正切换。早期实验在 PowerShell（STA）里验证通过，部署后
//!   应用从 BLE 工作线程（MTA）调用——修复形同虚设。
//! - **冷切换重绑延迟（2026-09-05 真机日志实锤，kb-live.log 10:31:40 会话）**：
//!   会话从未激活过 WeType 时，`ActivateProfile` 返回后目标应用的输入法
//!   会话重绑是异步的——紧跟的和弦落在旧会话（LWin 穿透、无 0xFC、微信
//!   无反应）；第二次起会话已是 WeType，立即触发。表现为"首次按失败、
//!   第二次起正常"。修复：先用 `GetActiveProfile` 判定当前会话状态——
//!   已是 WeType（热路径）零延迟注入；需要切换（冷路径）才激活并等待
//!   重绑窗口后再返回。
//!
//! 方案（参考 macOS 版 PreferredInputSourceMonitor 的"保证语音工具是活动
//! 输入源"职责设计）：注入和弦前，在临时 STA 线程上用公开 TSF API
//! （ITfInputProcessorProfileMgr + TF_IPPMF_FORSESSION 会话级标志——只影响
//! 当前应用会话，不改其他应用的输入法记忆）确保 WeType 为当前会话输入法。
//!
//! 护栏：激活失败或超时只记录提示并按原行为注入（绝不比现状更差）；幂等
//! ——WeType 已活跃时重复激活无害；STA 线程一次性的（创建→调用→退出，
//! 全部同线程内完成，无跨套间封送、无需消息泵），通信走 channel 有界
//! 等待（500ms）防卡 BLE 工作线程；仅在配置了按住说话快捷键的语音会话上
//! 调用。
//!
//! 2026-10-01 泛化：目标不再写死 WeType，而是按**用户选的输入工具**
//! （`AppSettings.voice_input_tool`）决定——豆包输入法同款配方（STA +
//! `TF_IPPMF_FORSESSION` + 冷路径重绑等待），GUID 由 `examples/doubao_ime_probe.rs`
//! 真机取得并读回验证。Vokie / 「其他工具」不切输入法（`NotRequired`）。

use std::sync::mpsc;
use std::time::Duration;

use sayall_core::settings::VoiceInputTool;
use windows::core::GUID;
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfInputProcessorProfileMgr, GUID_TFCAT_TIP_KEYBOARD, TF_INPUTPROCESSORPROFILE,
    TF_IPPMF_FORSESSION, TF_PROFILETYPE_INPUTPROCESSOR,
};

/// TF_InputProcessorProfiles（msctf.dll，公开 COM 类）。
const CLSID_TF_INPUT_PROCESSOR_PROFILES: GUID =
    GUID::from_u128(0x33c53a50_f456_4884_b049_85fd643e_cfed);

/// 微信输入法（WeType）的 TSF CLSID 与 Profile GUID（公开稳定标识；
/// 来自本机输入法列表 `0804:{86598FB9-…}{607FDF85-…}`，2.1.3.18 实测）。
const WETYPE_CLSID: GUID = GUID::from_u128(0x86598fb9_66a2_463e_b9c2_aeb906d477ad);
const WETYPE_PROFILE: GUID = GUID::from_u128(0x607fdf85_fcc8_4dbd_a365_41296f980c9c);
/// 豆包输入法的 TSF CLSID 与 Profile GUID（2026-10-01 探针实测：
/// `examples/doubao_ime_probe.rs`；同一把 0804 配置，registry Description=豆包输入法，
/// Enable=1；激活后 GetActiveProfile 读回即为该组合，非仅 S_OK）。
const DOUBAO_CLSID: GUID = GUID::from_u128(0x9d2b2e2b_3c93_4d2f_9d35_6eeb85f0d2b0);
const DOUBAO_PROFILE: GUID = GUID::from_u128(0x2b4d4b3a_4d4f_4c0a_8e66_7f771a2b9c10);
const LANGID_ZH_CN: u16 = 0x0804;

/// 一个语音工具对应的 TSF 会话级目标输入法配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImeProfile {
    clsid: GUID,
    guid_profile: GUID,
    /// 日志标签（不落 GUID、不落路径）。
    pub label: &'static str,
}

const WETYPE_IME: ImeProfile = ImeProfile {
    clsid: WETYPE_CLSID,
    guid_profile: WETYPE_PROFILE,
    label: "wechat",
};
const DOUBAO_IME: ImeProfile = ImeProfile {
    clsid: DOUBAO_CLSID,
    guid_profile: DOUBAO_PROFILE,
    label: "doubao",
};

/// 工具 → 需要确保为当前会话活动输入法的目标。
///
/// 只有"自身是输入法"的工具才需要切：Vokie 是桌面程序（不受输入法影响），
/// 「其他工具」由用户自备输入法，两者都返回 `None`——**绝不**替它们切输入法
/// （2026-09-29 真机实证：误切会让豆包路径被切成微信输入法）。
pub fn ime_target_for(tool: VoiceInputTool) -> Option<ImeProfile> {
    match tool {
        VoiceInputTool::Wechat => Some(WETYPE_IME),
        VoiceInputTool::Doubao => Some(DOUBAO_IME),
        VoiceInputTool::Vokie | VoiceInputTool::Other => None,
    }
}
/// STA 激活线程的有界等待：正常 <10ms，500ms 只是防卡上限。
const ACTIVATION_JOIN_TIMEOUT: Duration = Duration::from_millis(500);
/// 冷切换后的会话重绑等待：`ActivateProfile` 返回 ≠ 目标应用完成输入法
/// 重绑（2026-09-05 首按失败实证）；热路径不付此代价。
const SESSION_REBIND_SETTLE: Duration = Duration::from_millis(50);

/// 激活结果：AlreadyActive = 会话已是目标输入法（调用方可零延迟注入）；
/// Switched = 本次执行了会话切换（含重绑等待）；
/// SkippedSelfForeground = 前台是 SayAll 自身窗口，已跳过激活；
/// SkippedRecentSwitch = 距上次真实切换过近，按最小间隔规则跳过本次切换（2026-10-09）；
/// NotRequired = 该工具不需要切输入法（Vokie / 其他工具）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeActivation {
    AlreadyActive,
    Switched,
    SkippedSelfForeground,
    SkippedRecentSwitch,
    NotRequired,
}

/// 两次"真实会话切换"的最小间隔（2026-10-09 真机实证）。
///
/// 背景：Andy 复现——"微信切换 → 豆包切换"相隔约 2 秒时，Chromium 目标窗口
/// （DimAgent）进入僵死态：豆包语音连按不复位、7 秒后仍不出；而间隔 ≥5 秒的
/// 同序列（E4 手动验证）与历史 ≥4.4 秒样本全部正常。因此对**相邻真实切换**
/// 强制最小间隔：过近的切换推迟（对齐路径）或跳过（按下兜底路径），
/// 宁可损失一次按下的兜底，也不制造僵死态。
pub(crate) const MIN_SWITCH_GAP_MS: u64 = 5000;

/// 距上次切换若不足最小间隔，返回需要补足的等待毫秒数（纯函数，单测覆盖）。
pub(crate) fn switch_gap_wait_ms(age_ms: Option<u64>) -> u64 {
    match age_ms {
        Some(age) if age < MIN_SWITCH_GAP_MS => MIN_SWITCH_GAP_MS - age,
        _ => 0,
    }
}

/// 等到距上次真实切换满足最小间隔；返回实际等待的毫秒数（0 = 无需等待）。
pub(crate) fn wait_for_switch_gap() -> u64 {
    let wait = switch_gap_wait_ms(last_switch_age_ms());
    if wait > 0 {
        std::thread::sleep(std::time::Duration::from_millis(wait));
    }
    wait
}

/// 前台窗口是否属于 SayAll 自身进程。
///
/// 会话级 TSF 激活作用于焦点窗口——焦点落在自己的 WebView 时，把微信
/// 输入法切进自己的设置窗口毫无收益（听写需要目标应用的文本框），且
/// 实证会使 WebView2 整页重载（Bugs/2026-09-12：0x04 后 ime_activation
/// 失败/成功均伴随 document_load，热路径会话零重载）。调用方据此跳过。
pub(crate) fn foreground_is_self() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            return false;
        }
        let mut process_id: u32 = 0;
        GetWindowThreadProcessId(foreground, Some(&mut process_id));
        process_id != 0 && process_id == std::process::id()
    }
}

/// 前台窗口诊断探针（2026-10-09，Andy 同意加日志）：进程号 + 窗口类名**稳定哈希**。
///
/// 类名哈希用于跨日志回答"失败与成功的那几按是不是打在同一个窗口类型上"
/// （如 Chromium 类窗口 vs 记事本），不落盘明文类名；FNV-1a 64 位取高 32 位、
/// 输出 8 位十六进制，跨进程与版本稳定（类名先转小写再哈希）。
pub(crate) struct ForegroundProbe {
    pub pid: u32,
    pub class_token: String,
}

pub(crate) fn foreground_probe() -> ForegroundProbe {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
    };
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            return ForegroundProbe {
                pid: 0,
                class_token: "none".to_owned(),
            };
        }
        let mut process_id: u32 = 0;
        GetWindowThreadProcessId(foreground, Some(&mut process_id));
        let mut buffer = [0u16; 256];
        let len = GetClassNameW(foreground, &mut buffer);
        let class = if len > 0 {
            String::from_utf16_lossy(&buffer[..len as usize])
        } else {
            String::new()
        };
        ForegroundProbe {
            pid: process_id,
            class_token: if class.is_empty() {
                "unknown".to_owned()
            } else {
                class_token(&class)
            },
        }
    }
}

/// 窗口类名的稳定哈希（FNV-1a 64 → 高 32 位，8 位十六进制；输入先转小写）。
pub(crate) fn class_token(class: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in class.to_ascii_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", (hash >> 32) as u32)
}

/// 有界等待"前台不再是我们自己"的预算与步进（2026-10-09 预对齐竞态修复）。
///
/// 为什么需要：窗口失焦事件（`WindowEvent::Focused(false)`）到达时，新窗口
/// 往往还没完成激活，`GetForegroundWindow()` 仍可能指向我们自己；旧实现直接
/// 在这一瞬间判断并**一次性消费**布防，于是对齐被 `skipped_self_foreground`
/// 吞掉且不再重试（真机实证 2026-10-09：`last_switch_age_ms=never`）。
pub(crate) const PREALIGN_SETTLE_BUDGET_MS: u64 = 500;
pub(crate) const PREALIGN_SETTLE_POLL_MS: u64 = 25;

/// 有界等待的结果（进日志：一次拉取即可归因"等没等到、等了多久"）。
pub(crate) struct ForegroundSettle {
    pub elapsed_ms: u128,
    pub polls: u32,
    pub still_self: bool,
}

/// 纯判定（单测覆盖）：`samples[i]` = 第 i 次轮询时"前台是否仍是本进程"。
/// 首次出现 `false` 即视为新窗口已接管；全 `true`（含空样本）视为未让出。
#[derive(Debug, PartialEq, Eq)]
enum SettleOutcome {
    SettledAfter(u32),
    StillSelf,
}

fn settle_outcome(samples: &[bool]) -> SettleOutcome {
    match samples.iter().position(|still_self| !*still_self) {
        Some(index) => SettleOutcome::SettledAfter((index + 1) as u32),
        None => SettleOutcome::StillSelf,
    }
}

/// 有界等待：轮询 `foreground_is_self()` 到"不再是自己"或预算耗尽。
/// 判定统一走 `settle_outcome`（单一来源），时间只负责"何时放弃"。
pub(crate) fn wait_until_foreground_settles(budget_ms: u64, poll_ms: u64) -> ForegroundSettle {
    let started = std::time::Instant::now();
    let mut samples: Vec<bool> = Vec::new();
    loop {
        samples.push(foreground_is_self());
        let exhausted = started.elapsed().as_millis() as u64 >= budget_ms;
        match settle_outcome(&samples) {
            SettleOutcome::SettledAfter(polls) => {
                return ForegroundSettle {
                    elapsed_ms: started.elapsed().as_millis(),
                    polls,
                    still_self: false,
                };
            }
            SettleOutcome::StillSelf if exhausted => {
                return ForegroundSettle {
                    elapsed_ms: started.elapsed().as_millis(),
                    polls: samples.len() as u32,
                    still_self: true,
                };
            }
            SettleOutcome::StillSelf => {
                std::thread::sleep(std::time::Duration::from_millis(poll_ms));
            }
        }
    }
}

#[cfg(test)]
mod prealign_settle_tests {
    use super::{settle_outcome, SettleOutcome};

    #[test]
    fn settle_stops_at_the_first_non_self_sample() {
        assert_eq!(settle_outcome(&[false]), SettleOutcome::SettledAfter(1));
        assert_eq!(
            settle_outcome(&[true, false]),
            SettleOutcome::SettledAfter(2)
        );
        assert_eq!(
            settle_outcome(&[true, true, false]),
            SettleOutcome::SettledAfter(3)
        );
    }

    #[test]
    fn settle_reports_still_self_when_every_sample_is_self() {
        assert_eq!(settle_outcome(&[]), SettleOutcome::StillSelf);
        assert_eq!(settle_outcome(&[true]), SettleOutcome::StillSelf);
        assert_eq!(
            settle_outcome(&[true, true, true]),
            SettleOutcome::StillSelf
        );
    }
}

/// 切换触发场景（日志 `scope=`）：报障时一次日志拉取即可归因"这次切换是谁要求的"。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImeSwitchScope {
    /// 按下语音键时的兜底切换（本次按下可能赶不上，为下一次按下生效）。
    VoicePress,
    /// 用户刚在连接页选中输入工具后的一次性对齐（2026-10-03，离开自身窗口焦点后执行）。
    ToolSelect,
}

impl ImeSwitchScope {
    fn label(self) -> &'static str {
        match self {
            ImeSwitchScope::VoicePress => "voice_press",
            ImeSwitchScope::ToolSelect => "tool_select",
        }
    }
}

/// 确保 `tool` 对应的输入法是当前会话的活动输入法（幂等）。
///
/// 必须在 STA 线程上执行（MTA 调用返回 S_OK 但不生效，见模块注释）；
/// 本函数自行创建临时 STA 线程并经 channel 有界等待结果，对调用方
/// （BLE 工作线程，MTA）透明。返回 Err 时调用方记录提示后仍按原行为
/// 注入——激活失败不阻断语音。
///
/// `tool` 由"用户选的输入工具"决定（不是按和弦猜）：选豆包却把和弦配成
/// Ctrl+Win 时，旧实现会把输入法切成微信（2026-10-01 语义缺口）。
pub fn ensure_session_ime(
    tool: VoiceInputTool,
    scope: ImeSwitchScope,
) -> Result<ImeActivation, String> {
    let started = std::time::Instant::now();
    let Some(target) = ime_target_for(tool) else {
        // 功能点日志：Vokie / 其他工具明确"不切输入法"也是决策，要能从日志看出
        // 是"没轮到切"还是"切失败"（2026-09-29 真机曾把豆包路径误切成微信）。
        crate::ble::gatt_note(
            "ime_activation tool=none outcome=not_required elapsed_ms=0 reason=unsupported_tool"
                .to_owned(),
        );
        return Ok(ImeActivation::NotRequired);
    };
    // 前台是自身 WebView 时跳过激活：TSF 会话切换实证会触发 WebView2
    // 整页重载（Bugs/2026-09-12），且此时注入的和弦也落在自己窗口上，
    // 激活没有任何收益。返回 Ok——这不是错误，不应置 UI last_error。
    if foreground_is_self() {
        crate::ble::gatt_note(format!(
            "ime_activation tool={} outcome=skipped_self_foreground elapsed_ms=0 foreground_observed=true error_domain=none error_code=foreground_is_self retryable=false",
            target.label,
        ));
        return Ok(ImeActivation::SkippedSelfForeground);
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("sayall-ime-activate".to_owned())
        .spawn(move || {
            let outcome = sta_ensure_ime(target, scope);
            let _ = sender.send(outcome);
        })
        .map_err(|error| format!("创建激活线程失败：{error}"))?;
    // 有界等待，不 join：超时说明激活异常缓慢，按失败处理继续注入；
    // 线程在后台自然结束（若激活迟到，惠及下一次按键）。
    let result = match receiver.recv_timeout(ACTIVATION_JOIN_TIMEOUT) {
        Ok(result) => result,
        Err(_) => Err(format!("激活 {} 超时（500ms）", target.label)),
    };
    // 功能点日志（AGENTS.md）：决策结果 + 耗时 + 前台进程，报障时一次
    // 日志拉取即可定位是热/冷路径、查询、激活还是等待环节。
    let outcome = match &result {
        Ok(ImeActivation::AlreadyActive) => "already_active",
        Ok(ImeActivation::Switched) => "switched",
        Ok(ImeActivation::SkippedSelfForeground) => "skipped_self_foreground",
        Ok(ImeActivation::SkippedRecentSwitch) => "skipped_recent_switch",
        Ok(ImeActivation::NotRequired) => "not_required",
        Err(_) => "failed",
    };
    let probe = foreground_probe();
    crate::ble::gatt_note(format!(
        "ime_activation tool={} scope={} outcome={outcome} elapsed_ms={} last_switch_age_ms={} foreground_observed={} fg_pid={} fg_class={} error_domain={} error_code={} retryable={}",
        target.label,
        scope.label(),
        started.elapsed().as_millis(),
        age_label(last_switch_age_ms()),
        foreground_process_name().is_some(),
        probe.pid,
        probe.class_token,
        if result.is_ok() { "none" } else { "tsf" },
        if result.is_ok() { "none" } else { "activation_failed" },
        result.is_err(),
    ));
    result
}

/// TSF 配置切换唤醒（2026-09-05 方案迭代）：制造一次输入法配置变更事件
/// （切到其他输入法 → 短暂停 → 切回微信输入法），用于唤醒微信输入法
/// 休眠的热键钩子——打开其设置页能唤醒的公开 API 等效路径。
///
/// 背景：跨进程 `SetProcessInformation(ProcessPowerThrottling)` 实测返回
/// E_INVALIDARG（不支持作用于其他进程，2026-09-05 15:04 真机），原"解除
/// 微信进程节流"方案不可行；配置切换激活事件是其窗口激活之外唯一可由
/// 本应用触发的公开事件源。
///
/// 返回结果描述（用于日志）：切换用的临时输入法 CLSID 与两步激活结果。
pub fn cycle_wetype_profile() -> Result<String, String> {
    let (sender, receiver) = mpsc::channel();
    let _spawned = std::thread::Builder::new()
        .name("sayall-ime-cycle".to_owned())
        .spawn(move || {
            let outcome = sta_cycle_wetype_profile();
            let _ = sender.send(outcome);
        })
        .map_err(|error| format!("创建切换线程失败：{error}"))?;
    match receiver.recv_timeout(ACTIVATION_JOIN_TIMEOUT) {
        Ok(result) => result,
        Err(_) => Err("切换输入法配置超时（500ms）".to_owned()),
    }
}

fn sta_cycle_wetype_profile() -> Result<String, String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::TextServices::{TF_IPP_FLAG_ENABLED, TF_PROFILETYPE_INPUTPROCESSOR};
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if hr.is_err() && hr != windows::core::HRESULT(1) {
            return Err(format!("CoInitializeEx(STA) 失败：{hr:?}"));
        }
        let result = (|| {
            let manager: ITfInputProcessorProfileMgr = CoCreateInstance(
                &CLSID_TF_INPUT_PROCESSOR_PROFILES,
                None,
                CLSCTX_INPROC_SERVER,
            )
            .map_err(|error| format!("创建 TSF 配置管理器失败：{error}"))?;
            // 枚举 zh-CN 输入法，找一个非 WeType 的已启用处理器配置。
            let enumerator = manager
                .EnumProfiles(LANGID_ZH_CN)
                .map_err(|error| format!("枚举输入法配置失败：{error}"))?;
            let mut alternative: Option<(GUID, GUID)> = None;
            let mut profiles = [TF_INPUTPROCESSORPROFILE::default(); 16];
            let mut fetched: u32 = 0;
            loop {
                if enumerator.Next(&mut profiles, &mut fetched).is_err() || fetched == 0 {
                    break;
                }
                for profile in &profiles[..fetched as usize] {
                    if profile.dwProfileType == TF_PROFILETYPE_INPUTPROCESSOR
                        && profile.clsid != WETYPE_CLSID
                        && profile.dwFlags & TF_IPP_FLAG_ENABLED != 0
                    {
                        alternative = Some((profile.clsid, profile.guidProfile));
                        break;
                    }
                }
                if alternative.is_some() || (fetched as usize) < profiles.len() {
                    break;
                }
            }
            let Some((alt_clsid, alt_profile)) = alternative else {
                return Err("无可用作切换的其他输入法配置".to_owned());
            };
            // 第一步：切到临时输入法（制造配置变更事件）。
            manager
                .ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_ZH_CN,
                    &alt_clsid,
                    &alt_profile,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                )
                .map_err(|error| format!("切换到临时输入法失败：{error}"))?;
            std::thread::sleep(SESSION_REBIND_SETTLE);
            // 第二步：切回微信输入法。
            manager
                .ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_ZH_CN,
                    &WETYPE_CLSID,
                    &WETYPE_PROFILE,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                )
                .map_err(|error| format!("切回微信输入法失败：{error}"))?;
            Ok(format!(
                "switched via clsid={:08X} and back to WeType",
                alt_clsid.data1
            ))
        })();
        CoUninitialize();
        result
    }
}

/// 前台进程名（诊断用，不含窗口标题/路径等隐私信息）。
fn foreground_process_name() -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.0.is_null() {
            return None;
        }
        let mut process_id: u32 = 0;
        GetWindowThreadProcessId(foreground, Some(&mut process_id));
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?;
        let mut buffer = [0u16; 512];
        let mut size = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
        .is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if !queried {
            return None;
        }
        let full = String::from_utf16_lossy(&buffer[..size as usize]);
        Some(
            full.rsplit(['\\', '/'])
                .next()
                .unwrap_or("unknown")
                .to_owned(),
        )
    }
}

type AgeSlot = std::sync::OnceLock<std::sync::Mutex<Option<std::time::Instant>>>;

fn note_now(slot: &AgeSlot) {
    let slot = slot.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(std::time::Instant::now());
}

fn age_of(slot: &AgeSlot) -> Option<u64> {
    let slot = slot.get_or_init(|| std::sync::Mutex::new(None));
    let guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.map(|instant| instant.elapsed().as_millis() as u64)
}

/// 年龄字段统一格式（None → `never`）：`voice_attempt` / `ime_activation` 共用。
pub(crate) fn age_label(age: Option<u64>) -> String {
    age.map(|value| value.to_string())
        .unwrap_or_else(|| "never".to_owned())
}

/// 最近一次真正执行了会话切换（Switched）的时刻：用于把"首次按下没反应"
/// 与"刚刚切过输入法"关联起来（2026-10-01：Andy 实测记事本里第一次按下
/// 右 Alt 漏进记事本变成菜单助记符，说明目标应用的输入法会话还没接上）。
static LAST_SWITCH_AT: AgeSlot = std::sync::OnceLock::new();

/// 最近一次「用户选中输入工具」（`set_voice_input_tool` 被调用）的时刻（2026-10-09）。
///
/// 为什么记：Andy 复现的失败样本全部来自"切完工具、快速切到目标窗口"——按下时
/// 距这次选择的毫秒数与路径/门禁一起落 `voice_attempt`，快慢窗口可直接量化，
/// 不再依赖人工回忆"大概隔了几秒"。
static LAST_TOOL_SELECT_AT: AgeSlot = std::sync::OnceLock::new();

/// 最近一次「按住说话快捷键变更」（报告层合成目标随之下发）的时刻（2026-10-09）。
static LAST_HOTKEY_CHANGE_AT: AgeSlot = std::sync::OnceLock::new();

fn note_switch_happened() {
    note_now(&LAST_SWITCH_AT);
}

/// 记录一次「选中输入工具」（任何一次 set 调用都算，含应用启动时的配置装载）。
pub(crate) fn note_tool_select() {
    note_now(&LAST_TOOL_SELECT_AT);
}

/// 记录一次「按住说话快捷键变更」（任何一次 set 调用都算，含清空）。
pub(crate) fn note_hotkey_change() {
    note_now(&LAST_HOTKEY_CHANGE_AT);
}

/// 距离最近一次会话切换过去了多少毫秒；从未切过返回 None。
pub fn last_switch_age_ms() -> Option<u64> {
    age_of(&LAST_SWITCH_AT)
}

/// 距离最近一次「选中输入工具」过去了多少毫秒；从未选过返回 None。
pub(crate) fn tool_select_age_ms() -> Option<u64> {
    age_of(&LAST_TOOL_SELECT_AT)
}

/// 距离最近一次「按住说话快捷键变更」过去了多少毫秒；从未变过返回 None。
pub(crate) fn hotkey_change_age_ms() -> Option<u64> {
    age_of(&LAST_HOTKEY_CHANGE_AT)
}

/// 工具标签（日志用；与 `ime_target_for` 的 label 语义一致）。
pub(crate) fn tool_label(tool: Option<VoiceInputTool>) -> &'static str {
    match tool {
        Some(VoiceInputTool::Wechat) => "wechat",
        Some(VoiceInputTool::Doubao) => "doubao",
        Some(VoiceInputTool::Vokie) => "vokie",
        Some(VoiceInputTool::Other) => "other",
        None => "none",
    }
}

/// 该工具"是否真的开麦"的观测标记（ConsentStore 条目名匹配片段，2026-10-09）。
///
/// - 豆包：本机实测条目 `ImeService.exe` 的路径含 `doubao`，录音启停会更新
///   其 LastUsedTimeStart/Stop（与微信同一公开观测通道）；
/// - 微信：沿用既有 `wetype` 片段（`wetype_check` 阶梯已用同一观测做恢复裁决，
///   这里只复用观测本身，不改其恢复逻辑）；
/// - Vokie：`Vokie.exe`；
/// - 其他/未选：没有可依赖的条目，返回 None（判据按 `unknown` 处理，不推断）。
pub(crate) fn mic_marker_for(tool: Option<VoiceInputTool>) -> Option<&'static str> {
    match tool {
        Some(VoiceInputTool::Doubao) => Some("doubao"),
        Some(VoiceInputTool::Wechat) => Some("wetype"),
        Some(VoiceInputTool::Vokie) => Some("vokie"),
        _ => None,
    }
}

#[cfg(test)]
mod press_context_tests {
    use super::*;

    #[test]
    fn age_label_formats_none_as_never() {
        assert_eq!(age_label(None), "never");
        assert_eq!(age_label(Some(0)), "0");
        assert_eq!(age_label(Some(1234)), "1234");
    }

    #[test]
    fn mic_markers_cover_shipped_tools_only() {
        assert_eq!(mic_marker_for(Some(VoiceInputTool::Doubao)), Some("doubao"));
        assert_eq!(mic_marker_for(Some(VoiceInputTool::Wechat)), Some("wetype"));
        assert_eq!(mic_marker_for(Some(VoiceInputTool::Vokie)), Some("vokie"));
        assert_eq!(mic_marker_for(Some(VoiceInputTool::Other)), None);
        assert_eq!(mic_marker_for(None), None);
    }

    #[test]
    fn class_token_is_stable_and_case_insensitive() {
        let notepad = class_token("Notepad");
        assert_eq!(notepad.len(), 8);
        assert!(notepad.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(notepad, class_token("notepad"));
        assert_eq!(notepad, class_token("Notepad"));
        assert_ne!(notepad, class_token("Chrome_WidgetWin_1"));
    }

    #[test]
    fn switch_gap_wait_respects_minimum_spacing() {
        assert_eq!(switch_gap_wait_ms(None), 0);
        assert_eq!(switch_gap_wait_ms(Some(0)), MIN_SWITCH_GAP_MS);
        assert_eq!(switch_gap_wait_ms(Some(2000)), MIN_SWITCH_GAP_MS - 2000);
        assert_eq!(switch_gap_wait_ms(Some(MIN_SWITCH_GAP_MS)), 0);
        assert_eq!(switch_gap_wait_ms(Some(9000)), 0);
    }

    #[test]
    fn tool_labels_are_stable_log_tokens() {
        assert_eq!(tool_label(Some(VoiceInputTool::Wechat)), "wechat");
        assert_eq!(tool_label(Some(VoiceInputTool::Doubao)), "doubao");
        assert_eq!(tool_label(Some(VoiceInputTool::Vokie)), "vokie");
        assert_eq!(tool_label(Some(VoiceInputTool::Other)), "other");
        assert_eq!(tool_label(None), "none");
    }
}

/// 临时 STA 线程体：CoInitializeEx(STA) → 查询活动输入法 →
/// （需要时）ActivateProfile + 重绑等待 → CoUninitialize。
/// 全部调用在本线程内完成（无跨套间封送，无需消息泵）。
fn sta_ensure_ime(target: ImeProfile, scope: ImeSwitchScope) -> Result<ImeActivation, String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // S_OK(0) 或 S_FALSE(1)（本线程已初始化）都算成功。
        if hr.is_err() && hr != windows::core::HRESULT(1) {
            return Err(format!("CoInitializeEx(STA) 失败：{hr:?}"));
        }
        let result = (|| {
            let manager: ITfInputProcessorProfileMgr = CoCreateInstance(
                &CLSID_TF_INPUT_PROCESSOR_PROFILES,
                None,
                CLSCTX_INPROC_SERVER,
            )
            .map_err(|error| format!("创建 TSF 配置管理器失败：{error}"))?;
            let mut profile = TF_INPUTPROCESSORPROFILE::default();
            let query = manager.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile);
            let active_is_target = match &query {
                Ok(()) => {
                    profile.clsid == target.clsid && profile.guidProfile == target.guid_profile
                }
                Err(_) => false,
            };
            // 功能点日志：只记录冷/热判定，不落盘输入法 GUID 或前台应用身份。
            crate::ble::gatt_note(format!(
                "ime_query tool={} ok={} active_is_target={active_is_target} active_profile_present={}",
                target.label,
                query.is_ok(),
                profile.clsid != GUID::from_u128(0),
            ));
            if active_is_target {
                return Ok(ImeActivation::AlreadyActive);
            }
            // 最小切换间隔（2026-10-09）：按下兜底路径遇"距上次真实切换过近"直接
            // 跳过本次切换——过近的第二次真实切换是目标窗口僵死态的诱因；补切由
            // 对齐路径按间隔完成（见 lib.rs 对齐线程的 wait_for_switch_gap）。
            if scope == ImeSwitchScope::VoicePress {
                if let Some(age) = last_switch_age_ms() {
                    if age < MIN_SWITCH_GAP_MS {
                        return Ok(ImeActivation::SkippedRecentSwitch);
                    }
                }
            }
            manager
                .ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_ZH_CN,
                    &target.clsid,
                    &target.guid_profile,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                )
                .map_err(|error| format!("激活 {} 会话失败：{error}", target.label))?;
            // 冷切换：等目标应用完成输入法会话重绑再放行注入。
            std::thread::sleep(SESSION_REBIND_SETTLE);
            note_switch_happened();
            // 切换后自校验（2026-10-03，Andy：日志定位不了就加日志）：`ActivateProfile`
            // 返回成功不等于会话真的切过去（2026-09-05 曾实证 S_OK 但不生效）。这里读回
            // 活动配置；不一致按有界重试一次，两次结果都落日志——"切成没成"一眼可判，
            // 也保证"后续按下必能拉起"不会因一次静默失败而失效。
            // `stuck` 语义：true = 读回仍不是目标（失败方向），false = 已切到目标。
            let mut verified = query_active_is_target(&manager, target);
            let mut attempts = 1u32;
            if !verified {
                crate::ble::gatt_note(format!(
                    "ime_verify scope={} attempt=1 stuck=true",
                    scope.label(),
                ));
                let _ = manager.ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_ZH_CN,
                    &target.clsid,
                    &target.guid_profile,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                );
                std::thread::sleep(SESSION_REBIND_SETTLE);
                note_switch_happened();
                verified = query_active_is_target(&manager, target);
                attempts = 2;
            }
            crate::ble::gatt_note(format!(
                "ime_verify scope={} attempt={attempts} stuck={}",
                scope.label(),
                !verified,
            ));
            if !verified {
                return Err(format!(
                    "{} 切换后读回仍不是当前活动输入法（attempts={attempts}）",
                    target.label
                ));
            }
            // 2026-10-09 诊断（Andy：失败样本全部来自"切完工具快速切到窗口"）：
            // 切换当下读回成功 ≠ 之后一直有效。追加有界读回探针记录保持情况。
            spawn_readback_probes(target, scope);
            Ok(ImeActivation::Switched)
        })();
        CoUninitialize();
        result
    }
}

/// 读回当前会话的活动配置是否为目标（切换后自校验用；语义与 `sta_ensure_ime` 的查询一致）。
fn query_active_is_target(manager: &ITfInputProcessorProfileMgr, target: ImeProfile) -> bool {
    let mut profile = TF_INPUTPROCESSORPROFILE::default();
    match unsafe { manager.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile) } {
        Ok(()) => profile.clsid == target.clsid && profile.guidProfile == target.guid_profile,
        Err(_) => false,
    }
}

/// 切换成功后的读回探针延迟（毫秒，自切换返回起累计）。2026-10-09 诊断新增。
const READBACK_PROBE_DELAYS_MS: [u64; 3] = [500, 1500, 3000];

/// 并发探针上限：每次切换最多 2 组，超出直接留痕跳过（防连续按下堆线程）。
static READBACK_PROBES_IN_FLIGHT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// 切换成功后追加读回探针：验证"切过去"是否**保持**——静默回退、目标应用侧
/// 重绑输入法都会在这里现形，也给"多久才真正接上"提供时间线。
/// 只读、有界退出、不写任何状态；并发上限 2。
fn spawn_readback_probes(target: ImeProfile, scope: ImeSwitchScope) {
    use std::sync::atomic::Ordering;
    if READBACK_PROBES_IN_FLIGHT.load(Ordering::Relaxed) >= 2 {
        crate::ble::gatt_note(format!("ime_readback scope={} skipped=busy", scope.label()));
        return;
    }
    READBACK_PROBES_IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
    let spawned = std::thread::Builder::new()
        .name("sayall-ime-readback".to_owned())
        .spawn(move || {
            let started = std::time::Instant::now();
            for delay in READBACK_PROBE_DELAYS_MS {
                let elapsed = started.elapsed().as_millis() as u64;
                if elapsed < delay {
                    std::thread::sleep(std::time::Duration::from_millis(delay - elapsed));
                }
                let active = probe_active_is_target(target);
                crate::ble::gatt_note(format!(
                    "ime_readback scope={} followup_ms={} active_is_target={}",
                    scope.label(),
                    started.elapsed().as_millis(),
                    active
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_owned()),
                ));
            }
            READBACK_PROBES_IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
        });
    if spawned.is_err() {
        READBACK_PROBES_IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
    }
}

/// 独立线程（自带 STA + 管理器）读回活动输入法是否为目标；初始化失败返回 None。
fn probe_active_is_target(target: ImeProfile) -> Option<bool> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // S_OK(0) 或 S_FALSE(1)（本线程已初始化）都算成功。
        if hr.is_err() && hr != windows::core::HRESULT(1) {
            return None;
        }
        let result = (|| {
            let manager: ITfInputProcessorProfileMgr = CoCreateInstance(
                &CLSID_TF_INPUT_PROCESSOR_PROFILES,
                None,
                CLSCTX_INPROC_SERVER,
            )
            .ok()?;
            Some(query_active_is_target(&manager, target))
        })();
        CoUninitialize();
        result
    }
}

// ─── 录入期"输入法让位"（路线①，2026-09-27）───────────────────────────────
//
// 背景：LL 键盘钩子链为 FIFO（最早安装最先调用，见
// docs/investigations/2026-09-27-ll-hook-chain-order-fifo.md）。微信输入法随
// 登录先于本应用安装钩子，其语音和弦（= 用户在「按住说话快捷键」中配置的组合）
// 的物理边沿在到达本应用录入钩子之前就被它吞掉，表现为"已配置的组合录不进去"。
//
// 本模块头（2026-09-05 实验）已实锤：输入法的语音热键**只有当它是当前会话的
// 活动输入法时才生效**。因此录入期间把录入窗口所在线程的输入区域临时切到
// 非 IME 布局，其和弦判定即失效，物理边沿直达本应用钩子；录入结束恢复。
// 布局按线程生效且只作用于本应用窗口，不影响其他应用的输入法状态。
//
// 已知风险（需真机确认）：TSF/输入区域变更在本仓库曾出现"前台是自身 WebView
// 时整页重载"（Bugs/2026-09-12）；本实现用 ActivateKeyboardLayout（非 TSF 配置
// 切换），是否触发重载待实测。

use std::sync::atomic::{AtomicUsize, Ordering};
use windows::Win32::UI::Input::Ime::ImmIsIME;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ActivateKeyboardLayout, GetKeyboardLayoutList, LoadKeyboardLayoutW,
    ACTIVATE_KEYBOARD_LAYOUT_FLAGS, KLF_ACTIVATE,
};

/// 让位计划（纯逻辑，供单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YieldPlan {
    /// 当前布局不是 IME（或未知）——无需让位。
    NotNeeded,
    /// 切到该非 IME 布局。
    SwitchTo(usize),
    /// 系统里没有非 IME 布局，且加载 en-US 也失败。
    NoCandidate,
}

/// 选择录入期让位方案：当前布局是 IME 时，从候选里挑第一个非 IME 布局。
pub fn plan_input_method_yield(
    current: usize,
    candidates: &[usize],
    is_ime: impl Fn(usize) -> bool,
) -> YieldPlan {
    if current == 0 || !is_ime(current) {
        return YieldPlan::NotNeeded;
    }
    match candidates
        .iter()
        .copied()
        .find(|hkl| *hkl != current && !is_ime(*hkl))
    {
        Some(hkl) => YieldPlan::SwitchTo(hkl),
        None => YieldPlan::NoCandidate,
    }
}

/// 录入前的原始布局（0 = 无让位进行中）。
static CAPTURE_ORIGINAL_LAYOUT: AtomicUsize = AtomicUsize::new(0);

fn hkl_to_usize(hkl: HKL) -> usize {
    hkl.0 as usize
}

/// 录入开始时调用。
///
/// **必须在录入窗口所在线程上执行**（输入区域按线程生效；应用里即主线程）。
/// 返回结构化日志片段（调用方写入诊断日志）。让位失败不阻断录入——只是该
/// 组合可能仍被输入法吞掉，日志里可归因。
pub fn suspend_input_method_for_capture() -> String {
    unsafe {
        let current = current_thread_layout();
        let mut list = [HKL::default(); 32];
        let count = GetKeyboardLayoutList(Some(&mut list));
        let candidates: Vec<usize> = list[..(count.max(0) as usize)]
            .iter()
            .map(|hkl| hkl_to_usize(*hkl))
            .collect();
        let plan = plan_input_method_yield(current, &candidates, |hkl| {
            ImmIsIME(HKL(hkl as *mut core::ffi::c_void)).as_bool()
        });
        match plan {
            YieldPlan::NotNeeded => {
                return format!(
                    "capture_ime_yield outcome=not_needed current_hkl=0x{:04X}",
                    current & 0xFFFF
                );
            }
            YieldPlan::NoCandidate => {
                // 兜底：加载 en-US（标准布局，系统随时可加载）。
                match LoadKeyboardLayoutW(windows::core::w!("00000409"), KLF_ACTIVATE) {
                    Ok(hkl) => apply_yield(hkl_to_usize(hkl), current),
                    Err(_) => "capture_ime_yield outcome=no_candidate".to_owned(),
                }
            }
            YieldPlan::SwitchTo(target) => apply_yield(target, current),
        }
    }
}

fn apply_yield(target: usize, original: usize) -> String {
    unsafe {
        let activated = ActivateKeyboardLayout(
            HKL(target as *mut core::ffi::c_void),
            ACTIVATE_KEYBOARD_LAYOUT_FLAGS(0),
        );
        let observed = current_thread_layout();
        CAPTURE_ORIGINAL_LAYOUT.store(original, Ordering::Relaxed);
        format!(
            "capture_ime_yield outcome={} activated={} original_hkl=0x{:04X} target_hkl=0x{:04X} observed_hkl=0x{:04X} switched={}",
            if observed == target { "switched" } else { "not_switched" },
            activated.is_ok(),
            original & 0xFFFF,
            target & 0xFFFF,
            observed & 0xFFFF,
            observed == target
        )
    }
}

/// 录入结束时调用（同样必须在录入窗口所在线程上执行）：恢复原布局。
pub fn restore_input_method_after_capture() -> String {
    let original = CAPTURE_ORIGINAL_LAYOUT.swap(0, Ordering::Relaxed);
    if original == 0 {
        return "capture_ime_restore outcome=not_needed".to_owned();
    }
    unsafe {
        let restored = ActivateKeyboardLayout(
            HKL(original as *mut core::ffi::c_void),
            ACTIVATE_KEYBOARD_LAYOUT_FLAGS(0),
        );
        let observed = current_thread_layout();
        format!(
            "capture_ime_restore outcome={} restored={} target_hkl=0x{:04X} observed_hkl=0x{:04X}",
            if observed == original {
                "restored"
            } else {
                "not_restored"
            },
            restored.is_ok(),
            original & 0xFFFF,
            observed & 0xFFFF
        )
    }
}

/// 调用线程当前的输入区域布局（0 = 线程尚无输入上下文）。
fn current_thread_layout() -> usize {
    unsafe { hkl_to_usize(windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout(0)) }
}

#[cfg(test)]
mod yield_plan_tests {
    use super::{plan_input_method_yield, YieldPlan};

    #[test]
    fn yields_only_when_current_layout_is_an_ime() {
        let is_ime = |hkl: usize| hkl == 0x0804;
        // 当前是 IME（0x0804），候选里有非 IME（0x0409）→ 切换。
        assert_eq!(
            plan_input_method_yield(0x0804, &[0x0804, 0x0409], is_ime),
            YieldPlan::SwitchTo(0x0409)
        );
        // 当前不是 IME → 不动（不影响普通键盘布局）。
        assert_eq!(
            plan_input_method_yield(0x0409, &[0x0804, 0x0409], is_ime),
            YieldPlan::NotNeeded
        );
        // 无线索（0）→ 不动。
        assert_eq!(
            plan_input_method_yield(0, &[0x0804], is_ime),
            YieldPlan::NotNeeded
        );
        // 全是 IME → 交由调用方兜底加载 en-US。
        let both_are_ime = |hkl: usize| hkl == 0x0804 || hkl == 0x0411;
        assert_eq!(
            plan_input_method_yield(0x0804, &[0x0804, 0x0411], both_are_ime),
            YieldPlan::NoCandidate
        );
    }
}

#[cfg(test)]
mod ime_target_tests {
    use super::{ime_target_for, DOUBAO_CLSID, DOUBAO_PROFILE, WETYPE_CLSID, WETYPE_PROFILE};
    use sayall_core::settings::VoiceInputTool;

    /// 工具 → 目标输入法映射：输入法类工具要有目标，桌面程序/其他工具必须没有
    /// （给 Vokie 切输入法会破坏它，且 2026-09-29 真机回归过"豆包路径被切成微信"）。
    #[test]
    fn maps_only_input_method_tools() {
        let wechat = ime_target_for(VoiceInputTool::Wechat).expect("wechat target");
        assert_eq!(wechat.label, "wechat");
        let doubao = ime_target_for(VoiceInputTool::Doubao).expect("doubao target");
        assert_eq!(doubao.label, "doubao");
        assert!(ime_target_for(VoiceInputTool::Vokie).is_none());
        assert!(ime_target_for(VoiceInputTool::Other).is_none());
        // 身份必须是两套不同的 GUID（复制粘贴写错会在这里失败）。
        assert_ne!(WETYPE_CLSID, DOUBAO_CLSID);
        assert_ne!(WETYPE_PROFILE, DOUBAO_PROFILE);
        assert_ne!(
            ime_target_for(VoiceInputTool::Wechat),
            ime_target_for(VoiceInputTool::Doubao)
        );
    }

    /// Vokie（桌面程序）与「其他工具」绝不能被切输入法：整条链路必须原样返回
    /// NotRequired，不碰 TSF（2026-09-29 真机回归过"豆包路径被切成微信"）。
    #[test]
    fn vokie_and_other_do_not_touch_input_methods() {
        for tool in [VoiceInputTool::Vokie, VoiceInputTool::Other] {
            assert_eq!(
                super::ensure_session_ime(tool, super::ImeSwitchScope::VoicePress),
                Ok(super::ImeActivation::NotRequired)
            );
        }
    }

    /// 切换场景标签稳定（日志消费方按它归因；改动会引起日志口径变化）。
    #[test]
    fn ime_switch_scope_labels_are_stable() {
        assert_eq!(super::ImeSwitchScope::VoicePress.label(), "voice_press");
        assert_eq!(super::ImeSwitchScope::ToolSelect.label(), "tool_select");
    }
}
