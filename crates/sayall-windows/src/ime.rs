//! 第三方输入法（微信 WeType / 豆包）会话级激活。
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
//! ——目标已活跃时重复激活无害；STA 线程一次性的（创建→调用→退出，
//! 全部同线程内完成，无跨套间封送、无需消息泵），通信走 channel 有界
//! 等待（500ms）防卡 BLE 工作线程；仅在配置了按住说话快捷键的语音会话上
//! 调用。
//!
//! 多目标（2026-09-17）：本模块原先只服务微信输入法，现按
//! [`crate::voice_target::VoiceTarget`] 分流。两家的机制完全相同（同一 TSF
//! API、同一 FORSESSION 标志、同一冷/热路径判定），差异仅在 CLSID/Profile
//! GUID 与日志标签——因此共用一个 STA 骨架（[`sta_ensure_target`]），避免
//! 复制一份几乎相同的实现。**微信路径行为与引入多目标前逐字等价。**

use std::sync::mpsc;
use std::time::Duration;

use windows::core::GUID;
use windows::Win32::UI::Input::KeyboardAndMouse::HKL;
use windows::Win32::UI::TextServices::{
    ITfInputProcessorProfileMgr, GUID_TFCAT_TIP_KEYBOARD, TF_INPUTPROCESSORPROFILE,
    TF_IPPMF_FORSESSION, TF_PROFILETYPE_INPUTPROCESSOR,
};

use crate::voice_target::VoiceTarget;

/// TF_InputProcessorProfiles（msctf.dll，公开 COM 类）。
const CLSID_TF_INPUT_PROCESSOR_PROFILES: GUID =
    GUID::from_u128(0x33c53a50_f456_4884_b049_85fd643e_cfed);

/// 微信输入法（WeType）的 TSF CLSID 与 Profile GUID（公开稳定标识；
/// 来自本机输入法列表 `0804:{86598FB9-…}{607FDF85-…}`，2.1.3.18 实测）。
const WETYPE_CLSID: GUID = GUID::from_u128(0x86598fb9_66a2_463e_b9c2_aeb906d477ad);
const WETYPE_PROFILE: GUID = GUID::from_u128(0x607fdf85_fcc8_4dbd_a365_41296f980c9c);

/// 豆包输入法的 TSF CLSID 与 Profile GUID（公开稳定标识）。
///
/// 来源（本机 2026-09-17 只读读取，非第三方私有配置）：
/// - CLSID 来自 `HKLM\SOFTWARE\Microsoft\CTF\TIP` 枚举 +
///   `HKCR\CLSID\{…}\InprocServer32` = `C:\Windows\System32\tsf-oime.dll`
///   （值为 `{9D2B2E2B-3C93-4D2F-9D35-6EEB85F0D2B0}`，注册表默认值
///   "豆包输入法 Text Service"）。
/// - Profile GUID 来自同一 CLSID 下
///   `LanguageProfile\0x00000804\{2B4D4B3A-4D4F-4C0A-8E66-7F771A2B9C10}`，
///   `Description = 豆包输入法`、`Enable = 1`。
/// 这两项是 TSF 向系统公开的输入法注册身份，等价于 `ime list` 可见信息，
/// 不属于 AGENTS.md 禁止的私有配置/内部数据库/内存。
const DOUBAO_CLSID: GUID = GUID::from_u128(0x9d2b2e2b_3c93_4d2f_9d35_6eeb85f0d2b0);
const DOUBAO_PROFILE: GUID = GUID::from_u128(0x2b4d4b3a_4d4f_4c0a_8e66_7f771a2b9c10);

const LANGID_ZH_CN: u16 = 0x0804;
/// STA 激活线程的有界等待：正常 <10ms，500ms 只是防卡上限。
const ACTIVATION_JOIN_TIMEOUT: Duration = Duration::from_millis(500);
/// 冷切换后的会话重绑等待：`ActivateProfile` 返回 ≠ 目标应用完成输入法
/// 重绑（2026-09-05 首按失败实证）；热路径不付此代价。
const SESSION_REBIND_SETTLE: Duration = Duration::from_millis(50);

/// 目标的 TSF 身份（CLSID + Profile GUID）。`None` = 该目标不做会话激活。
fn tsf_identity(target: VoiceTarget) -> Option<(GUID, GUID)> {
    match target {
        VoiceTarget::WeType => Some((WETYPE_CLSID, WETYPE_PROFILE)),
        VoiceTarget::Doubao => Some((DOUBAO_CLSID, DOUBAO_PROFILE)),
        VoiceTarget::Custom => None,
    }
}

/// 激活结果：AlreadyActive = 会话已是目标输入法（调用方可零延迟注入）；
/// Switched = 本次执行了会话切换（含重绑等待）；
/// SkippedSelfForeground = 前台是 SayAll 自身窗口，已跳过激活。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceTargetActivation {
    AlreadyActive,
    Switched,
    SkippedSelfForeground,
}

/// 微信输入法激活结果（历史名，保持既有调用点与测试不变）。
#[allow(dead_code)]
pub type WeTypeActivation = VoiceTargetActivation;

/// 前台窗口是否属于 SayAll 自身进程。
///
/// 会话级 TSF 激活作用于焦点窗口——焦点落在自己的 WebView 时，把目标
/// 输入法切进自己的设置窗口毫无收益（听写需要目标应用的文本框），且
/// 实证会使 WebView2 整页重载（Bugs/2026-09-12：0x04 后 ime_activation
/// 失败/成功均伴随 document_load，热路径会话零重载）。调用方据此跳过。
fn foreground_is_self() -> bool {
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

/// 确保指定输入法是当前会话的活动输入法（幂等）。
///
/// 必须在 STA 线程上执行（MTA 调用返回 S_OK 但不生效，见模块注释）；
/// 本函数自行创建临时 STA 线程并经 channel 有界等待结果，对调用方
/// （BLE 工作线程，MTA）透明。返回 Err 时调用方记录提示后仍按原行为
/// 注入——激活失败不阻断语音。
///
/// 目标无 TSF 身份（[`VoiceTarget::Custom`]）时视为"无需激活"：直接按
/// 已激活返回（`AlreadyActive`），调用方随即零延迟注入。自定义目标可能是
/// 会议软件等非输入法工具，强行切输入法只会造成副作用。
pub fn activate_target_session(target: VoiceTarget) -> Result<VoiceTargetActivation, String> {
    let started = std::time::Instant::now();
    let target_label = target.as_log_str();
    // Custom 无 TSF 身份：无输入法可激活（用户自选的可能是会议软件等）。
    if tsf_identity(target).is_none() {
        crate::ble::gatt_note(format!(
            "ime_activation target={target_label} outcome=skipped_no_tsf_identity elapsed_ms=0 error_domain=none error_code=no_tsf_identity retryable=false"
        ));
        return Ok(VoiceTargetActivation::AlreadyActive);
    }
    // 前台是自身 WebView 时跳过激活：TSF 会话切换实证会触发 WebView2
    // 整页重载（Bugs/2026-09-12），且此时注入的和弦也落在自己窗口上，
    // 激活没有任何收益。返回 Ok——这不是错误，不应置 UI last_error。
    if foreground_is_self() {
        crate::ble::gatt_note(format!(
            "ime_activation target={target_label} outcome=skipped_self_foreground elapsed_ms=0 foreground_observed=true error_domain=none error_code=foreground_is_self retryable=false"
        ));
        return Ok(VoiceTargetActivation::SkippedSelfForeground);
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("sayall-ime-activate".to_owned())
        .spawn(move || {
            let outcome = sta_ensure_target(target);
            let _ = sender.send(outcome);
        })
        .map_err(|error| format!("创建激活线程失败：{error}"))?;
    // 有界等待，不 join：超时说明激活异常缓慢，按失败处理继续注入；
    // 线程在后台自然结束（若激活迟到，惠及下一次按键）。
    let result = match receiver.recv_timeout(ACTIVATION_JOIN_TIMEOUT) {
        Ok(result) => result,
        Err(_) => Err(format!("激活{}超时（500ms）", target.display_name())),
    };
    // 功能点日志（AGENTS.md）：决策结果 + 耗时 + 前台进程，报障时一次
    // 日志拉取即可定位是热/冷路径、查询、激活还是等待环节。
    let outcome = match &result {
        Ok(VoiceTargetActivation::AlreadyActive) => "already_active",
        Ok(VoiceTargetActivation::Switched) => "switched",
        Ok(VoiceTargetActivation::SkippedSelfForeground) => "skipped_self_foreground",
        Err(_) => "failed",
    };
    crate::ble::gatt_note(format!(
        "ime_activation target={target_label} outcome={outcome} elapsed_ms={} foreground_observed={} error_domain={} error_code={} retryable={}",
        started.elapsed().as_millis(),
        foreground_process_name().is_some(),
        if result.is_ok() { "none" } else { "tsf" },
        if result.is_ok() { "none" } else { "activation_failed" },
        result.is_err(),
    ));
    result
}

/// 确保微信输入法是当前会话的活动输入法（幂等）。
///
/// 保留原名作为薄封装：既有调用点与测试无需改动，且微信路径行为不变。
pub fn activate_wetype_session() -> Result<VoiceTargetActivation, String> {
    activate_target_session(VoiceTarget::WeType)
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
    let spawned = std::thread::Builder::new()
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

/// 临时 STA 线程体：CoInitializeEx(STA) → 查询活动输入法 →
/// （需要时）ActivateProfile + 重绑等待 → CoUninitialize。
/// 全部调用在本线程内完成（无跨套间封送，无需消息泵）。
///
/// 微信与豆包共用本骨架：机制相同（同一 TSF API、同一 FORSESSION 标志、
/// 同一冷/热路径判定），差异只有 CLSID/Profile GUID 与日志标签。
fn sta_ensure_target(target: VoiceTarget) -> Result<VoiceTargetActivation, String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    let Some((target_clsid, target_profile)) = tsf_identity(target) else {
        // 无 TSF 身份：无输入法可激活（调用方通常已在入口过滤）。
        return Ok(VoiceTargetActivation::AlreadyActive);
    };
    let target_label = target.as_log_str();
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
                Ok(()) => profile.clsid == target_clsid && profile.guidProfile == target_profile,
                Err(_) => false,
            };
            // 功能点日志：只记录冷/热判定，不落盘输入法 GUID 或前台应用身份。
            // 保留 `active_is_wetype` 字段名以兼容既有日志分析脚本，新增
            // `target` 标识实际目标。
            crate::ble::gatt_note(format!(
                "ime_query target={target_label} ok={} active_is_wetype={active_is_target} active_profile_present={}",
                query.is_ok(),
                profile.clsid != GUID::from_u128(0),
            ));
            if active_is_target {
                return Ok(VoiceTargetActivation::AlreadyActive);
            }
            manager
                .ActivateProfile(
                    TF_PROFILETYPE_INPUTPROCESSOR,
                    LANGID_ZH_CN,
                    &target_clsid,
                    &target_profile,
                    HKL::default(),
                    TF_IPPMF_FORSESSION,
                )
                .map_err(|error| format!("激活{}会话失败：{error}", target.display_name()))?;
            // 冷切换：等目标应用完成输入法会话重绑再放行注入。
            std::thread::sleep(SESSION_REBIND_SETTLE);
            Ok(VoiceTargetActivation::Switched)
        })();
        CoUninitialize();
        result
    }
}

/// 微信输入法的 STA 线程体（历史名保留，供既有测试/调用点使用）。
#[allow(dead_code)]
fn sta_ensure_wetype() -> Result<VoiceTargetActivation, String> {
    sta_ensure_target(VoiceTarget::WeType)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_activatable_target_has_distinct_tsf_identity() {
        let wetype = tsf_identity(VoiceTarget::WeType).expect("WeType must have TSF identity");
        let doubao = tsf_identity(VoiceTarget::Doubao).expect("Doubao must have TSF identity");
        assert_ne!(wetype.0, doubao.0, "CLSID must differ");
        assert_ne!(wetype.1, doubao.1, "Profile GUID must differ");
        // 回归护栏：微信的 TSF 身份在引入多目标前后不得改变。
        assert_eq!(wetype.0, WETYPE_CLSID);
        assert_eq!(wetype.1, WETYPE_PROFILE);
    }

    #[test]
    fn custom_target_has_no_tsf_identity() {
        assert!(tsf_identity(VoiceTarget::Custom).is_none());
    }

    #[test]
    fn doubao_identity_matches_registered_profile() {
        // 与 HKLM CTF/TIP 注册表读取值一致（2026-09-17 本机核对）。
        assert_eq!(
            DOUBAO_CLSID,
            GUID::from_u128(0x9d2b2e2b_3c93_4d2f_9d35_6eeb85f0d2b0)
        );
        assert_eq!(
            DOUBAO_PROFILE,
            GUID::from_u128(0x2b4d4b3a_4d4f_4c0a_8e66_7f771a2b9c10)
        );
    }
}
